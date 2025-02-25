use common_utils::{errors::CustomResult, id_type};
use diesel_models::{self as storage, enums};
use error_stack::report;
use router_env::{instrument, tracing};
use sample::mandate::MandateInterface;

use crate::{connection, errors, redis::kv_store::KvStorePartition, DatabaseStore, RouterStore};

impl KvStorePartition for storage::Mandate {}

// #[cfg(not(feature = "kv_store"))]
// mod storage {

#[async_trait::async_trait]
impl<T: DatabaseStore> MandateInterface for RouterStore<T> {
    type Error = errors::StorageError;

    #[instrument(skip_all)]
    async fn find_mandate_by_merchant_id_mandate_id(
        &self,
        merchant_id: &id_type::MerchantId,
        mandate_id: &str,
        _storage_scheme: enums::MerchantStorageScheme,
    ) -> CustomResult<storage::Mandate, errors::StorageError> {
        let conn = connection::pg_connection_read(self).await?;
        storage::Mandate::find_by_merchant_id_mandate_id(&conn, merchant_id, mandate_id)
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
    }

    #[instrument(skip_all)]
    async fn find_mandate_by_merchant_id_connector_mandate_id(
        &self,
        merchant_id: &id_type::MerchantId,
        connector_mandate_id: &str,
        _storage_scheme: enums::MerchantStorageScheme,
    ) -> CustomResult<storage::Mandate, errors::StorageError> {
        let conn = connection::pg_connection_read(self).await?;
        storage::Mandate::find_by_merchant_id_connector_mandate_id(
            &conn,
            merchant_id,
            connector_mandate_id,
        )
        .await
        .map_err(|error| report!(errors::StorageError::from(error)))
    }

    #[instrument(skip_all)]
    async fn find_mandate_by_merchant_id_customer_id(
        &self,
        merchant_id: &id_type::MerchantId,
        customer_id: &id_type::CustomerId,
    ) -> CustomResult<Vec<storage::Mandate>, errors::StorageError> {
        let conn = connection::pg_connection_read(self).await?;
        storage::Mandate::find_by_merchant_id_customer_id(&conn, merchant_id, customer_id)
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
    }

    // Need to fix this once we start moving to mandate v2
    #[cfg(all(feature = "v2", feature = "customer_v2"))]
    #[instrument(skip_all)]
    async fn find_mandate_by_global_customer_id(
        &self,
        customer_id: &id_type::GlobalCustomerId,
    ) -> CustomResult<Vec<storage::Mandate>, errors::StorageError> {
        let conn = connection::pg_connection_read(self).await?;
        storage::Mandate::find_by_global_id(&conn, customer_id)
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
    }

    #[instrument(skip_all)]
    async fn update_mandate_by_merchant_id_mandate_id(
        &self,
        merchant_id: &id_type::MerchantId,
        mandate_id: &str,
        mandate_update: storage::MandateUpdate,
        _mandate: storage::Mandate,
        _storage_scheme: enums::MerchantStorageScheme,
    ) -> CustomResult<storage::Mandate, errors::StorageError> {
        let conn = connection::pg_connection_write(self).await?;
        storage::Mandate::update_by_merchant_id_mandate_id(
            &conn,
            merchant_id,
            mandate_id,
            storage::MandateUpdateInternal::from(mandate_update),
        )
        .await
        .map_err(|error| report!(errors::StorageError::from(error)))
    }

    // #[instrument(skip_all)]
    // async fn find_mandates_by_merchant_id(
    //     &self,
    //     merchant_id: &id_type::MerchantId,
    //     mandate_constraints: api_models::mandates::MandateListConstraints,
    // ) -> CustomResult<Vec<storage::Mandate>, errors::StorageError> {
    //     let conn = connection::pg_connection_read(self).await?;
    //     storage::Mandate::filter_by_constraints(&conn, merchant_id, mandate_constraints)
    //         .await
    //         .map_err(|error| report!(errors::StorageError::from(error)))
    // }

    #[instrument(skip_all)]
    async fn insert_mandate(
        &self,
        mandate: storage::MandateNew,
        _storage_scheme: enums::MerchantStorageScheme,
    ) -> CustomResult<storage::Mandate, errors::StorageError> {
        let conn = connection::pg_connection_write(self).await?;
        mandate
            .insert(&conn)
            .await
            .map_err(|error| report!(errors::StorageError::from(error)))
    }
}
// }

// #[async_trait::async_trait]
// impl MandateInterface for MockDb {
//     async fn find_mandate_by_merchant_id_mandate_id(
//         &self,
//         merchant_id: &id_type::MerchantId,
//         mandate_id: &str,
//         _storage_scheme: enums::MerchantStorageScheme,
//     ) -> CustomResult<storage::Mandate, errors::StorageError> {
//         self.mandates
//             .lock()
//             .await
//             .iter()
//             .find(|mandate| mandate.merchant_id == *merchant_id && mandate.mandate_id == mandate_id)
//             .cloned()
//             .ok_or_else(|| errors::StorageError::ValueNotFound("mandate not found".to_string()))
//             .map_err(|err| err.into())
//     }

//     async fn find_mandate_by_merchant_id_connector_mandate_id(
//         &self,
//         merchant_id: &id_type::MerchantId,
//         connector_mandate_id: &str,
//         _storage_scheme: enums::MerchantStorageScheme,
//     ) -> CustomResult<storage::Mandate, errors::StorageError> {
//         self.mandates
//             .lock()
//             .await
//             .iter()
//             .find(|mandate| {
//                 mandate.merchant_id == *merchant_id
//                     && mandate.connector_mandate_id == Some(connector_mandate_id.to_string())
//             })
//             .cloned()
//             .ok_or_else(|| errors::StorageError::ValueNotFound("mandate not found".to_string()))
//             .map_err(|err| err.into())
//     }

//     async fn find_mandate_by_merchant_id_customer_id(
//         &self,
//         merchant_id: &id_type::MerchantId,
//         customer_id: &id_type::CustomerId,
//     ) -> CustomResult<Vec<storage::Mandate>, errors::StorageError> {
//         return Ok(self
//             .mandates
//             .lock()
//             .await
//             .iter()
//             .filter(|mandate| {
//                 mandate.merchant_id == *merchant_id && &mandate.customer_id == customer_id
//             })
//             .cloned()
//             .collect());
//     }

//     // Need to fix this once we move to v2 mandate
//     #[cfg(all(feature = "v2", feature = "customer_v2"))]
//     async fn find_mandate_by_global_customer_id(
//         &self,
//         id: &id_type::GlobalCustomerId,
//     ) -> CustomResult<Vec<storage::Mandate>, errors::StorageError> {
//         todo!()
//     }

//     async fn update_mandate_by_merchant_id_mandate_id(
//         &self,
//         merchant_id: &id_type::MerchantId,
//         mandate_id: &str,
//         mandate_update: storage::MandateUpdate,
//         _mandate: storage::Mandate,
//         _storage_scheme: enums::MerchantStorageScheme,
//     ) -> CustomResult<storage::Mandate, errors::StorageError> {
//         let mut mandates = self.mandates.lock().await;
//         match mandates
//             .iter_mut()
//             .find(|mandate| mandate.merchant_id == *merchant_id && mandate.mandate_id == mandate_id)
//         {
//             Some(mandate) => {
//                 let m_update = diesel_models::MandateUpdateInternal::from(mandate_update);
//                 let updated_mandate = m_update.clone().apply_changeset(mandate.clone());
//                 Ok(updated_mandate)
//             }
//             None => {
//                 Err(errors::StorageError::ValueNotFound("mandate not found".to_string()).into())
//             }
//         }
//     }

//     async fn find_mandates_by_merchant_id(
//         &self,
//         merchant_id: &id_type::MerchantId,
//         mandate_constraints: api_models::mandates::MandateListConstraints,
//     ) -> CustomResult<Vec<storage::Mandate>, errors::StorageError> {
//         let mandates = self.mandates.lock().await;
//         let mandates_iter = mandates.iter().filter(|mandate| {
//             let mut checker = mandate.merchant_id == *merchant_id;
//             if let Some(created_time) = mandate_constraints.created_time {
//                 checker &= mandate.created_at == created_time;
//             }
//             if let Some(created_time_lt) = mandate_constraints.created_time_lt {
//                 checker &= mandate.created_at < created_time_lt;
//             }
//             if let Some(created_time_gt) = mandate_constraints.created_time_gt {
//                 checker &= mandate.created_at > created_time_gt;
//             }
//             if let Some(created_time_lte) = mandate_constraints.created_time_lte {
//                 checker &= mandate.created_at <= created_time_lte;
//             }
//             if let Some(created_time_gte) = mandate_constraints.created_time_gte {
//                 checker &= mandate.created_at >= created_time_gte;
//             }
//             if let Some(connector) = &mandate_constraints.connector {
//                 checker &= mandate.connector == *connector;
//             }
//             if let Some(mandate_status) = mandate_constraints.mandate_status {
//                 checker &= mandate.mandate_status == mandate_status;
//             }
//             checker
//         });

//         #[allow(clippy::as_conversions)]
//         let offset = (if mandate_constraints.offset.unwrap_or(0) < 0 {
//             0
//         } else {
//             mandate_constraints.offset.unwrap_or(0)
//         }) as usize;

//         let mandates: Vec<storage::Mandate> = if let Some(limit) = mandate_constraints.limit {
//             #[allow(clippy::as_conversions)]
//             mandates_iter
//                 .skip(offset)
//                 .take((if limit < 0 { 0 } else { limit }) as usize)
//                 .cloned()
//                 .collect()
//         } else {
//             mandates_iter.skip(offset).cloned().collect()
//         };
//         Ok(mandates)
//     }

//     async fn insert_mandate(
//         &self,
//         mandate_new: storage::MandateNew,
//         _storage_scheme: enums::MerchantStorageScheme,
//     ) -> CustomResult<storage::Mandate, errors::StorageError> {
//         let mut mandates = self.mandates.lock().await;
//         let mandate = storage::Mandate {
//             mandate_id: mandate_new.mandate_id.clone(),
//             customer_id: mandate_new.customer_id,
//             merchant_id: mandate_new.merchant_id,
//             original_payment_id: mandate_new.original_payment_id,
//             payment_method_id: mandate_new.payment_method_id,
//             mandate_status: mandate_new.mandate_status,
//             mandate_type: mandate_new.mandate_type,
//             customer_accepted_at: mandate_new.customer_accepted_at,
//             customer_ip_address: mandate_new.customer_ip_address,
//             customer_user_agent: mandate_new.customer_user_agent,
//             network_transaction_id: mandate_new.network_transaction_id,
//             previous_attempt_id: mandate_new.previous_attempt_id,
//             created_at: mandate_new
//                 .created_at
//                 .unwrap_or_else(common_utils::date_time::now),
//             mandate_amount: mandate_new.mandate_amount,
//             mandate_currency: mandate_new.mandate_currency,
//             amount_captured: mandate_new.amount_captured,
//             connector: mandate_new.connector,
//             connector_mandate_id: mandate_new.connector_mandate_id,
//             start_date: mandate_new.start_date,
//             end_date: mandate_new.end_date,
//             metadata: mandate_new.metadata,
//             connector_mandate_ids: mandate_new.connector_mandate_ids,
//             merchant_connector_id: mandate_new.merchant_connector_id,
//             updated_by: mandate_new.updated_by,
//         };
//         mandates.push(mandate.clone());
//         Ok(mandate)
//     }
// }
