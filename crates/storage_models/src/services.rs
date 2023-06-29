pub mod logger;

use std::sync::{atomic, Arc};

use common_utils::{errors::CustomResult};
use error_stack::{IntoReport, ResultExt};
#[cfg(feature = "kms")]
use external_services::kms;
use redis_interface::{errors as redis_errors, PubsubInterface, RedisValue};
use tokio::sync::oneshot;
use futures::lock::Mutex;

use crate::{self as storage, errors, kv, cache::{CacheKind, CONFIG_CACHE, ACCOUNTS_CACHE}, consts, async_spawn, connection::{PgPool, diesel_make_pg_pool}, configs::settings};


#[async_trait::async_trait]
pub trait PubSubInterface {
    async fn subscribe(&self, channel: &str) -> CustomResult<(), redis_errors::RedisError>;

    async fn publish<'a>(
        &self,
        channel: &str,
        key: CacheKind<'a>,
    ) -> CustomResult<usize, redis_errors::RedisError>;

    async fn on_message(&self) -> CustomResult<(), redis_errors::RedisError>;
}

#[async_trait::async_trait]
impl PubSubInterface for redis_interface::RedisConnectionPool {
    #[inline]
    async fn subscribe(&self, channel: &str) -> CustomResult<(), redis_errors::RedisError> {
        // Spawns a task that will automatically re-subscribe to any channels or channel patterns used by the client.
        self.subscriber.manage_subscriptions();

        self.subscriber
            .subscribe(channel)
            .await
            .into_report()
            .change_context(redis_errors::RedisError::SubscribeError)
    }

    #[inline]
    async fn publish<'a>(
        &self,
        channel: &str,
        key: CacheKind<'a>,
    ) -> CustomResult<usize, redis_errors::RedisError> {
        self.publisher
            .publish(channel, RedisValue::from(key).into_inner())
            .await
            .into_report()
            .change_context(redis_errors::RedisError::SubscribeError)
    }

    #[inline]
    async fn on_message(&self) -> CustomResult<(), redis_errors::RedisError> {
        logger::debug!("Started on message");
        let mut rx = self.subscriber.on_message();
        while let Ok(message) = rx.recv().await {
            logger::debug!("Invalidating {message:?}");
            let key: CacheKind<'_> = match RedisValue::new(message.value)
                .try_into()
                .change_context(redis_errors::RedisError::OnMessageError)
            {
                Ok(value) => value,
                Err(err) => {
                    logger::error!(value_conversion_err=?err);
                    continue;
                }
            };

            let key = match key {
                CacheKind::Config(key) => {
                    CONFIG_CACHE.invalidate(key.as_ref()).await;
                    key
                }
                CacheKind::Accounts(key) => {
                    ACCOUNTS_CACHE.invalidate(key.as_ref()).await;
                    key
                }
            };

            self.delete_key(key.as_ref())
                .await
                .map_err(|err| logger::error!("Error while deleting redis key: {err:?}"))
                .ok();

            logger::debug!("Done invalidating {key}");
        }
        Ok(())
    }
}

pub trait RedisConnInterface {
    fn get_redis_conn(&self) -> Arc<redis_interface::RedisConnectionPool>;
}

impl RedisConnInterface for Store {
    fn get_redis_conn(&self) -> Arc<redis_interface::RedisConnectionPool> {
        self.redis_conn.clone()
    }
}

#[derive(Clone)]
pub struct Store {
    pub master_pool: PgPool,
    #[cfg(feature = "olap")]
    pub replica_pool: PgPool,
    pub redis_conn: Arc<redis_interface::RedisConnectionPool>,
    #[cfg(feature = "kv_store")]
    pub(crate) config: StoreConfig,
    pub master_key: Vec<u8>,
}

#[cfg(feature = "kv_store")]
#[derive(Clone)]
pub(crate) struct StoreConfig {
    pub(crate) drainer_stream_name: String,
    pub(crate) drainer_num_partitions: u8,
}

impl Store {
    pub async fn new(
        config: &settings::Settings,
        test_transaction: bool,
        shut_down_signal: oneshot::Sender<()>,
    ) -> Self {
        let redis_conn = Arc::new(crate::connection::redis_connection(config).await);
        let redis_clone = redis_conn.clone();

        let subscriber_conn = redis_conn.clone();

        if let Err(e) = redis_conn.subscribe(consts::PUB_SUB_CHANNEL).await {
            logger::error!(subscribe_err=?e);
        }

        async_spawn!({
            if let Err(e) = subscriber_conn.on_message().await {
                logger::error!(pubsub_err=?e);
            }
        });
        async_spawn!({
            redis_clone.on_error(shut_down_signal).await;
        });

        let master_enc_key = get_master_enc_key(
            config,
            #[cfg(feature = "kms")]
            &config.kms,
        )
        .await;

        Self {
            master_pool: diesel_make_pg_pool(
                &config.master_database,
                test_transaction,
                #[cfg(feature = "kms")]
                &config.kms,
            )
            .await,
            #[cfg(feature = "olap")]
            replica_pool: diesel_make_pg_pool(
                &config.replica_database,
                test_transaction,
                #[cfg(feature = "kms")]
                &config.kms,
            )
            .await,
            redis_conn,
            #[cfg(feature = "kv_store")]
            config: StoreConfig {
                drainer_stream_name: config.drainer.stream_name.clone(),
                drainer_num_partitions: config.drainer.num_partitions,
            },
            master_key: master_enc_key,
        }
    }

    #[cfg(feature = "kv_store")]
    pub fn get_drainer_stream_name(&self, shard_key: &str) -> String {
        // Example: {shard_5}_drainer_stream
        format!("{{{}}}_{}", shard_key, self.config.drainer_stream_name,)
    }

    pub fn redis_conn(
        &self,
    ) -> CustomResult<Arc<redis_interface::RedisConnectionPool>, redis_errors::RedisError>
    {
        if self
            .redis_conn
            .is_redis_available
            .load(atomic::Ordering::SeqCst)
        {
            Ok(self.redis_conn.clone())
        } else {
            Err(redis_errors::RedisError::RedisConnectionError.into())
        }
    }

    #[cfg(feature = "kv_store")]
    pub async fn push_to_drainer_stream<T>(
        &self,
        redis_entry: kv::TypedSql,
        partition_key: crate::utils::storage_partitioning::PartitionKey<'_>,
    ) -> CustomResult<(), errors::StorageError>
    where
        T: crate::utils::storage_partitioning::KvStorePartition,
    {

        let shard_key = T::shard_key(partition_key, self.config.drainer_num_partitions);
        let stream_name = self.get_drainer_stream_name(&shard_key);
        self.redis_conn
            .stream_append_entry(
                &stream_name,
                &redis_interface::RedisEntryId::AutoGeneratedID,
                redis_entry
                    .to_field_value_pairs()
                    .change_context(errors::StorageError::KVError)?,
            )
            .await
            .change_context(errors::StorageError::KVError)
    }
}

#[allow(clippy::expect_used)]
async fn get_master_enc_key(
    conf: &crate::configs::settings::Settings,
    #[cfg(feature = "kms")] kms_config: &kms::KmsConfig,
) -> Vec<u8> {
    #[cfg(feature = "kms")]
    let master_enc_key = hex::decode(
        kms::get_kms_client(kms_config)
            .await
            .decrypt(&conf.secrets.master_enc_key)
            .await
            .expect("Failed to decrypt master enc key"),
    )
    .expect("Failed to decode from hex");

    #[cfg(not(feature = "kms"))]
    let master_enc_key =
        hex::decode(&conf.secrets.master_enc_key).expect("Failed to decode from hex");

    master_enc_key
}

#[inline]
pub fn generate_aes256_key() -> CustomResult<[u8; 32], common_utils::errors::CryptoError> {
    use ring::rand::SecureRandom;

    let rng = ring::rand::SystemRandom::new();
    let mut key: [u8; 256 / 8] = [0_u8; 256 / 8];
    rng.fill(&mut key)
        .into_report()
        .change_context(common_utils::errors::CryptoError::EncodingFailed)?;
    Ok(key)
}


#[derive(Clone)]
pub struct MockDb {
    pub addresses: Arc<Mutex<Vec<storage::Address>>>,
    pub merchant_accounts: Arc<Mutex<Vec<storage::MerchantAccount>>>,
    pub merchant_connector_accounts: Arc<Mutex<Vec<storage::MerchantConnectorAccount>>>,
    pub payment_attempts: Arc<Mutex<Vec<storage::PaymentAttempt>>>,
    pub payment_intents: Arc<Mutex<Vec<storage::PaymentIntent>>>,
    pub payment_methods: Arc<Mutex<Vec<storage::PaymentMethod>>>,
    pub customers: Arc<Mutex<Vec<storage::Customer>>>,
    pub refunds: Arc<Mutex<Vec<storage::Refund>>>,
    pub processes: Arc<Mutex<Vec<storage::ProcessTracker>>>,
    pub connector_response: Arc<Mutex<Vec<storage::ConnectorResponse>>>,
    pub redis: Arc<redis_interface::RedisConnectionPool>,
    pub api_keys: Arc<Mutex<Vec<storage::ApiKey>>>,
    pub ephemeral_keys: Arc<Mutex<Vec<storage::EphemeralKey>>>,
    pub cards_info: Arc<Mutex<Vec<storage::CardInfo>>>,
    pub events: Arc<Mutex<Vec<storage::Event>>>,
    pub disputes: Arc<Mutex<Vec<storage::Dispute>>>,
    pub lockers: Arc<Mutex<Vec<storage::LockerMockUp>>>,
    pub mandates: Arc<Mutex<Vec<storage::Mandate>>>,
}

impl MockDb {
    pub async fn new(redis: &crate::configs::settings::Settings) -> Self {
        Self {
            addresses: Default::default(),
            merchant_accounts: Default::default(),
            merchant_connector_accounts: Default::default(),
            payment_attempts: Default::default(),
            payment_intents: Default::default(),
            payment_methods: Default::default(),
            customers: Default::default(),
            refunds: Default::default(),
            processes: Default::default(),
            connector_response: Default::default(),
            redis: Arc::new(crate::connection::redis_connection(redis).await),
            api_keys: Default::default(),
            ephemeral_keys: Default::default(),
            cards_info: Default::default(),
            events: Default::default(),
            disputes: Default::default(),
            lockers: Default::default(),
            mandates: Default::default(),
        }
    }
}