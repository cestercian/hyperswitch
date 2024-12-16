use api_models::payments;
use base64::Engine;
use common_utils::pii;
use masking::{ExposeInterface, PeekInterface, Secret};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    connector::utils::{
        self, AddressDetailsData, ApplePayDecrypt, CardData, CardIssuer,
        PaymentsAuthorizeRequestData, PaymentsSetupMandateRequestData, PaymentsSyncRequestData,
        RecurringMandateData, RouterData,
    },
    consts,
    core::errors,
    types::{
        self,
        api::{self, enums as api_enums},
        domain,
        storage::enums,
        transformers::ForeignFrom,
        ApplePayPredecryptData,
    },
    unimplemented_payment_method,
};

pub struct BankOfAmericaAuthType {
    pub(super) api_key: Secret<String>,
    pub(super) merchant_account: Secret<String>,
    pub(super) api_secret: Secret<String>,
}

impl TryFrom<&types::ConnectorAuthType> for BankOfAmericaAuthType {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(auth_type: &types::ConnectorAuthType) -> Result<Self, Self::Error> {
        if let types::ConnectorAuthType::SignatureKey {
            api_key,
            key1,
            api_secret,
        } = auth_type
        {
            Ok(Self {
                api_key: api_key.to_owned(),
                merchant_account: key1.to_owned(),
                api_secret: api_secret.to_owned(),
            })
        } else {
            Err(errors::ConnectorError::FailedToObtainAuthType)?
        }
    }
}

pub struct BankOfAmericaRouterData<T> {
    pub amount: String,
    pub router_data: T,
}

impl<T> TryFrom<(&api::CurrencyUnit, enums::Currency, i64, T)> for BankOfAmericaRouterData<T> {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (currency_unit, currency, amount, item): (&api::CurrencyUnit, enums::Currency, i64, T),
    ) -> Result<Self, Self::Error> {
        let amount = utils::get_amount_as_string(currency_unit, amount, currency)?;
        Ok(Self {
            amount,
            router_data: item,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaPaymentsRequest {
    processing_information: ProcessingInformation,
    payment_information: PaymentInformation,
    order_information: OrderInformationWithBill,
    client_reference_information: ClientReferenceInformation,
    #[serde(skip_serializing_if = "Option::is_none")]
    consumer_authentication_information: Option<BankOfAmericaConsumerAuthInformation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    merchant_defined_information: Option<Vec<MerchantDefinedInformation>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingInformation {
    action_list: Option<Vec<BankOfAmericaActionsList>>,
    action_token_types: Option<Vec<BankOfAmericaActionsTokenType>>,
    authorization_options: Option<BankOfAmericaAuthorizationOptions>,
    commerce_indicator: String,
    capture: Option<bool>,
    capture_options: Option<CaptureOptions>,
    payment_solution: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BankOfAmericaActionsList {
    TokenCreate,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BankOfAmericaActionsTokenType {
    PaymentInstrument,
    Customer,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaAuthorizationOptions {
    initiator: Option<BankOfAmericaPaymentInitiator>,
    merchant_intitiated_transaction: Option<MerchantInitiatedTransaction>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaPaymentInitiator {
    #[serde(rename = "type")]
    initiator_type: Option<BankOfAmericaPaymentInitiatorTypes>,
    credential_stored_on_file: Option<bool>,
    stored_credential_used: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BankOfAmericaPaymentInitiatorTypes {
    Customer,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantInitiatedTransaction {
    reason: Option<String>,
    //Required for recurring mandates payment
    original_authorized_amount: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantDefinedInformation {
    key: u8,
    value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaConsumerAuthInformation {
    ucaf_collection_indicator: Option<String>,
    cavv: Option<String>,
    ucaf_authentication_data: Option<Secret<String>>,
    xid: Option<String>,
    directory_server_transaction_id: Option<Secret<String>>,
    specification_version: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOptions {
    capture_sequence_number: u32,
    total_capture_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankOfAmericaPaymentInstrument {
    id: Secret<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardPaymentInformation {
    card: Card,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GooglePayPaymentInformation {
    fluid_data: FluidData,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplePayTokenizedCard {
    transaction_type: TransactionType,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplePayTokenPaymentInformation {
    fluid_data: FluidData,
    tokenized_card: ApplePayTokenizedCard,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplePayPaymentInformation {
    tokenized_card: TokenizedCard,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum PaymentInformation {
    Cards(Box<CardPaymentInformation>),
    GooglePay(Box<GooglePayPaymentInformation>),
    ApplePay(Box<ApplePayPaymentInformation>),
    ApplePayToken(Box<ApplePayTokenPaymentInformation>),
    MandatePayment(Box<MandatePaymentInformation>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MandatePaymentInformation {
    payment_instrument: BankOfAmericaPaymentInstrument,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    number: cards::CardNumber,
    expiration_month: Secret<String>,
    expiration_year: Secret<String>,
    security_code: Secret<String>,
    #[serde(rename = "type")]
    card_type: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenizedCard {
    number: Secret<String>,
    expiration_month: Secret<String>,
    expiration_year: Secret<String>,
    cryptogram: Secret<String>,
    transaction_type: TransactionType,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FluidData {
    value: Secret<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderInformationWithBill {
    amount_details: Amount,
    bill_to: Option<BillTo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Amount {
    total_amount: String,
    currency: api_models::enums::Currency,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillTo {
    first_name: Option<Secret<String>>,
    last_name: Option<Secret<String>>,
    address1: Option<Secret<String>>,
    locality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    administrative_area: Option<Secret<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    postal_code: Option<Secret<String>>,
    country: Option<api_enums::CountryAlpha2>,
    email: pii::Email,
}

impl TryFrom<&types::SetupMandateRouterData> for BankOfAmericaPaymentsRequest {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(item: &types::SetupMandateRouterData) -> Result<Self, Self::Error> {
        match item.request.payment_method_data.clone() {
            domain::PaymentMethodData::Card(card_data) => Self::try_from((item, card_data)),
            domain::PaymentMethodData::Wallet(wallet_data) => match wallet_data {
                domain::WalletData::ApplePay(apple_pay_data) => {
                    Self::try_from((item, apple_pay_data))
                }
                domain::WalletData::GooglePay(google_pay_data) => {
                    Self::try_from((item, google_pay_data))
                }
                domain::WalletData::AliPayQr(_)
                | domain::WalletData::AliPayRedirect(_)
                | domain::WalletData::AliPayHkRedirect(_)
                | domain::WalletData::MomoRedirect(_)
                | domain::WalletData::KakaoPayRedirect(_)
                | domain::WalletData::GoPayRedirect(_)
                | domain::WalletData::GcashRedirect(_)
                | domain::WalletData::ApplePayRedirect(_)
                | domain::WalletData::ApplePayThirdPartySdk(_)
                | domain::WalletData::DanaRedirect {}
                | domain::WalletData::GooglePayRedirect(_)
                | domain::WalletData::GooglePayThirdPartySdk(_)
                | domain::WalletData::MbWayRedirect(_)
                | domain::WalletData::MobilePayRedirect(_)
                | domain::WalletData::PaypalRedirect(_)
                | domain::WalletData::PaypalSdk(_)
                | domain::WalletData::Paze(_)
                | domain::WalletData::SamsungPay(_)
                | domain::WalletData::TwintRedirect {}
                | domain::WalletData::VippsRedirect {}
                | domain::WalletData::TouchNGoRedirect(_)
                | domain::WalletData::WeChatPayRedirect(_)
                | domain::WalletData::WeChatPayQr(_)
                | domain::WalletData::CashappQr(_)
                | domain::WalletData::SwishQr(_)
                | domain::WalletData::Mifinity(_) => Err(errors::ConnectorError::NotImplemented(
                    utils::get_unimplemented_payment_method_error_message("BankOfAmerica"),
                ))?,
            },
            domain::PaymentMethodData::CardRedirect(_)
            | domain::PaymentMethodData::PayLater(_)
            | domain::PaymentMethodData::BankRedirect(_)
            | domain::PaymentMethodData::BankDebit(_)
            | domain::PaymentMethodData::BankTransfer(_)
            | domain::PaymentMethodData::Crypto(_)
            | domain::PaymentMethodData::MandatePayment
            | domain::PaymentMethodData::Reward
            | domain::PaymentMethodData::RealTimePayment(_)
            | domain::PaymentMethodData::MobilePayment(_)
            | domain::PaymentMethodData::Upi(_)
            | domain::PaymentMethodData::Voucher(_)
            | domain::PaymentMethodData::GiftCard(_)
            | domain::PaymentMethodData::OpenBanking(_)
            | domain::PaymentMethodData::CardToken(_)
            | domain::PaymentMethodData::NetworkToken(_)
            | domain::PaymentMethodData::CardDetailsForNetworkTransactionId(_) => {
                Err(errors::ConnectorError::NotImplemented(
                    utils::get_unimplemented_payment_method_error_message("BankOfAmerica"),
                ))?
            }
        }
    }
}

impl<F, T>
    TryFrom<
        types::ResponseRouterData<
            F,
            BankOfAmericaSetupMandatesResponse,
            T,
            types::PaymentsResponseData,
        >,
    > for types::RouterData<F, T, types::PaymentsResponseData>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: types::ResponseRouterData<
            F,
            BankOfAmericaSetupMandatesResponse,
            T,
            types::PaymentsResponseData,
        >,
    ) -> Result<Self, Self::Error> {
        match item.response {
            BankOfAmericaSetupMandatesResponse::ClientReferenceInformation(info_response) => {
                let mandate_reference = info_response.token_information.clone().map(|token_info| {
                    types::MandateReference {
                        connector_mandate_id: token_info
                            .payment_instrument
                            .map(|payment_instrument| payment_instrument.id.expose()),
                        payment_method_id: None,
                        mandate_metadata: None,
                        connector_mandate_request_reference_id: None,
                    }
                });
                let mut mandate_status =
                    enums::AttemptStatus::foreign_from((info_response.status.clone(), false));
                if matches!(mandate_status, enums::AttemptStatus::Authorized) {
                    //In case of zero auth mandates we want to make the payment reach the terminal status so we are converting the authorized status to charged as well.
                    mandate_status = enums::AttemptStatus::Charged
                }
                let error_response =
                    get_error_response_if_failure((&info_response, mandate_status, item.http_code));

                let connector_response = match item.data.payment_method {
                    common_enums::PaymentMethod::Card => info_response
                        .processor_information
                        .as_ref()
                        .and_then(|processor_information| {
                            info_response
                                .consumer_authentication_information
                                .as_ref()
                                .map(|consumer_auth_information| {
                                    types::AdditionalPaymentMethodConnectorResponse::foreign_from((
                                        processor_information,
                                        consumer_auth_information,
                                    ))
                                })
                        })
                        .map(types::ConnectorResponseData::with_additional_payment_method_data),
                    common_enums::PaymentMethod::CardRedirect
                    | common_enums::PaymentMethod::PayLater
                    | common_enums::PaymentMethod::Wallet
                    | common_enums::PaymentMethod::BankRedirect
                    | common_enums::PaymentMethod::BankTransfer
                    | common_enums::PaymentMethod::Crypto
                    | common_enums::PaymentMethod::BankDebit
                    | common_enums::PaymentMethod::Reward
                    | common_enums::PaymentMethod::RealTimePayment
                    | common_enums::PaymentMethod::MobilePayment
                    | common_enums::PaymentMethod::Upi
                    | common_enums::PaymentMethod::Voucher
                    | common_enums::PaymentMethod::OpenBanking
                    | common_enums::PaymentMethod::GiftCard => None,
                };

                Ok(Self {
                    status: mandate_status,
                    response: match error_response {
                        Some(error) => Err(error),
                        None => Ok(types::PaymentsResponseData::TransactionResponse {
                            resource_id: types::ResponseId::ConnectorTransactionId(
                                info_response.id.clone(),
                            ),
                            redirection_data: Box::new(None),
                            mandate_reference: Box::new(mandate_reference),
                            connector_metadata: None,
                            network_txn_id: None,
                            connector_response_reference_id: Some(
                                info_response
                                    .client_reference_information
                                    .code
                                    .clone()
                                    .unwrap_or(info_response.id),
                            ),
                            incremental_authorization_allowed: None,
                            charge_id: None,
                        }),
                    },
                    connector_response,
                    ..item.data
                })
            }
            BankOfAmericaSetupMandatesResponse::ErrorInformation(error_response) => {
                let response = Err(types::ErrorResponse::foreign_from((
                    &*error_response,
                    item.http_code,
                )));
                Ok(Self {
                    response,
                    status: enums::AttemptStatus::Failure,
                    ..item.data
                })
            }
        }
    }
}

// for bankofamerica each item in Billing is mandatory
// fn build_bill_to(
//     address_details: &payments::Address,
//     email: pii::Email,
// ) -> Result<BillTo, error_stack::Report<errors::ConnectorError>> {
//     let address = address_details
//         .address
//         .as_ref()
//         .ok_or_else(utils::missing_field_err("billing.address"))?;

//     let country = address.get_country()?.to_owned();
//     let first_name = address.get_first_name()?;

//     let (administrative_area, postal_code) =
//         if country == api_enums::CountryAlpha2::US || country == api_enums::CountryAlpha2::CA {
//             let mut state = address.to_state_code()?.peek().clone();
//             state.truncate(20);
//             (
//                 Some(Secret::from(state)),
//                 Some(address.get_zip()?.to_owned()),
//             )
//         } else {
//             let zip = address.zip.clone();
//             let mut_state = address.state.clone().map(|state| state.expose());
//             match mut_state {
//                 Some(mut state) => {
//                     state.truncate(20);
//                     (Some(Secret::from(state)), zip)
//                 }
//                 None => (None, zip),
//             }
//         };
//     Ok(BillTo {
//         first_name: first_name.clone(),
//         last_name: address.get_last_name().unwrap_or(first_name).clone(),
//         address1: address.get_line1()?.to_owned(),
//         locality: Secret::new(address.get_city()?.to_owned()),
//         administrative_area,
//         postal_code,
//         country,
//         email,
//     })
// }

fn build_bill_to(
    address_details: Option<&payments::Address>,
    email: pii::Email,
) -> Result<BillTo, error_stack::Report<errors::ConnectorError>> {
    let default_address = BillTo {
        first_name: None,
        last_name: None,
        address1: None,
        locality: None,
        administrative_area: None,
        postal_code: None,
        country: None,
        email: email.clone(),
    };
    Ok(address_details
        .and_then(|addr| {
            addr.address.as_ref().map(|addr| BillTo {
                first_name: addr.first_name.clone().map(From::from),
                last_name: addr.last_name.clone().map(From::from),
                address1: addr.line1.clone(),
                locality: addr.city.clone(),
                administrative_area: addr.to_state_code_as_optional().ok().flatten(),
                postal_code: addr.zip.clone(),
                country: addr.country,
                email,
            })
        })
        .unwrap_or(default_address))
}

impl From<CardIssuer> for String {
    fn from(card_issuer: CardIssuer) -> Self {
        let card_type = match card_issuer {
            CardIssuer::AmericanExpress => "003",
            CardIssuer::Master => "002",
            //"042" is the type code for Masetro Cards(International). For Maestro Cards(UK-Domestic) the mapping should be "024"
            CardIssuer::Maestro => "042",
            CardIssuer::Visa => "001",
            CardIssuer::Discover => "004",
            CardIssuer::DinersClub => "005",
            CardIssuer::CarteBlanche => "006",
            CardIssuer::JCB => "007",
        };
        card_type.to_string()
    }
}

fn get_boa_card_type(card_network: common_enums::CardNetwork) -> Option<&'static str> {
    match card_network {
        common_enums::CardNetwork::Visa => Some("001"),
        common_enums::CardNetwork::Mastercard => Some("002"),
        common_enums::CardNetwork::AmericanExpress => Some("003"),
        common_enums::CardNetwork::JCB => Some("007"),
        common_enums::CardNetwork::DinersClub => Some("005"),
        common_enums::CardNetwork::Discover => Some("004"),
        common_enums::CardNetwork::CartesBancaires => Some("006"),
        common_enums::CardNetwork::UnionPay => Some("062"),
        //"042" is the type code for Masetro Cards(International). For Maestro Cards(UK-Domestic) the mapping should be "024"
        common_enums::CardNetwork::Maestro => Some("042"),
        common_enums::CardNetwork::Interac | common_enums::CardNetwork::RuPay => None,
    }
}

#[derive(Debug, Serialize)]
pub enum PaymentSolution {
    ApplePay,
    GooglePay,
}

impl From<PaymentSolution> for String {
    fn from(solution: PaymentSolution) -> Self {
        let payment_solution = match solution {
            PaymentSolution::ApplePay => "001",
            PaymentSolution::GooglePay => "012",
        };
        payment_solution.to_string()
    }
}

#[derive(Debug, Serialize)]
pub enum TransactionType {
    #[serde(rename = "1")]
    ApplePay,
}

impl
    From<(
        &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
        Option<BillTo>,
    )> for OrderInformationWithBill
{
    fn from(
        (item, bill_to): (
            &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
            Option<BillTo>,
        ),
    ) -> Self {
        Self {
            amount_details: Amount {
                total_amount: item.amount.to_owned(),
                currency: item.router_data.request.currency,
            },
            bill_to,
        }
    }
}

impl
    TryFrom<(
        &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
        Option<PaymentSolution>,
        Option<String>,
    )> for ProcessingInformation
{
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(
        (item, solution, network): (
            &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
            Option<PaymentSolution>,
            Option<String>,
        ),
    ) -> Result<Self, Self::Error> {
        let (action_list, action_token_types, authorization_options) = if item
            .router_data
            .request
            .setup_future_usage
            .map_or(false, |future_usage| {
                matches!(future_usage, common_enums::FutureUsage::OffSession)
            })
            && (item.router_data.request.customer_acceptance.is_some()
                || item
                    .router_data
                    .request
                    .setup_mandate_details
                    .clone()
                    .map_or(false, |mandate_details| {
                        mandate_details.customer_acceptance.is_some()
                    })) {
            get_boa_mandate_action_details()
        } else if item.router_data.request.connector_mandate_id().is_some() {
            let original_amount = item
                .router_data
                .get_recurring_mandate_payment_data()?
                .get_original_payment_amount()?;
            let original_currency = item
                .router_data
                .get_recurring_mandate_payment_data()?
                .get_original_payment_currency()?;
            (
                None,
                None,
                Some(BankOfAmericaAuthorizationOptions {
                    initiator: None,
                    merchant_intitiated_transaction: Some(MerchantInitiatedTransaction {
                        reason: None,
                        original_authorized_amount: Some(utils::get_amount_as_string(
                            &api::CurrencyUnit::Base,
                            original_amount,
                            original_currency,
                        )?),
                    }),
                }),
            )
        } else {
            (None, None, None)
        };

        let commerce_indicator = get_commerce_indicator(network);

        Ok(Self {
            capture: Some(matches!(
                item.router_data.request.capture_method,
                Some(enums::CaptureMethod::Automatic) | None
            )),
            payment_solution: solution.map(String::from),
            action_list,
            action_token_types,
            authorization_options,
            capture_options: None,
            commerce_indicator,
        })
    }
}

impl From<&BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>>
    for ClientReferenceInformation
{
    fn from(item: &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>) -> Self {
        Self {
            code: Some(item.router_data.connector_request_reference_id.clone()),
        }
    }
}

impl From<&types::SetupMandateRouterData> for ClientReferenceInformation {
    fn from(item: &types::SetupMandateRouterData) -> Self {
        Self {
            code: Some(item.connector_request_reference_id.clone()),
        }
    }
}

impl ForeignFrom<Value> for Vec<MerchantDefinedInformation> {
    fn foreign_from(metadata: Value) -> Self {
        let hashmap: std::collections::BTreeMap<String, Value> =
            serde_json::from_str(&metadata.to_string())
                .unwrap_or(std::collections::BTreeMap::new());
        let mut vector: Self = Self::new();
        let mut iter = 1;
        for (key, value) in hashmap {
            vector.push(MerchantDefinedInformation {
                key: iter,
                value: format!("{key}={value}"),
            });
            iter += 1;
        }
        vector
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientReferenceInformation {
    code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientProcessorInformation {
    avs: Option<Avs>,
    card_verification: Option<CardVerification>,
    processor: Option<ProcessorResponse>,
    network_transaction_id: Option<Secret<String>>,
    approval_code: Option<String>,
    merchant_advice: Option<MerchantAdvice>,
    response_code: Option<String>,
    ach_verification: Option<AchVerification>,
    system_trace_audit_number: Option<String>,
    event_status: Option<String>,
    retrieval_reference_number: Option<String>,
    consumer_authentication_response: Option<ConsumerAuthenticationResponse>,
    response_details: Option<String>,
    transaction_id: Option<Secret<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantAdvice {
    code: Option<String>,
    code_raw: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerAuthenticationResponse {
    code: Option<String>,
    code_raw: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchVerification {
    result_code_raw: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorResponse {
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardVerification {
    result_code: Option<String>,
    result_code_raw: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRiskInformation {
    rules: Option<Vec<ClientRiskInformationRules>>,
    profile: Option<Profile>,
    score: Option<Score>,
    info_codes: Option<InfoCodes>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoCodes {
    address: Option<Vec<String>>,
    identity_change: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    factor_codes: Option<Vec<String>>,
    result: Option<RiskResult>,
    model_used: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RiskResult {
    StringVariant(String),
    IntVariant(u64),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    early_decision: Option<String>,
    name: Option<String>,
    decision: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientRiskInformationRules {
    name: Option<Secret<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avs {
    code: Option<String>,
    code_raw: Option<String>,
}

impl
    TryFrom<(
        &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
        domain::Card,
    )> for BankOfAmericaPaymentsRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (item, ccard): (
            &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
            domain::Card,
        ),
    ) -> Result<Self, Self::Error> {
        let email = item.router_data.request.get_email()?;
        let bill_to = build_bill_to(item.router_data.get_optional_billing(), email)?;
        let order_information = OrderInformationWithBill::from((item, Some(bill_to)));
        let payment_information = PaymentInformation::try_from(&ccard)?;
        let processing_information = ProcessingInformation::try_from((item, None, None))?;
        let client_reference_information = ClientReferenceInformation::from(item);
        let merchant_defined_information = item
            .router_data
            .request
            .metadata
            .clone()
            .map(Vec::<MerchantDefinedInformation>::foreign_from);

        Ok(Self {
            processing_information,
            payment_information,
            order_information,
            client_reference_information,
            merchant_defined_information,
            consumer_authentication_information: None,
        })
    }
}

impl
    TryFrom<(
        &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
        Box<ApplePayPredecryptData>,
        domain::ApplePayWalletData,
    )> for BankOfAmericaPaymentsRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (item, apple_pay_data, apple_pay_wallet_data): (
            &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
            Box<ApplePayPredecryptData>,
            domain::ApplePayWalletData,
        ),
    ) -> Result<Self, Self::Error> {
        let email = item.router_data.request.get_email()?;
        let bill_to = build_bill_to(item.router_data.get_optional_billing(), email)?;
        let order_information = OrderInformationWithBill::from((item, Some(bill_to)));
        let processing_information = ProcessingInformation::try_from((
            item,
            Some(PaymentSolution::ApplePay),
            Some(apple_pay_wallet_data.payment_method.network.clone()),
        ))?;
        let client_reference_information = ClientReferenceInformation::from(item);
        let payment_information = PaymentInformation::try_from(&apple_pay_data)?;
        let merchant_defined_information = item
            .router_data
            .request
            .metadata
            .clone()
            .map(Vec::<MerchantDefinedInformation>::foreign_from);
        let ucaf_collection_indicator = match apple_pay_wallet_data
            .payment_method
            .network
            .to_lowercase()
            .as_str()
        {
            "mastercard" => Some("2".to_string()),
            _ => None,
        };
        Ok(Self {
            processing_information,
            payment_information,
            order_information,
            client_reference_information,
            merchant_defined_information,
            consumer_authentication_information: Some(BankOfAmericaConsumerAuthInformation {
                ucaf_collection_indicator,
                cavv: None,
                ucaf_authentication_data: None,
                xid: None,
                directory_server_transaction_id: None,
                specification_version: None,
            }),
        })
    }
}

impl
    TryFrom<(
        &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
        domain::GooglePayWalletData,
    )> for BankOfAmericaPaymentsRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (item, google_pay_data): (
            &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
            domain::GooglePayWalletData,
        ),
    ) -> Result<Self, Self::Error> {
        let email = item.router_data.request.get_email()?;
        let bill_to = build_bill_to(item.router_data.get_optional_billing(), email)?;
        let order_information = OrderInformationWithBill::from((item, Some(bill_to)));
        let payment_information = PaymentInformation::from(&google_pay_data);
        let processing_information =
            ProcessingInformation::try_from((item, Some(PaymentSolution::GooglePay), None))?;
        let client_reference_information = ClientReferenceInformation::from(item);
        let merchant_defined_information = item
            .router_data
            .request
            .metadata
            .clone()
            .map(Vec::<MerchantDefinedInformation>::foreign_from);

        Ok(Self {
            processing_information,
            payment_information,
            order_information,
            client_reference_information,
            merchant_defined_information,
            consumer_authentication_information: None,
        })
    }
}

impl TryFrom<&BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>>
    for BankOfAmericaPaymentsRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
    ) -> Result<Self, Self::Error> {
        match item.router_data.request.connector_mandate_id() {
            Some(connector_mandate_id) => Self::try_from((item, connector_mandate_id)),
            None => {
                match item.router_data.request.payment_method_data.clone() {
                    domain::PaymentMethodData::Card(ccard) => Self::try_from((item, ccard)),
                    domain::PaymentMethodData::Wallet(wallet_data) => match wallet_data {
                        domain::WalletData::ApplePay(apple_pay_data) => {
                            match item.router_data.payment_method_token.clone() {
                                Some(payment_method_token) => match payment_method_token {
                                    types::PaymentMethodToken::ApplePayDecrypt(decrypt_data) => {
                                        Self::try_from((item, decrypt_data, apple_pay_data))
                                    }
                                    types::PaymentMethodToken::Token(_) => {
                                        Err(unimplemented_payment_method!(
                                            "Apple Pay",
                                            "Manual",
                                            "Bank Of America"
                                        ))?
                                    }
                                    types::PaymentMethodToken::PazeDecrypt(_) => Err(
                                        unimplemented_payment_method!("Paze", "Bank Of America"),
                                    )?,
                                },
                                None => {
                                    let email = item.router_data.request.get_email()?;
                                    let bill_to = build_bill_to(
                                        item.router_data.get_optional_billing(),
                                        email,
                                    )?;
                                    let order_information: OrderInformationWithBill =
                                        OrderInformationWithBill::from((item, Some(bill_to)));
                                    let processing_information =
                                        ProcessingInformation::try_from((
                                            item,
                                            Some(PaymentSolution::ApplePay),
                                            Some(apple_pay_data.payment_method.network.clone()),
                                        ))?;
                                    let client_reference_information =
                                        ClientReferenceInformation::from(item);
                                    let payment_information =
                                        PaymentInformation::from(&apple_pay_data);
                                    let merchant_defined_information =
                                        item.router_data.request.metadata.clone().map(|metadata| {
                                            Vec::<MerchantDefinedInformation>::foreign_from(
                                                metadata,
                                            )
                                        });
                                    let ucaf_collection_indicator = match apple_pay_data
                                        .payment_method
                                        .network
                                        .to_lowercase()
                                        .as_str()
                                    {
                                        "mastercard" => Some("2".to_string()),
                                        _ => None,
                                    };
                                    Ok(Self {
                                        processing_information,
                                        payment_information,
                                        order_information,
                                        merchant_defined_information,
                                        client_reference_information,
                                        consumer_authentication_information: Some(
                                            BankOfAmericaConsumerAuthInformation {
                                                ucaf_collection_indicator,
                                                cavv: None,
                                                ucaf_authentication_data: None,
                                                xid: None,
                                                directory_server_transaction_id: None,
                                                specification_version: None,
                                            },
                                        ),
                                    })
                                }
                            }
                        }
                        domain::WalletData::GooglePay(google_pay_data) => {
                            Self::try_from((item, google_pay_data))
                        }

                        domain::WalletData::AliPayQr(_)
                        | domain::WalletData::AliPayRedirect(_)
                        | domain::WalletData::AliPayHkRedirect(_)
                        | domain::WalletData::MomoRedirect(_)
                        | domain::WalletData::KakaoPayRedirect(_)
                        | domain::WalletData::GoPayRedirect(_)
                        | domain::WalletData::GcashRedirect(_)
                        | domain::WalletData::ApplePayRedirect(_)
                        | domain::WalletData::ApplePayThirdPartySdk(_)
                        | domain::WalletData::DanaRedirect {}
                        | domain::WalletData::GooglePayRedirect(_)
                        | domain::WalletData::GooglePayThirdPartySdk(_)
                        | domain::WalletData::MbWayRedirect(_)
                        | domain::WalletData::MobilePayRedirect(_)
                        | domain::WalletData::PaypalRedirect(_)
                        | domain::WalletData::PaypalSdk(_)
                        | domain::WalletData::Paze(_)
                        | domain::WalletData::SamsungPay(_)
                        | domain::WalletData::TwintRedirect {}
                        | domain::WalletData::VippsRedirect {}
                        | domain::WalletData::TouchNGoRedirect(_)
                        | domain::WalletData::WeChatPayRedirect(_)
                        | domain::WalletData::WeChatPayQr(_)
                        | domain::WalletData::CashappQr(_)
                        | domain::WalletData::SwishQr(_)
                        | domain::WalletData::Mifinity(_) => {
                            Err(errors::ConnectorError::NotImplemented(
                                utils::get_unimplemented_payment_method_error_message(
                                    "Bank of America",
                                ),
                            )
                            .into())
                        }
                    },
                    // If connector_mandate_id is present MandatePayment will be the PMD, the case will be handled in the first `if` clause.
                    // This is a fallback implementation in the event of catastrophe.
                    domain::PaymentMethodData::MandatePayment => {
                        let connector_mandate_id =
                            item.router_data.request.connector_mandate_id().ok_or(
                                errors::ConnectorError::MissingRequiredField {
                                    field_name: "connector_mandate_id",
                                },
                            )?;
                        Self::try_from((item, connector_mandate_id))
                    }
                    domain::PaymentMethodData::CardRedirect(_)
                    | domain::PaymentMethodData::PayLater(_)
                    | domain::PaymentMethodData::BankRedirect(_)
                    | domain::PaymentMethodData::BankDebit(_)
                    | domain::PaymentMethodData::BankTransfer(_)
                    | domain::PaymentMethodData::Crypto(_)
                    | domain::PaymentMethodData::Reward
                    | domain::PaymentMethodData::RealTimePayment(_)
                    | domain::PaymentMethodData::MobilePayment(_)
                    | domain::PaymentMethodData::Upi(_)
                    | domain::PaymentMethodData::Voucher(_)
                    | domain::PaymentMethodData::GiftCard(_)
                    | domain::PaymentMethodData::OpenBanking(_)
                    | domain::PaymentMethodData::CardToken(_)
                    | domain::PaymentMethodData::NetworkToken(_)
                    | domain::PaymentMethodData::CardDetailsForNetworkTransactionId(_) => {
                        Err(errors::ConnectorError::NotImplemented(
                            utils::get_unimplemented_payment_method_error_message(
                                "Bank of America",
                            ),
                        )
                        .into())
                    }
                }
            }
        }
    }
}

impl
    TryFrom<(
        &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
        String,
    )> for BankOfAmericaPaymentsRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (item, connector_mandate_id): (
            &BankOfAmericaRouterData<&types::PaymentsAuthorizeRouterData>,
            String,
        ),
    ) -> Result<Self, Self::Error> {
        let processing_information = ProcessingInformation::try_from((item, None, None))?;
        let payment_instrument = BankOfAmericaPaymentInstrument {
            id: connector_mandate_id.into(),
        };
        let bill_to =
            item.router_data.request.get_email().ok().and_then(|email| {
                build_bill_to(item.router_data.get_optional_billing(), email).ok()
            });
        let order_information = OrderInformationWithBill::from((item, bill_to));
        let payment_information =
            PaymentInformation::MandatePayment(Box::new(MandatePaymentInformation {
                payment_instrument,
            }));
        let client_reference_information = ClientReferenceInformation::from(item);
        let merchant_defined_information = item
            .router_data
            .request
            .metadata
            .clone()
            .map(Vec::<MerchantDefinedInformation>::foreign_from);
        Ok(Self {
            processing_information,
            payment_information,
            order_information,
            client_reference_information,
            merchant_defined_information,
            consumer_authentication_information: None,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BankofamericaPaymentStatus {
    Authorized,
    Succeeded,
    Failed,
    Voided,
    Reversed,
    Pending,
    Declined,
    Rejected,
    Challenge,
    AuthorizedPendingReview,
    AuthorizedRiskDeclined,
    Transmitted,
    InvalidRequest,
    ServerError,
    PendingAuthentication,
    PendingReview,
    Accepted,
    Cancelled,
    //PartialAuthorized, not being consumed yet.
}

impl ForeignFrom<(BankofamericaPaymentStatus, bool)> for enums::AttemptStatus {
    fn foreign_from((status, auto_capture): (BankofamericaPaymentStatus, bool)) -> Self {
        match status {
            BankofamericaPaymentStatus::Authorized
            | BankofamericaPaymentStatus::AuthorizedPendingReview => {
                if auto_capture {
                    // Because BankOfAmerica will return Payment Status as Authorized even in AutoCapture Payment
                    Self::Charged
                } else {
                    Self::Authorized
                }
            }
            BankofamericaPaymentStatus::Pending => {
                if auto_capture {
                    Self::Charged
                } else {
                    Self::Pending
                }
            }
            BankofamericaPaymentStatus::Succeeded | BankofamericaPaymentStatus::Transmitted => {
                Self::Charged
            }
            BankofamericaPaymentStatus::Voided
            | BankofamericaPaymentStatus::Reversed
            | BankofamericaPaymentStatus::Cancelled => Self::Voided,
            BankofamericaPaymentStatus::Failed
            | BankofamericaPaymentStatus::Declined
            | BankofamericaPaymentStatus::AuthorizedRiskDeclined
            | BankofamericaPaymentStatus::InvalidRequest
            | BankofamericaPaymentStatus::Rejected
            | BankofamericaPaymentStatus::ServerError => Self::Failure,
            BankofamericaPaymentStatus::PendingAuthentication => Self::AuthenticationPending,
            BankofamericaPaymentStatus::PendingReview
            | BankofamericaPaymentStatus::Challenge
            | BankofamericaPaymentStatus::Accepted => Self::Pending,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BankOfAmericaPaymentsResponse {
    ClientReferenceInformation(Box<BankOfAmericaClientReferenceResponse>),
    ErrorInformation(Box<BankOfAmericaErrorInformationResponse>),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BankOfAmericaSetupMandatesResponse {
    ClientReferenceInformation(Box<BankOfAmericaClientReferenceResponse>),
    ErrorInformation(Box<BankOfAmericaErrorInformationResponse>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaClientReferenceResponse {
    id: String,
    status: BankofamericaPaymentStatus,
    client_reference_information: ClientReferenceInformation,
    processor_information: Option<ClientProcessorInformation>,
    processing_information: Option<ProcessingInformationResponse>,
    payment_information: Option<PaymentInformationResponse>,
    payment_insights_information: Option<PaymentInsightsInformation>,
    risk_information: Option<ClientRiskInformation>,
    token_information: Option<BankOfAmericaTokenInformation>,
    error_information: Option<BankOfAmericaErrorInformation>,
    issuer_information: Option<IssuerInformation>,
    sender_information: Option<SenderInformation>,
    payment_account_information: Option<PaymentAccountInformation>,
    reconciliation_id: Option<String>,
    consumer_authentication_information: Option<ConsumerAuthenticationInformation>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerAuthenticationInformation {
    eci_raw: Option<String>,
    eci: Option<String>,
    acs_transaction_id: Option<String>,
    cavv: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SenderInformation {
    payment_information: Option<PaymentInformationResponse>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentInsightsInformation {
    response_insights: Option<ResponseInsights>,
    rule_results: Option<RuleResults>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseInsights {
    category_code: Option<String>,
    category: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleResults {
    id: Option<String>,
    decision: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentInformationResponse {
    tokenized_card: Option<CardResponseObject>,
    customer: Option<CustomerResponseObject>,
    card: Option<CardResponseObject>,
    scheme: Option<String>,
    bin: Option<String>,
    account_type: Option<String>,
    issuer: Option<String>,
    bin_country: Option<api_enums::CountryAlpha2>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerResponseObject {
    customer_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentAccountInformation {
    card: Option<PaymentAccountCardInformation>,
    features: Option<PaymentAccountFeatureInformation>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentAccountFeatureInformation {
    health_card: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentAccountCardInformation {
    #[serde(rename = "type")]
    card_type: Option<String>,
    hashed_number: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingInformationResponse {
    payment_solution: Option<String>,
    commerce_indicator: Option<String>,
    commerce_indicator_label: Option<String>,
    authorization_options: Option<AuthorizationOptions>,
    ecommerce_indicator: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationOptions {
    auth_type: Option<String>,
    initiator: Option<Initiator>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Initiator {
    merchant_initiated_transaction: Option<MerchantInitiatedTransactionResponse>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantInitiatedTransactionResponse {
    agreement_id: Option<String>,
    previous_transaction_id: Option<String>,
    original_authorized_amount: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaTokenInformation {
    payment_instrument: Option<BankOfAmericaPaymentInstrument>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuerInformation {
    country: Option<api_enums::CountryAlpha2>,
    discretionary_data: Option<String>,
    country_specific_discretionary_data: Option<String>,
    response_code: Option<String>,
    pin_request_indicator: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardResponseObject {
    suffix: Option<String>,
    prefix: Option<String>,
    expiration_month: Option<Secret<String>>,
    expiration_year: Option<Secret<String>>,
    #[serde(rename = "type")]
    card_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaErrorInformationResponse {
    id: String,
    error_information: BankOfAmericaErrorInformation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BankOfAmericaErrorInformation {
    reason: Option<String>,
    message: Option<String>,
    details: Option<Vec<Details>>,
}

impl<F, T>
    ForeignFrom<(
        &BankOfAmericaErrorInformationResponse,
        types::ResponseRouterData<F, BankOfAmericaPaymentsResponse, T, types::PaymentsResponseData>,
        Option<enums::AttemptStatus>,
    )> for types::RouterData<F, T, types::PaymentsResponseData>
{
    fn foreign_from(
        (error_response, item, transaction_status): (
            &BankOfAmericaErrorInformationResponse,
            types::ResponseRouterData<
                F,
                BankOfAmericaPaymentsResponse,
                T,
                types::PaymentsResponseData,
            >,
            Option<enums::AttemptStatus>,
        ),
    ) -> Self {
        let detailed_error_info =
            error_response
                .error_information
                .details
                .as_ref()
                .map(|details| {
                    details
                        .iter()
                        .map(|details| format!("{} : {}", details.field, details.reason))
                        .collect::<Vec<_>>()
                        .join(", ")
                });

        let reason = get_error_reason(
            error_response.error_information.message.clone(),
            detailed_error_info,
            None,
        );
        let response = Err(types::ErrorResponse {
            code: error_response
                .error_information
                .reason
                .clone()
                .unwrap_or(consts::NO_ERROR_CODE.to_string()),
            message: error_response
                .error_information
                .reason
                .clone()
                .unwrap_or(consts::NO_ERROR_MESSAGE.to_string()),
            reason,
            status_code: item.http_code,
            attempt_status: None,
            connector_transaction_id: Some(error_response.id.clone()),
        });

        match transaction_status {
            Some(status) => Self {
                response,
                status,
                ..item.data
            },
            None => Self {
                response,
                ..item.data
            },
        }
    }
}

fn get_error_response_if_failure(
    (info_response, status, http_code): (
        &BankOfAmericaClientReferenceResponse,
        enums::AttemptStatus,
        u16,
    ),
) -> Option<types::ErrorResponse> {
    if utils::is_payment_failure(status) {
        Some(types::ErrorResponse::foreign_from((
            &info_response.error_information,
            &info_response.risk_information,
            Some(status),
            http_code,
            info_response.id.clone(),
        )))
    } else {
        None
    }
}

fn get_payment_response(
    (info_response, status, http_code): (
        &BankOfAmericaClientReferenceResponse,
        enums::AttemptStatus,
        u16,
    ),
) -> Result<types::PaymentsResponseData, types::ErrorResponse> {
    let error_response = get_error_response_if_failure((info_response, status, http_code));
    match error_response {
        Some(error) => Err(error),
        None => {
            let mandate_reference =
                info_response
                    .token_information
                    .clone()
                    .map(|token_info| types::MandateReference {
                        connector_mandate_id: token_info
                            .payment_instrument
                            .map(|payment_instrument| payment_instrument.id.expose()),
                        payment_method_id: None,
                        mandate_metadata: None,
                        connector_mandate_request_reference_id: None,
                    });

            Ok(types::PaymentsResponseData::TransactionResponse {
                resource_id: types::ResponseId::ConnectorTransactionId(info_response.id.clone()),
                redirection_data: Box::new(None),
                mandate_reference: Box::new(mandate_reference),
                connector_metadata: None,
                network_txn_id: None,
                connector_response_reference_id: Some(
                    info_response
                        .client_reference_information
                        .code
                        .clone()
                        .unwrap_or(info_response.id.clone()),
                ),
                incremental_authorization_allowed: None,
                charge_id: None,
            })
        }
    }
}

impl<F>
    TryFrom<
        types::ResponseRouterData<
            F,
            BankOfAmericaPaymentsResponse,
            types::PaymentsAuthorizeData,
            types::PaymentsResponseData,
        >,
    > for types::RouterData<F, types::PaymentsAuthorizeData, types::PaymentsResponseData>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: types::ResponseRouterData<
            F,
            BankOfAmericaPaymentsResponse,
            types::PaymentsAuthorizeData,
            types::PaymentsResponseData,
        >,
    ) -> Result<Self, Self::Error> {
        match item.response {
            BankOfAmericaPaymentsResponse::ClientReferenceInformation(info_response) => {
                let status = enums::AttemptStatus::foreign_from((
                    info_response.status.clone(),
                    item.data.request.is_auto_capture()?,
                ));
                let response = get_payment_response((&info_response, status, item.http_code));
                let connector_response = match item.data.payment_method {
                    common_enums::PaymentMethod::Card => info_response
                        .processor_information
                        .as_ref()
                        .and_then(|processor_information| {
                            info_response
                                .consumer_authentication_information
                                .as_ref()
                                .map(|consumer_auth_information| {
                                    types::AdditionalPaymentMethodConnectorResponse::foreign_from((
                                        processor_information,
                                        consumer_auth_information,
                                    ))
                                })
                        })
                        .map(types::ConnectorResponseData::with_additional_payment_method_data),
                    common_enums::PaymentMethod::CardRedirect
                    | common_enums::PaymentMethod::PayLater
                    | common_enums::PaymentMethod::Wallet
                    | common_enums::PaymentMethod::BankRedirect
                    | common_enums::PaymentMethod::BankTransfer
                    | common_enums::PaymentMethod::Crypto
                    | common_enums::PaymentMethod::BankDebit
                    | common_enums::PaymentMethod::Reward
                    | common_enums::PaymentMethod::RealTimePayment
                    | common_enums::PaymentMethod::MobilePayment
                    | common_enums::PaymentMethod::Upi
                    | common_enums::PaymentMethod::Voucher
                    | common_enums::PaymentMethod::OpenBanking
                    | common_enums::PaymentMethod::GiftCard => None,
                };

                Ok(Self {
                    status,
                    response,
                    connector_response,
                    ..item.data
                })
            }
            BankOfAmericaPaymentsResponse::ErrorInformation(ref error_response) => {
                Ok(Self::foreign_from((
                    &*error_response.clone(),
                    item,
                    Some(enums::AttemptStatus::Failure),
                )))
            }
        }
    }
}

impl
    ForeignFrom<(
        &ClientProcessorInformation,
        &ConsumerAuthenticationInformation,
    )> for types::AdditionalPaymentMethodConnectorResponse
{
    fn foreign_from(
        item: (
            &ClientProcessorInformation,
            &ConsumerAuthenticationInformation,
        ),
    ) -> Self {
        let processor_information = item.0;
        let consumer_authentication_information = item.1;
        let payment_checks = Some(serde_json::json!({
        "avs_response": processor_information.avs,
        "card_verification": processor_information.card_verification,
        "approval_code": processor_information.approval_code,
        "consumer_authentication_response": processor_information.consumer_authentication_response,
        "cavv": consumer_authentication_information.cavv,
        "eci": consumer_authentication_information.eci,
        "eci_raw": consumer_authentication_information.eci_raw,
        }));

        let authentication_data = Some(serde_json::json!({
        "retrieval_reference_number": processor_information.retrieval_reference_number,
        "acs_transaction_id": consumer_authentication_information.acs_transaction_id,
        "system_trace_audit_number": processor_information.system_trace_audit_number,
        }));

        Self::Card {
            authentication_data,
            payment_checks,
        }
    }
}

impl<F>
    TryFrom<
        types::ResponseRouterData<
            F,
            BankOfAmericaPaymentsResponse,
            types::PaymentsCaptureData,
            types::PaymentsResponseData,
        >,
    > for types::RouterData<F, types::PaymentsCaptureData, types::PaymentsResponseData>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: types::ResponseRouterData<
            F,
            BankOfAmericaPaymentsResponse,
            types::PaymentsCaptureData,
            types::PaymentsResponseData,
        >,
    ) -> Result<Self, Self::Error> {
        match item.response {
            BankOfAmericaPaymentsResponse::ClientReferenceInformation(info_response) => {
                let status =
                    enums::AttemptStatus::foreign_from((info_response.status.clone(), true));
                let response = get_payment_response((&info_response, status, item.http_code));
                Ok(Self {
                    status,
                    response,
                    ..item.data
                })
            }
            BankOfAmericaPaymentsResponse::ErrorInformation(ref error_response) => {
                Ok(Self::foreign_from((&*error_response.clone(), item, None)))
            }
        }
    }
}

impl<F>
    TryFrom<
        types::ResponseRouterData<
            F,
            BankOfAmericaPaymentsResponse,
            types::PaymentsCancelData,
            types::PaymentsResponseData,
        >,
    > for types::RouterData<F, types::PaymentsCancelData, types::PaymentsResponseData>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: types::ResponseRouterData<
            F,
            BankOfAmericaPaymentsResponse,
            types::PaymentsCancelData,
            types::PaymentsResponseData,
        >,
    ) -> Result<Self, Self::Error> {
        match item.response {
            BankOfAmericaPaymentsResponse::ClientReferenceInformation(info_response) => {
                let status =
                    enums::AttemptStatus::foreign_from((info_response.status.clone(), false));
                let response = get_payment_response((&info_response, status, item.http_code));
                Ok(Self {
                    status,
                    response,
                    ..item.data
                })
            }
            BankOfAmericaPaymentsResponse::ErrorInformation(ref error_response) => {
                Ok(Self::foreign_from((&*error_response.clone(), item, None)))
            }
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaTransactionResponse {
    id: String,
    application_information: ApplicationInformation,
    client_reference_information: Option<ClientReferenceInformation>,
    processor_information: Option<ClientProcessorInformation>,
    processing_information: Option<ProcessingInformationResponse>,
    payment_information: Option<PaymentInformationResponse>,
    payment_insights_information: Option<PaymentInsightsInformation>,
    error_information: Option<BankOfAmericaErrorInformation>,
    fraud_marking_information: Option<FraudMarkingInformation>,
    risk_information: Option<ClientRiskInformation>,
    token_information: Option<BankOfAmericaTokenInformation>,
    reconciliation_id: Option<String>,
    consumer_authentication_information: Option<ConsumerAuthenticationInformation>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FraudMarkingInformation {
    reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationInformation {
    status: Option<BankofamericaPaymentStatus>,
}

impl<F>
    TryFrom<
        types::ResponseRouterData<
            F,
            BankOfAmericaTransactionResponse,
            types::PaymentsSyncData,
            types::PaymentsResponseData,
        >,
    > for types::RouterData<F, types::PaymentsSyncData, types::PaymentsResponseData>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: types::ResponseRouterData<
            F,
            BankOfAmericaTransactionResponse,
            types::PaymentsSyncData,
            types::PaymentsResponseData,
        >,
    ) -> Result<Self, Self::Error> {
        match item.response.application_information.status {
            Some(app_status) => {
                let status = enums::AttemptStatus::foreign_from((
                    app_status,
                    item.data.request.is_auto_capture()?,
                ));

                let connector_response = match item.data.payment_method {
                    common_enums::PaymentMethod::Card => item
                        .response
                        .processor_information
                        .as_ref()
                        .and_then(|processor_information| {
                            item.response
                                .consumer_authentication_information
                                .as_ref()
                                .map(|consumer_auth_information| {
                                    types::AdditionalPaymentMethodConnectorResponse::foreign_from((
                                        processor_information,
                                        consumer_auth_information,
                                    ))
                                })
                        })
                        .map(types::ConnectorResponseData::with_additional_payment_method_data),
                    common_enums::PaymentMethod::CardRedirect
                    | common_enums::PaymentMethod::PayLater
                    | common_enums::PaymentMethod::Wallet
                    | common_enums::PaymentMethod::BankRedirect
                    | common_enums::PaymentMethod::BankTransfer
                    | common_enums::PaymentMethod::Crypto
                    | common_enums::PaymentMethod::BankDebit
                    | common_enums::PaymentMethod::Reward
                    | common_enums::PaymentMethod::RealTimePayment
                    | common_enums::PaymentMethod::MobilePayment
                    | common_enums::PaymentMethod::Upi
                    | common_enums::PaymentMethod::Voucher
                    | common_enums::PaymentMethod::OpenBanking
                    | common_enums::PaymentMethod::GiftCard => None,
                };

                let risk_info: Option<ClientRiskInformation> = None;
                if utils::is_payment_failure(status) {
                    Ok(Self {
                        response: Err(types::ErrorResponse::foreign_from((
                            &item.response.error_information,
                            &risk_info,
                            Some(status),
                            item.http_code,
                            item.response.id.clone(),
                        ))),
                        status: enums::AttemptStatus::Failure,
                        connector_response,
                        ..item.data
                    })
                } else {
                    Ok(Self {
                        status,
                        response: Ok(types::PaymentsResponseData::TransactionResponse {
                            resource_id: types::ResponseId::ConnectorTransactionId(
                                item.response.id.clone(),
                            ),
                            redirection_data: Box::new(None),
                            mandate_reference: Box::new(None),
                            connector_metadata: None,
                            network_txn_id: None,
                            connector_response_reference_id: item
                                .response
                                .client_reference_information
                                .map(|cref| cref.code)
                                .unwrap_or(Some(item.response.id)),
                            incremental_authorization_allowed: None,
                            charge_id: None,
                        }),
                        connector_response,
                        ..item.data
                    })
                }
            }
            None => Ok(Self {
                status: item.data.status,
                response: Ok(types::PaymentsResponseData::TransactionResponse {
                    resource_id: types::ResponseId::ConnectorTransactionId(
                        item.response.id.clone(),
                    ),
                    redirection_data: Box::new(None),
                    mandate_reference: Box::new(None),
                    connector_metadata: None,
                    network_txn_id: None,
                    connector_response_reference_id: Some(item.response.id),
                    incremental_authorization_allowed: None,
                    charge_id: None,
                }),
                ..item.data
            }),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderInformation {
    amount_details: Amount,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaCaptureRequest {
    order_information: OrderInformation,
    client_reference_information: ClientReferenceInformation,
    #[serde(skip_serializing_if = "Option::is_none")]
    merchant_defined_information: Option<Vec<MerchantDefinedInformation>>,
}

impl TryFrom<&BankOfAmericaRouterData<&types::PaymentsCaptureRouterData>>
    for BankOfAmericaCaptureRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        value: &BankOfAmericaRouterData<&types::PaymentsCaptureRouterData>,
    ) -> Result<Self, Self::Error> {
        let merchant_defined_information = value
            .router_data
            .request
            .metadata
            .clone()
            .map(Vec::<MerchantDefinedInformation>::foreign_from);
        Ok(Self {
            order_information: OrderInformation {
                amount_details: Amount {
                    total_amount: value.amount.to_owned(),
                    currency: value.router_data.request.currency,
                },
            },
            client_reference_information: ClientReferenceInformation {
                code: Some(value.router_data.connector_request_reference_id.clone()),
            },
            merchant_defined_information,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaVoidRequest {
    client_reference_information: ClientReferenceInformation,
    reversal_information: ReversalInformation,
    #[serde(skip_serializing_if = "Option::is_none")]
    merchant_defined_information: Option<Vec<MerchantDefinedInformation>>,
    // The connector documentation does not mention the merchantDefinedInformation field for Void requests. But this has been still added because it works!
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReversalInformation {
    amount_details: Amount,
    reason: String,
}

impl TryFrom<&BankOfAmericaRouterData<&types::PaymentsCancelRouterData>>
    for BankOfAmericaVoidRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        value: &BankOfAmericaRouterData<&types::PaymentsCancelRouterData>,
    ) -> Result<Self, Self::Error> {
        let merchant_defined_information = value
            .router_data
            .request
            .metadata
            .clone()
            .map(Vec::<MerchantDefinedInformation>::foreign_from);
        Ok(Self {
            client_reference_information: ClientReferenceInformation {
                code: Some(value.router_data.connector_request_reference_id.clone()),
            },
            reversal_information: ReversalInformation {
                amount_details: Amount {
                    total_amount: value.amount.to_owned(),
                    currency: value.router_data.request.currency.ok_or(
                        errors::ConnectorError::MissingRequiredField {
                            field_name: "Currency",
                        },
                    )?,
                },
                reason: value
                    .router_data
                    .request
                    .cancellation_reason
                    .clone()
                    .ok_or(errors::ConnectorError::MissingRequiredField {
                        field_name: "Cancellation Reason",
                    })?,
            },
            merchant_defined_information,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaRefundRequest {
    order_information: OrderInformation,
    client_reference_information: ClientReferenceInformation,
}

impl<F> TryFrom<&BankOfAmericaRouterData<&types::RefundsRouterData<F>>>
    for BankOfAmericaRefundRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: &BankOfAmericaRouterData<&types::RefundsRouterData<F>>,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            order_information: OrderInformation {
                amount_details: Amount {
                    total_amount: item.amount.clone(),
                    currency: item.router_data.request.currency,
                },
            },
            client_reference_information: ClientReferenceInformation {
                code: Some(item.router_data.request.refund_id.clone()),
            },
        })
    }
}

impl From<BankOfAmericaRefundResponse> for enums::RefundStatus {
    fn from(item: BankOfAmericaRefundResponse) -> Self {
        let error_reason = item
            .error_information
            .and_then(|error_info| error_info.reason);
        match item.status {
            BankofamericaRefundStatus::Succeeded | BankofamericaRefundStatus::Transmitted => {
                Self::Success
            }
            BankofamericaRefundStatus::Cancelled
            | BankofamericaRefundStatus::Failed
            | BankofamericaRefundStatus::Voided => Self::Failure,
            BankofamericaRefundStatus::Pending => Self::Pending,
            BankofamericaRefundStatus::TwoZeroOne => {
                if error_reason == Some("PROCESSOR_DECLINED".to_string()) {
                    Self::Failure
                } else {
                    Self::Pending
                }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaRefundResponse {
    id: String,
    status: BankofamericaRefundStatus,
    error_information: Option<BankOfAmericaErrorInformation>,
}

impl TryFrom<types::RefundsResponseRouterData<api::Execute, BankOfAmericaRefundResponse>>
    for types::RefundsRouterData<api::Execute>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: types::RefundsResponseRouterData<api::Execute, BankOfAmericaRefundResponse>,
    ) -> Result<Self, Self::Error> {
        let refund_status = enums::RefundStatus::from(item.response.clone());
        let response = if utils::is_refund_failure(refund_status) {
            Err(types::ErrorResponse::foreign_from((
                &item.response.error_information,
                &None,
                None,
                item.http_code,
                item.response.id,
            )))
        } else {
            Ok(types::RefundsResponseData {
                connector_refund_id: item.response.id,
                refund_status,
            })
        };

        Ok(Self {
            response,
            ..item.data
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BankofamericaRefundStatus {
    Succeeded,
    Transmitted,
    Failed,
    Pending,
    Voided,
    Cancelled,
    #[serde(rename = "201")]
    TwoZeroOne,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RsyncApplicationInformation {
    status: Option<BankofamericaRefundStatus>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaRsyncResponse {
    id: String,
    application_information: Option<RsyncApplicationInformation>,
    error_information: Option<BankOfAmericaErrorInformation>,
}

impl TryFrom<types::RefundsResponseRouterData<api::RSync, BankOfAmericaRsyncResponse>>
    for types::RefundsRouterData<api::RSync>
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        item: types::RefundsResponseRouterData<api::RSync, BankOfAmericaRsyncResponse>,
    ) -> Result<Self, Self::Error> {
        let response = match item
            .response
            .application_information
            .and_then(|application_information| application_information.status)
        {
            Some(status) => {
                let error_reason = item
                    .response
                    .error_information
                    .clone()
                    .and_then(|error_info| error_info.reason);
                let refund_status = match status {
                    BankofamericaRefundStatus::Succeeded
                    | BankofamericaRefundStatus::Transmitted => enums::RefundStatus::Success,
                    BankofamericaRefundStatus::Cancelled
                    | BankofamericaRefundStatus::Failed
                    | BankofamericaRefundStatus::Voided => enums::RefundStatus::Failure,
                    BankofamericaRefundStatus::Pending => enums::RefundStatus::Pending,
                    BankofamericaRefundStatus::TwoZeroOne => {
                        if error_reason == Some("PROCESSOR_DECLINED".to_string()) {
                            enums::RefundStatus::Failure
                        } else {
                            enums::RefundStatus::Pending
                        }
                    }
                };
                if utils::is_refund_failure(refund_status) {
                    if status == BankofamericaRefundStatus::Voided {
                        Err(types::ErrorResponse::foreign_from((
                            &Some(BankOfAmericaErrorInformation {
                                message: Some(consts::REFUND_VOIDED.to_string()),
                                reason: Some(consts::REFUND_VOIDED.to_string()),
                                details: None,
                            }),
                            &None,
                            None,
                            item.http_code,
                            item.response.id.clone(),
                        )))
                    } else {
                        Err(types::ErrorResponse::foreign_from((
                            &item.response.error_information,
                            &None,
                            None,
                            item.http_code,
                            item.response.id.clone(),
                        )))
                    }
                } else {
                    Ok(types::RefundsResponseData {
                        connector_refund_id: item.response.id,
                        refund_status,
                    })
                }
            }

            None => Ok(types::RefundsResponseData {
                connector_refund_id: item.response.id.clone(),
                refund_status: match item.data.response {
                    Ok(response) => response.refund_status,
                    Err(_) => common_enums::RefundStatus::Pending,
                },
            }),
        };

        Ok(Self {
            response,
            ..item.data
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaStandardErrorResponse {
    pub error_information: Option<ErrorInformation>,
    pub status: Option<String>,
    pub message: Option<String>,
    pub reason: Option<String>,
    pub details: Option<Vec<Details>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankOfAmericaServerErrorResponse {
    pub status: Option<String>,
    pub message: Option<String>,
    pub reason: Option<Reason>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Reason {
    SystemError,
    ServerTimeout,
    ServiceTimeout,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BankOfAmericaAuthenticationErrorResponse {
    pub response: AuthenticationErrorInformation,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum BankOfAmericaErrorResponse {
    AuthenticationError(BankOfAmericaAuthenticationErrorResponse),
    StandardError(BankOfAmericaStandardErrorResponse),
}

#[derive(Debug, Deserialize, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub field: String,
    pub reason: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ErrorInformation {
    pub message: String,
    pub reason: String,
    pub details: Option<Vec<Details>>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct AuthenticationErrorInformation {
    pub rmsg: String,
}

impl
    ForeignFrom<(
        &Option<BankOfAmericaErrorInformation>,
        &Option<ClientRiskInformation>,
        Option<enums::AttemptStatus>,
        u16,
        String,
    )> for types::ErrorResponse
{
    fn foreign_from(
        (error_data, risk_information, attempt_status, status_code, transaction_id): (
            &Option<BankOfAmericaErrorInformation>,
            &Option<ClientRiskInformation>,
            Option<enums::AttemptStatus>,
            u16,
            String,
        ),
    ) -> Self {
        let avs_message = risk_information
            .clone()
            .map(|client_risk_information| {
                client_risk_information.rules.map(|rules| {
                    rules
                        .iter()
                        .map(|risk_info| {
                            risk_info.name.clone().map_or("".to_string(), |name| {
                                format!(" , {}", name.clone().expose())
                            })
                        })
                        .collect::<Vec<String>>()
                        .join("")
                })
            })
            .unwrap_or(Some("".to_string()));

        let detailed_error_info = error_data.to_owned().and_then(|error_info| {
            error_info.details.map(|error_details| {
                error_details
                    .iter()
                    .map(|details| format!("{} : {}", details.field, details.reason))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
        });

        let reason = get_error_reason(
            error_data
                .clone()
                .and_then(|error_details| error_details.message),
            detailed_error_info,
            avs_message,
        );
        let error_message = error_data
            .clone()
            .and_then(|error_details| error_details.reason);

        Self {
            code: error_message
                .clone()
                .unwrap_or(consts::NO_ERROR_CODE.to_string()),
            message: error_message
                .clone()
                .unwrap_or(consts::NO_ERROR_MESSAGE.to_string()),
            reason,
            status_code,
            attempt_status,
            connector_transaction_id: Some(transaction_id.clone()),
        }
    }
}

impl TryFrom<(&types::SetupMandateRouterData, domain::Card)> for BankOfAmericaPaymentsRequest {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (item, ccard): (&types::SetupMandateRouterData, domain::Card),
    ) -> Result<Self, Self::Error> {
        let order_information = OrderInformationWithBill::try_from(item)?;
        let client_reference_information = ClientReferenceInformation::from(item);
        let merchant_defined_information = item.request.metadata.clone().map(|metadata| {
            Vec::<MerchantDefinedInformation>::foreign_from(metadata.peek().to_owned())
        });
        let payment_information = PaymentInformation::try_from(&ccard)?;
        let processing_information = ProcessingInformation::try_from((None, None))?;
        Ok(Self {
            processing_information,
            payment_information,
            order_information,
            client_reference_information,
            consumer_authentication_information: None,
            merchant_defined_information,
        })
    }
}

impl TryFrom<(&types::SetupMandateRouterData, domain::ApplePayWalletData)>
    for BankOfAmericaPaymentsRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (item, apple_pay_data): (&types::SetupMandateRouterData, domain::ApplePayWalletData),
    ) -> Result<Self, Self::Error> {
        let order_information = OrderInformationWithBill::try_from(item)?;
        let client_reference_information = ClientReferenceInformation::from(item);
        let merchant_defined_information = item.request.metadata.clone().map(|metadata| {
            Vec::<MerchantDefinedInformation>::foreign_from(metadata.peek().to_owned())
        });
        let payment_information = match item.payment_method_token.clone() {
            Some(payment_method_token) => match payment_method_token {
                types::PaymentMethodToken::ApplePayDecrypt(decrypt_data) => {
                    PaymentInformation::try_from(&decrypt_data)?
                }
                types::PaymentMethodToken::Token(_) => Err(unimplemented_payment_method!(
                    "Apple Pay",
                    "Manual",
                    "Bank Of America"
                ))?,
                types::PaymentMethodToken::PazeDecrypt(_) => {
                    Err(unimplemented_payment_method!("Paze", "Bank Of America"))?
                }
            },
            None => PaymentInformation::from(&apple_pay_data),
        };
        let processing_information = ProcessingInformation::try_from((
            Some(PaymentSolution::ApplePay),
            Some(apple_pay_data.payment_method.network.clone()),
        ))?;
        let ucaf_collection_indicator = match apple_pay_data
            .payment_method
            .network
            .to_lowercase()
            .as_str()
        {
            "mastercard" => Some("2".to_string()),
            _ => None,
        };
        let consumer_authentication_information = Some(BankOfAmericaConsumerAuthInformation {
            ucaf_collection_indicator,
            cavv: None,
            ucaf_authentication_data: None,
            xid: None,
            directory_server_transaction_id: None,
            specification_version: None,
        });

        Ok(Self {
            processing_information,
            payment_information,
            order_information,
            client_reference_information,
            merchant_defined_information,
            consumer_authentication_information,
        })
    }
}

impl TryFrom<(&types::SetupMandateRouterData, domain::GooglePayWalletData)>
    for BankOfAmericaPaymentsRequest
{
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (item, google_pay_data): (&types::SetupMandateRouterData, domain::GooglePayWalletData),
    ) -> Result<Self, Self::Error> {
        let order_information = OrderInformationWithBill::try_from(item)?;
        let client_reference_information = ClientReferenceInformation::from(item);
        let merchant_defined_information = item.request.metadata.clone().map(|metadata| {
            Vec::<MerchantDefinedInformation>::foreign_from(metadata.peek().to_owned())
        });
        let payment_information = PaymentInformation::from(&google_pay_data);
        let processing_information =
            ProcessingInformation::try_from((Some(PaymentSolution::GooglePay), None))?;

        Ok(Self {
            processing_information,
            payment_information,
            order_information,
            client_reference_information,
            merchant_defined_information,
            consumer_authentication_information: None,
        })
    }
}

// specific for setupMandate flow
impl TryFrom<(Option<PaymentSolution>, Option<String>)> for ProcessingInformation {
    type Error = error_stack::Report<errors::ConnectorError>;
    fn try_from(
        (solution, network): (Option<PaymentSolution>, Option<String>),
    ) -> Result<Self, Self::Error> {
        let (action_list, action_token_types, authorization_options) =
            get_boa_mandate_action_details();
        let commerce_indicator = get_commerce_indicator(network);

        Ok(Self {
            capture: Some(false),
            capture_options: None,
            action_list,
            action_token_types,
            authorization_options,
            commerce_indicator,
            payment_solution: solution.map(String::from),
        })
    }
}

impl TryFrom<&types::SetupMandateRouterData> for OrderInformationWithBill {
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(item: &types::SetupMandateRouterData) -> Result<Self, Self::Error> {
        let email = item.request.get_email()?;
        let bill_to = build_bill_to(item.get_optional_billing(), email)?;

        Ok(Self {
            amount_details: Amount {
                total_amount: "0".to_string(),
                currency: item.request.currency,
            },
            bill_to: Some(bill_to),
        })
    }
}

impl TryFrom<&domain::Card> for PaymentInformation {
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(ccard: &domain::Card) -> Result<Self, Self::Error> {
        let card_type = match ccard.card_network.clone().and_then(get_boa_card_type) {
            Some(card_network) => Some(card_network.to_string()),
            None => ccard.get_card_issuer().ok().map(String::from),
        };
        Ok(Self::Cards(Box::new(CardPaymentInformation {
            card: Card {
                number: ccard.card_number.clone(),
                expiration_month: ccard.card_exp_month.clone(),
                expiration_year: ccard.card_exp_year.clone(),
                security_code: ccard.card_cvc.clone(),
                card_type,
            },
        })))
    }
}

impl TryFrom<&Box<ApplePayPredecryptData>> for PaymentInformation {
    type Error = error_stack::Report<errors::ConnectorError>;

    fn try_from(apple_pay_data: &Box<ApplePayPredecryptData>) -> Result<Self, Self::Error> {
        let expiration_month = apple_pay_data.get_expiry_month()?;
        let expiration_year = apple_pay_data.get_four_digit_expiry_year()?;

        Ok(Self::ApplePay(Box::new(ApplePayPaymentInformation {
            tokenized_card: TokenizedCard {
                number: apple_pay_data.application_primary_account_number.clone(),
                cryptogram: apple_pay_data
                    .payment_data
                    .online_payment_cryptogram
                    .clone(),
                transaction_type: TransactionType::ApplePay,
                expiration_year,
                expiration_month,
            },
        })))
    }
}

impl From<&domain::ApplePayWalletData> for PaymentInformation {
    fn from(apple_pay_data: &domain::ApplePayWalletData) -> Self {
        Self::ApplePayToken(Box::new(ApplePayTokenPaymentInformation {
            fluid_data: FluidData {
                value: Secret::from(apple_pay_data.payment_data.clone()),
            },
            tokenized_card: ApplePayTokenizedCard {
                transaction_type: TransactionType::ApplePay,
            },
        }))
    }
}

impl From<&domain::GooglePayWalletData> for PaymentInformation {
    fn from(google_pay_data: &domain::GooglePayWalletData) -> Self {
        Self::GooglePay(Box::new(GooglePayPaymentInformation {
            fluid_data: FluidData {
                value: Secret::from(
                    consts::BASE64_ENGINE.encode(google_pay_data.tokenization_data.token.clone()),
                ),
            },
        }))
    }
}

impl ForeignFrom<(&BankOfAmericaErrorInformationResponse, u16)> for types::ErrorResponse {
    fn foreign_from(
        (error_response, status_code): (&BankOfAmericaErrorInformationResponse, u16),
    ) -> Self {
        let detailed_error_info =
            error_response
                .error_information
                .to_owned()
                .details
                .map(|error_details| {
                    error_details
                        .iter()
                        .map(|details| format!("{} : {}", details.field, details.reason))
                        .collect::<Vec<_>>()
                        .join(", ")
                });

        let reason = get_error_reason(
            error_response.error_information.message.to_owned(),
            detailed_error_info,
            None,
        );
        Self {
            code: error_response
                .error_information
                .reason
                .clone()
                .unwrap_or(consts::NO_ERROR_CODE.to_string()),
            message: error_response
                .error_information
                .reason
                .clone()
                .unwrap_or(consts::NO_ERROR_MESSAGE.to_string()),
            reason,
            status_code,
            attempt_status: None,
            connector_transaction_id: Some(error_response.id.clone()),
        }
    }
}

fn get_boa_mandate_action_details() -> (
    Option<Vec<BankOfAmericaActionsList>>,
    Option<Vec<BankOfAmericaActionsTokenType>>,
    Option<BankOfAmericaAuthorizationOptions>,
) {
    (
        Some(vec![BankOfAmericaActionsList::TokenCreate]),
        Some(vec![
            BankOfAmericaActionsTokenType::PaymentInstrument,
            BankOfAmericaActionsTokenType::Customer,
        ]),
        Some(BankOfAmericaAuthorizationOptions {
            initiator: Some(BankOfAmericaPaymentInitiator {
                initiator_type: Some(BankOfAmericaPaymentInitiatorTypes::Customer),
                credential_stored_on_file: Some(true),
                stored_credential_used: None,
            }),
            merchant_intitiated_transaction: None,
        }),
    )
}

fn get_commerce_indicator(network: Option<String>) -> String {
    match network {
        Some(card_network) => match card_network.to_lowercase().as_str() {
            "amex" => "aesk",
            "discover" => "dipb",
            "mastercard" => "spa",
            "visa" => "internet",
            _ => "internet",
        },
        None => "internet",
    }
    .to_string()
}

pub fn get_error_reason(
    error_info: Option<String>,
    detailed_error_info: Option<String>,
    avs_error_info: Option<String>,
) -> Option<String> {
    match (error_info, detailed_error_info, avs_error_info) {
        (Some(message), Some(details), Some(avs_message)) => Some(format!(
            "{}, detailed_error_information: {}, avs_message: {}",
            message, details, avs_message
        )),
        (Some(message), Some(details), None) => Some(format!(
            "{}, detailed_error_information: {}",
            message, details
        )),
        (Some(message), None, Some(avs_message)) => {
            Some(format!("{}, avs_message: {}", message, avs_message))
        }
        (None, Some(details), Some(avs_message)) => {
            Some(format!("{}, avs_message: {}", details, avs_message))
        }
        (Some(message), None, None) => Some(message),
        (None, Some(details), None) => Some(details),
        (None, None, Some(avs_message)) => Some(avs_message),
        (None, None, None) => None,
    }
}
