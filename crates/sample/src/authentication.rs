// use diesel_models::authentication::AuthenticationUpdateInternal;
// use error_stack::report;
// use router_env::{instrument, tracing};

// use hyperswitch_domain_models::errors;
use common_utils::errors::CustomResult;
use diesel_models::authentication as storage;

#[async_trait::async_trait]
#[allow(dead_code)]
pub trait AuthenticationInterface {
    type Error;
    async fn insert_authentication(
        &self,
        authentication: storage::AuthenticationNew,
    ) -> CustomResult<storage::Authentication, Self::Error>;

    async fn find_authentication_by_merchant_id_authentication_id(
        &self,
        merchant_id: &common_utils::id_type::MerchantId,
        authentication_id: String,
    ) -> CustomResult<storage::Authentication, Self::Error>;

    async fn find_authentication_by_merchant_id_connector_authentication_id(
        &self,
        merchant_id: common_utils::id_type::MerchantId,
        connector_authentication_id: String,
    ) -> CustomResult<storage::Authentication, Self::Error>;

    async fn update_authentication_by_merchant_id_authentication_id(
        &self,
        previous_state: storage::Authentication,
        authentication_update: storage::AuthenticationUpdate,
    ) -> CustomResult<storage::Authentication, Self::Error>;
}

// #[async_trait::async_trait]
// impl AuthenticationInterface for MockDb {
//     async fn insert_authentication(
//         &self,
//         authentication: storage::AuthenticationNew,
//     ) -> CustomResult<storage::Authentication, errors::StorageError> {
//         let mut authentications = self.authentications.lock().await;
//         if authentications.iter().any(|authentication_inner| {
//             authentication_inner.authentication_id == authentication.authentication_id
//         }) {
//             Err(errors::StorageError::DuplicateValue {
//                 entity: "authentication_id",
//                 key: Some(authentication.authentication_id.clone()),
//             })?
//         }
//         let authentication = storage::Authentication {
//             created_at: common_utils::date_time::now(),
//             modified_at: common_utils::date_time::now(),
//             authentication_id: authentication.authentication_id,
//             merchant_id: authentication.merchant_id,
//             authentication_status: authentication.authentication_status,
//             authentication_connector: authentication.authentication_connector,
//             connector_authentication_id: authentication.connector_authentication_id,
//             authentication_data: None,
//             payment_method_id: authentication.payment_method_id,
//             authentication_type: authentication.authentication_type,
//             authentication_lifecycle_status: authentication.authentication_lifecycle_status,
//             error_code: authentication.error_code,
//             error_message: authentication.error_message,
//             connector_metadata: authentication.connector_metadata,
//             maximum_supported_version: authentication.maximum_supported_version,
//             threeds_server_transaction_id: authentication.threeds_server_transaction_id,
//             cavv: authentication.cavv,
//             authentication_flow_type: authentication.authentication_flow_type,
//             message_version: authentication.message_version,
//             eci: authentication.eci,
//             trans_status: authentication.trans_status,
//             acquirer_bin: authentication.acquirer_bin,
//             acquirer_merchant_id: authentication.acquirer_merchant_id,
//             three_ds_method_data: authentication.three_ds_method_data,
//             three_ds_method_url: authentication.three_ds_method_url,
//             acs_url: authentication.acs_url,
//             challenge_request: authentication.challenge_request,
//             acs_reference_number: authentication.acs_reference_number,
//             acs_trans_id: authentication.acs_trans_id,
//             acs_signed_content: authentication.acs_signed_content,
//             profile_id: authentication.profile_id,
//             payment_id: authentication.payment_id,
//             merchant_connector_id: authentication.merchant_connector_id,
//             ds_trans_id: authentication.ds_trans_id,
//             directory_server_id: authentication.directory_server_id,
//             acquirer_country_code: authentication.acquirer_country_code,
//             service_details: authentication.service_details,
//             organization_id: authentication.organization_id,
//         };
//         authentications.push(authentication.clone());
//         Ok(authentication)
//     }

//     async fn find_authentication_by_merchant_id_authentication_id(
//         &self,
//         merchant_id: &common_utils::id_type::MerchantId,
//         authentication_id: String,
//     ) -> CustomResult<storage::Authentication, errors::StorageError> {
//         let authentications = self.authentications.lock().await;
//         authentications
//             .iter()
//             .find(|a| a.merchant_id == *merchant_id && a.authentication_id == authentication_id)
//             .ok_or(
//                 errors::StorageError::ValueNotFound(format!(
//                     "cannot find authentication for authentication_id = {authentication_id} and merchant_id = {merchant_id:?}"
//                 )).into(),
//             ).cloned()
//     }

//     async fn find_authentication_by_merchant_id_connector_authentication_id(
//         &self,
//         _merchant_id: common_utils::id_type::MerchantId,
//         _connector_authentication_id: String,
//     ) -> CustomResult<storage::Authentication, errors::StorageError> {
//         Err(errors::StorageError::MockDbError)?
//     }

//     async fn update_authentication_by_merchant_id_authentication_id(
//         &self,
//         previous_state: storage::Authentication,
//         authentication_update: storage::AuthenticationUpdate,
//     ) -> CustomResult<storage::Authentication, errors::StorageError> {
//         let mut authentications = self.authentications.lock().await;
//         let authentication_id = previous_state.authentication_id.clone();
//         let merchant_id = previous_state.merchant_id.clone();
//         authentications
//             .iter_mut()
//             .find(|authentication| authentication.authentication_id == authentication_id && authentication.merchant_id == merchant_id)
//             .map(|authentication| {
//                 let authentication_update_internal =
//                     AuthenticationUpdateInternal::from(authentication_update);
//                 let updated_authentication = authentication_update_internal.apply_changeset(previous_state);
//                 *authentication = updated_authentication.clone();
//                 updated_authentication
//             })
//             .ok_or(
//                 errors::StorageError::ValueNotFound(format!(
//                     "cannot find authentication for authentication_id = {authentication_id} and merchant_id = {merchant_id:?}"
//                 ))
//                 .into(),
//             )
//     }
// }
