use common_utils::{pii, id_type};
use serde::{self, Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallbackMapper {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub data: pii::SecretSerdeValue,
    pub created_at: time::PrimitiveDateTime,
    pub last_modified_at: time::PrimitiveDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CallBackMapperData {
    NetworkTokenWebhook {
        merchant_id: id_type::MerchantId,
        payment_method_id: String,
        customer_id: id_type::CustomerId,
    },
}
