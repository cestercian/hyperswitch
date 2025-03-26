use std::{marker::PhantomData, str::FromStr};

use api_models::{payments as api_payments, webhooks};
use common_utils::{
    ext_traits::{AsyncExt, ValueExt},
    id_type,
};
use diesel_models::{process_tracker as storage, schema::process_tracker::retry_count};
use error_stack::{report, ResultExt};
use hyperswitch_domain_models::{
    errors::api_error_response, revenue_recovery, router_data_v2::flow_common_types,
    router_flow_types, router_request_types::revenue_recovery as revenue_recovery_request,
    router_response_types::revenue_recovery as revenue_recovery_response,
    types::AdditionalRevenueRecoveryDetailsRouterData,
};
use hyperswitch_interfaces::{api::RevenueRecovery, webhooks as interface_webhooks};
use router_env::{instrument, tracing};
use serde_with::rust::unwrap_or_skip;

use crate::{
    core::{
        errors::{self, CustomResult},
        payments::{self, helpers},
    },
    db::{errors::RevenueRecoveryError, StorageInterface},
    routes::{app::ReqState, metrics, SessionState},
    services::{
        self,
        connector_integration_interface::{self, RouterDataConversion},
    },
    types::{self, api, domain, storage::passive_churn_recovery as storage_churn_recovery},
    workflows::passive_churn_recovery_workflow,
};

#[allow(clippy::too_many_arguments)]
#[instrument(skip_all)]
#[cfg(feature = "revenue_recovery")]
pub async fn recovery_incoming_webhook_flow(
    state: SessionState,
    merchant_account: domain::MerchantAccount,
    business_profile: domain::Profile,
    key_store: domain::MerchantKeyStore,
    _webhook_details: api::IncomingWebhookDetails,
    source_verified: bool,
    connector_enum: &connector_integration_interface::ConnectorEnum,
    billing_connector_account: hyperswitch_domain_models::merchant_connector_account::MerchantConnectorAccount,
    connector_name: &str,
    request_details: &hyperswitch_interfaces::webhooks::IncomingWebhookRequestDetails<'_>,
    event_type: webhooks::IncomingWebhookEvent,
    req_state: ReqState,
    object_ref_id: &webhooks::ObjectReferenceId,
) -> CustomResult<webhooks::WebhookResponseTracker, errors::RevenueRecoveryError> {
    // Source verification is necessary for revenue recovery webhooks flow since We don't have payment intent/attempt object created before in our system.
    common_utils::fp_utils::when(!source_verified, || {
        Err(report!(
            errors::RevenueRecoveryError::WebhookAuthenticationFailed
        ))
    })?;

    let connector = api_models::enums::Connector::from_str(connector_name)
        .change_context(errors::RevenueRecoveryError::InvoiceWebhookProcessingFailed)
        .attach_printable_lazy(|| format!("unable to parse connector name {connector_name:?}"))?;

    let billing_connectors_which_require_payment_api_call =
        &state.conf.additional_revenue_recovery_details_call;

    let should_billing_connector_payment_api_called =
        billing_connectors_which_require_payment_api_call
            .connectors_with_additional_revenue_recovery_details_call
            .contains(&connector);

    let billing_connector_payment_details =
        AdditionalRevenueRecoveryResponse::get_billing_connector_payment_details(
            should_billing_connector_payment_api_called,
            &state,
            &merchant_account,
            &billing_connector_account,
            connector_name,
            object_ref_id,
        )
        .await?;

    // Checks whether we have data in recovery_details , If its there then it will use the data and convert it into required from or else fetches from Incoming webhook

    let invoice_details = RevenueRecoveryInvoice::get_recovery_invoice_details(
        connector_enum,
        request_details,
        billing_connector_payment_details.as_ref(),
    )?;

    // Fetch the intent using merchant reference id, if not found create new intent.
    let payment_intent = invoice_details
        .get_payment_intent(
            &state,
            &req_state,
            &merchant_account,
            &business_profile,
            &key_store,
        )
        .await
        .transpose()
        .async_unwrap_or_else(|| async {
            invoice_details
                .create_payment_intent(
                    &state,
                    &req_state,
                    &merchant_account,
                    &business_profile,
                    &key_store,
                )
                .await
        })
        .await?;

    let is_event_recovery_transaction_event = event_type.is_recovery_transaction_event();

    let payment_attempt = RevenueRecoveryAttempt::get_recovery_payment_attempt(
        is_event_recovery_transaction_event,
        &billing_connector_account,
        &state,
        &key_store,
        connector_enum,
        &req_state,
        billing_connector_payment_details.as_ref(),
        request_details,
        &merchant_account,
        &business_profile,
        &payment_intent,
    )
    .await?;

    let attempt_triggered_by = payment_attempt
        .as_ref()
        .and_then(revenue_recovery::RecoveryPaymentAttempt::get_attempt_triggered_by);

    let action = revenue_recovery::RecoveryAction::get_action(event_type, attempt_triggered_by);

    match action {
        revenue_recovery::RecoveryAction::CancelInvoice => todo!(),
        revenue_recovery::RecoveryAction::ScheduleFailedPayment => {
            Ok(RevenueRecoveryAttempt::insert_execute_pcr_task(
                &*state.store,
                merchant_account.get_id().to_owned(),
                payment_intent,
                business_profile.get_id().to_owned(),
                payment_attempt.map(|attempt| attempt.attempt_id.clone()),
                storage::ProcessTrackerRunner::PassiveRecoveryWorkflow,
            )
            .await
            .change_context(errors::RevenueRecoveryError::InvoiceWebhookProcessingFailed)?)
        }
        revenue_recovery::RecoveryAction::SuccessPaymentExternal => {
            // Need to add recovery stop flow for this scenario
            router_env::logger::info!("Payment has been succeeded via external system");
            Ok(webhooks::WebhookResponseTracker::NoEffect)
        }
        revenue_recovery::RecoveryAction::PendingPayment => {
            router_env::logger::info!(
                "Pending transactions are not consumed by the revenue recovery webhooks"
            );
            Ok(webhooks::WebhookResponseTracker::NoEffect)
        }
        revenue_recovery::RecoveryAction::NoAction => {
            router_env::logger::info!(
                "No Recovery action is taken place for recovery event : {:?} and attempt triggered_by : {:?} ", event_type.clone(), attempt_triggered_by
            );
            Ok(webhooks::WebhookResponseTracker::NoEffect)
        }
        revenue_recovery::RecoveryAction::InvalidAction => {
            router_env::logger::error!(
                "Invalid Revenue recovery action state has been received, event : {:?}, triggered_by : {:?}", event_type, attempt_triggered_by
            );
            Ok(webhooks::WebhookResponseTracker::NoEffect)
        }
    }
}
#[derive(Debug)]
pub struct RevenueRecoveryInvoice(revenue_recovery::RevenueRecoveryInvoiceData);
#[derive(Debug)]
pub struct RevenueRecoveryAttempt(revenue_recovery::RevenueRecoveryAttemptData);

impl RevenueRecoveryInvoice {
    fn get_recovery_invoice_details(
        connector_enum: &connector_integration_interface::ConnectorEnum,
        request_details: &hyperswitch_interfaces::webhooks::IncomingWebhookRequestDetails<'_>,
        billing_connector_payment_details: Option<
            &revenue_recovery_response::GetAdditionalRevenueRecoveryResponseData,
        >,
    ) -> CustomResult<Self, errors::RevenueRecoveryError> {
        billing_connector_payment_details.map_or_else(
            || {
                interface_webhooks::IncomingWebhook::get_revenue_recovery_invoice_details(
                    connector_enum,
                    request_details,
                )
                .change_context(errors::RevenueRecoveryError::InvoiceWebhookProcessingFailed)
                .attach_printable("Failed while getting revenue recovery invoice details")
                .map(RevenueRecoveryInvoice)
            },
            |data| {
                Ok(Self(revenue_recovery::RevenueRecoveryInvoiceData::from(
                    data,
                )))
            },
        )
    }

    async fn get_payment_intent(
        &self,
        state: &SessionState,
        req_state: &ReqState,
        merchant_account: &domain::MerchantAccount,
        profile: &domain::Profile,
        key_store: &domain::MerchantKeyStore,
    ) -> CustomResult<Option<revenue_recovery::RecoveryPaymentIntent>, errors::RevenueRecoveryError>
    {
        let payment_response = Box::pin(payments::payments_get_intent_using_merchant_reference(
            state.clone(),
            merchant_account.clone(),
            profile.clone(),
            key_store.clone(),
            req_state.clone(),
            &self.0.merchant_reference_id,
            hyperswitch_domain_models::payments::HeaderPayload::default(),
            None,
        ))
        .await;
        let response = match payment_response {
            Ok(services::ApplicationResponse::JsonWithHeaders((payments_response, _))) => {
                let payment_id = payments_response.id.clone();
                let status = payments_response.status;
                let feature_metadata = payments_response.feature_metadata;
                Ok(Some(revenue_recovery::RecoveryPaymentIntent {
                    payment_id,
                    status,
                    feature_metadata,
                }))
            }
            Err(err)
                if matches!(
                    err.current_context(),
                    &errors::ApiErrorResponse::PaymentNotFound
                ) =>
            {
                Ok(None)
            }
            Ok(_) => Err(errors::RevenueRecoveryError::PaymentIntentFetchFailed)
                .attach_printable("Unexpected response from payment intent core"),
            error @ Err(_) => {
                router_env::logger::error!(?error);
                Err(errors::RevenueRecoveryError::PaymentIntentFetchFailed)
                    .attach_printable("failed to fetch payment intent recovery webhook flow")
            }
        }?;
        Ok(response)
    }
    async fn create_payment_intent(
        &self,
        state: &SessionState,
        req_state: &ReqState,
        merchant_account: &domain::MerchantAccount,
        profile: &domain::Profile,
        key_store: &domain::MerchantKeyStore,
    ) -> CustomResult<revenue_recovery::RecoveryPaymentIntent, errors::RevenueRecoveryError> {
        let payload = api_payments::PaymentsCreateIntentRequest::from(&self.0);
        let global_payment_id = id_type::GlobalPaymentId::generate(&state.conf.cell_information.id);

        let create_intent_response = Box::pin(payments::payments_intent_core::<
            router_flow_types::payments::PaymentCreateIntent,
            api_payments::PaymentsIntentResponse,
            _,
            _,
            hyperswitch_domain_models::payments::PaymentIntentData<
                router_flow_types::payments::PaymentCreateIntent,
            >,
        >(
            state.clone(),
            req_state.clone(),
            merchant_account.clone(),
            profile.clone(),
            key_store.clone(),
            payments::operations::PaymentIntentCreate,
            payload,
            global_payment_id,
            hyperswitch_domain_models::payments::HeaderPayload::default(),
            None,
        ))
        .await
        .change_context(errors::RevenueRecoveryError::PaymentIntentCreateFailed)?;

        let response = create_intent_response
            .get_json_body()
            .change_context(errors::RevenueRecoveryError::PaymentIntentCreateFailed)
            .attach_printable("expected json response")?;

        Ok(revenue_recovery::RecoveryPaymentIntent {
            payment_id: response.id,
            status: response.status,
            feature_metadata: response.feature_metadata,
        })
    }
}

impl RevenueRecoveryAttempt {
    fn get_recovery_invoice_transaction_details(
        connector_enum: &connector_integration_interface::ConnectorEnum,
        request_details: &hyperswitch_interfaces::webhooks::IncomingWebhookRequestDetails<'_>,
        billing_connector_payment_details: Option<
            &revenue_recovery_response::GetAdditionalRevenueRecoveryResponseData,
        >,
    ) -> CustomResult<Self, errors::RevenueRecoveryError> {
        billing_connector_payment_details.map_or_else(
            || {
                interface_webhooks::IncomingWebhook::get_revenue_recovery_attempt_details(
                    connector_enum,
                    request_details,
                )
                .change_context(errors::RevenueRecoveryError::TransactionWebhookProcessingFailed)
                .attach_printable(
                    "Failed to get recovery attempt details from the billing connector",
                )
                .map(RevenueRecoveryAttempt)
            },
            |data| {
                Ok(Self(revenue_recovery::RevenueRecoveryAttemptData::from(
                    data,
                )))
            },
        )
    }

    async fn get_payment_attempt(
        &self,
        state: &SessionState,
        req_state: &ReqState,
        merchant_account: &domain::MerchantAccount,
        profile: &domain::Profile,
        key_store: &domain::MerchantKeyStore,
        payment_id: id_type::GlobalPaymentId,
    ) -> CustomResult<Option<revenue_recovery::RecoveryPaymentAttempt>, errors::RevenueRecoveryError>
    {
        let attempt_response = Box::pin(payments::payments_core::<
            router_flow_types::payments::PSync,
            api_payments::PaymentsResponse,
            _,
            _,
            _,
            hyperswitch_domain_models::payments::PaymentStatusData<
                router_flow_types::payments::PSync,
            >,
        >(
            state.clone(),
            req_state.clone(),
            merchant_account.clone(),
            profile.clone(),
            key_store.clone(),
            payments::operations::PaymentGet,
            api_payments::PaymentsRetrieveRequest {
                force_sync: false,
                expand_attempts: true,
                param: None,
            },
            payment_id.clone(),
            payments::CallConnectorAction::Avoid,
            hyperswitch_domain_models::payments::HeaderPayload::default(),
        ))
        .await;
        let response = match attempt_response {
            Ok(services::ApplicationResponse::JsonWithHeaders((payments_response, _))) => {
                let final_attempt =
                    self.0
                        .connector_transaction_id
                        .as_ref()
                        .and_then(|transaction_id| {
                            payments_response
                                .find_attempt_in_attempts_list_using_connector_transaction_id(
                                    transaction_id,
                                )
                        });
                let payment_attempt =
                    final_attempt.map(|attempt_res| revenue_recovery::RecoveryPaymentAttempt {
                        attempt_id: attempt_res.id.to_owned(),
                        attempt_status: attempt_res.status.to_owned(),
                        feature_metadata: attempt_res.feature_metadata.to_owned(),
                    });
                Ok(payment_attempt)
            }
            Ok(_) => Err(errors::RevenueRecoveryError::PaymentAttemptFetchFailed)
                .attach_printable("Unexpected response from payment intent core"),
            error @ Err(_) => {
                router_env::logger::error!(?error);
                Err(errors::RevenueRecoveryError::PaymentAttemptFetchFailed)
                    .attach_printable("failed to fetch payment attempt in recovery webhook flow")
            }
        }?;
        Ok(response)
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_payment_attempt(
        &self,
        state: &SessionState,
        req_state: &ReqState,
        merchant_account: &domain::MerchantAccount,
        profile: &domain::Profile,
        key_store: &domain::MerchantKeyStore,
        payment_id: id_type::GlobalPaymentId,
        billing_connector_account_id: &id_type::MerchantConnectorAccountId,
        payment_connector_account: Option<domain::MerchantConnectorAccount>,
    ) -> CustomResult<revenue_recovery::RecoveryPaymentAttempt, errors::RevenueRecoveryError> {
        let request_payload = self
            .create_payment_record_request(billing_connector_account_id, payment_connector_account);
        let attempt_response = Box::pin(payments::record_attempt_core(
            state.clone(),
            req_state.clone(),
            merchant_account.clone(),
            profile.clone(),
            key_store.clone(),
            request_payload,
            payment_id.clone(),
            hyperswitch_domain_models::payments::HeaderPayload::default(),
            None,
        ))
        .await;

        let response = match attempt_response {
            Ok(services::ApplicationResponse::JsonWithHeaders((attempt_response, _))) => {
                Ok(revenue_recovery::RecoveryPaymentAttempt {
                    attempt_id: attempt_response.id,
                    attempt_status: attempt_response.status,
                    feature_metadata: attempt_response.feature_metadata,
                })
            }
            Ok(_) => Err(errors::RevenueRecoveryError::PaymentAttemptFetchFailed)
                .attach_printable("Unexpected response from record attempt core"),
            error @ Err(_) => {
                router_env::logger::error!(?error);
                Err(errors::RevenueRecoveryError::PaymentAttemptFetchFailed)
                    .attach_printable("failed to record attempt in recovery webhook flow")
            }
        }?;
        Ok(response)
    }

    pub fn create_payment_record_request(
        &self,
        billing_merchant_connector_account_id: &id_type::MerchantConnectorAccountId,
        payment_merchant_connector_account: Option<domain::MerchantConnectorAccount>,
    ) -> api_payments::PaymentsAttemptRecordRequest {
        let amount_details = api_payments::PaymentAttemptAmountDetails::from(&self.0);
        let feature_metadata = api_payments::PaymentAttemptFeatureMetadata {
            revenue_recovery: Some(api_payments::PaymentAttemptRevenueRecoveryData {
                // Since we are recording the external paymenmt attempt, this is hardcoded to External
                attempt_triggered_by: common_enums::TriggeredBy::External,
            }),
        };
        let error = Option::<api_payments::RecordAttemptErrorDetails>::from(&self.0);
        api_payments::PaymentsAttemptRecordRequest {
            amount_details,
            status: self.0.status,
            billing: None,
            shipping: None,
            connector : payment_merchant_connector_account.as_ref().map(|account| account.connector_name),
            payment_merchant_connector_id: payment_merchant_connector_account.as_ref().map(|account: &hyperswitch_domain_models::merchant_connector_account::MerchantConnectorAccount| account.id.clone()),
            error,
            description: None,
            connector_transaction_id: self.0.connector_transaction_id.clone(),
            payment_method_type: self.0.payment_method_type,
            billing_connector_id: billing_merchant_connector_account_id.clone(),
            payment_method_subtype: self.0.payment_method_sub_type,
            payment_method_data: None,
            metadata: None,
            feature_metadata: Some(feature_metadata),
            transaction_created_at: self.0.transaction_created_at,
            processor_payment_method_token: self.0.processor_payment_method_token.clone(),
            connector_customer_id: self.0.connector_customer_id.clone(),
        }
    }

    pub async fn find_payment_merchant_connector_account(
        &self,
        state: &SessionState,
        key_store: &domain::MerchantKeyStore,
        billing_connector_account: &domain::MerchantConnectorAccount,
    ) -> CustomResult<Option<domain::MerchantConnectorAccount>, errors::RevenueRecoveryError> {
        let payment_merchant_connector_account_id = billing_connector_account
            .get_payment_merchant_connector_account_id_using_account_reference_id(
                self.0.connector_account_reference_id.clone(),
            );
        let db = &*state.store;
        let key_manager_state = &(state).into();
        let payment_merchant_connector_account = payment_merchant_connector_account_id
            .as_ref()
            .async_map(|mca_id| async move {
                db.find_merchant_connector_account_by_id(key_manager_state, mca_id, key_store)
                    .await
            })
            .await
            .transpose()
            .change_context(errors::RevenueRecoveryError::PaymentMerchantConnectorAccountNotFound)
            .attach_printable(
                "failed to fetch payment merchant connector id using account reference id",
            )?;
        Ok(payment_merchant_connector_account)
    }

    #[allow(clippy::too_many_arguments)]
    async fn get_recovery_payment_attempt(
        is_recovery_transaction_event: bool,
        billing_connector_account: &domain::MerchantConnectorAccount,
        state: &SessionState,
        key_store: &domain::MerchantKeyStore,
        connector_enum: &connector_integration_interface::ConnectorEnum,
        req_state: &ReqState,
        billing_connector_payment_details: Option<
            &revenue_recovery_response::GetAdditionalRevenueRecoveryResponseData,
        >,
        request_details: &hyperswitch_interfaces::webhooks::IncomingWebhookRequestDetails<'_>,
        merchant_account: &domain::MerchantAccount,
        business_profile: &domain::Profile,
        payment_intent: &revenue_recovery::RecoveryPaymentIntent,
    ) -> CustomResult<Option<revenue_recovery::RecoveryPaymentAttempt>, errors::RevenueRecoveryError>
    {
        let recovery_payment_attempt = match is_recovery_transaction_event {
            true => {
                // Checks whether we have data in recovery_details , If its there then it will use the data and convert it into required from or else fetches from Incoming webhook
                let invoice_transaction_details = Self::get_recovery_invoice_transaction_details(
                    connector_enum,
                    request_details,
                    billing_connector_payment_details,
                )?;

                // Find the payment merchant connector ID at the top level to avoid multiple DB calls.
                let payment_merchant_connector_account = invoice_transaction_details
                    .find_payment_merchant_connector_account(
                        state,
                        key_store,
                        billing_connector_account,
                    )
                    .await?;

                Some(
                    invoice_transaction_details
                        .get_payment_attempt(
                            state,
                            req_state,
                            merchant_account,
                            business_profile,
                            key_store,
                            payment_intent.payment_id.clone(),
                        )
                        .await
                        .transpose()
                        .async_unwrap_or_else(|| async {
                            invoice_transaction_details
                                .record_payment_attempt(
                                    state,
                                    req_state,
                                    merchant_account,
                                    business_profile,
                                    key_store,
                                    payment_intent.payment_id.clone(),
                                    &billing_connector_account.id,
                                    payment_merchant_connector_account,
                                )
                                .await
                        })
                        .await?,
                )
            }
            false => None,
        };

        Ok(recovery_payment_attempt)
    }

    async fn insert_execute_pcr_task(
        db: &dyn StorageInterface,
        merchant_id: id_type::MerchantId,
        payment_intent: revenue_recovery::RecoveryPaymentIntent,
        profile_id: id_type::ProfileId,
        payment_attempt_id: Option<id_type::GlobalAttemptId>,
        runner: storage::ProcessTrackerRunner,
    ) -> CustomResult<webhooks::WebhookResponseTracker, errors::RevenueRecoveryError> {
        let task = "EXECUTE_WORKFLOW";

        let payment_id = payment_intent.payment_id.clone();

        let process_tracker_id = format!("{runner}_{task}_{}", payment_id.get_string_repr());

        let total_retry_count = payment_intent
            .feature_metadata
            .and_then(|feature_metadata| feature_metadata.get_retry_count())
            .unwrap_or(0);

        let schedule_time =
            passive_churn_recovery_workflow::get_schedule_time_to_retry_mit_payments(
                db,
                &merchant_id,
                (total_retry_count + 1).into(),
            )
            .await
            .map_or_else(
                || {
                    Err(
                        report!(errors::RevenueRecoveryError::ScheduleTimeFetchFailed)
                            .attach_printable("Failed to get schedule time for pcr workflow"),
                    )
                },
                Ok, // Simply returns `time` wrapped in `Ok`
            )?;

        let payment_attempt_id = payment_attempt_id
            .ok_or(report!(
                errors::RevenueRecoveryError::PaymentAttemptIdNotFound
            ))
            .attach_printable("payment attempt id is required for pcr workflow tracking")?;

        let execute_workflow_tracking_data = storage_churn_recovery::PcrWorkflowTrackingData {
            global_payment_id: payment_id.clone(),
            merchant_id,
            profile_id,
            payment_attempt_id,
        };

        let tag = ["PCR"];

        let process_tracker_entry = storage::ProcessTrackerNew::new(
            process_tracker_id,
            task,
            runner,
            tag,
            execute_workflow_tracking_data,
            Some(total_retry_count.into()),
            schedule_time,
            common_enums::ApiVersion::V2,
        )
        .change_context(errors::RevenueRecoveryError::ProcessTrackerCreationError)
        .attach_printable("Failed to construct process tracker entry")?;

        db.insert_process(process_tracker_entry)
            .await
            .change_context(errors::RevenueRecoveryError::ProcessTrackerResponseError)
            .attach_printable("Failed to enter process_tracker_entry in DB")?;
        metrics::TASKS_ADDED_COUNT.add(1, router_env::metric_attributes!(("flow", "ExecutePCR")));

        Ok(webhooks::WebhookResponseTracker::Payment {
            payment_id,
            status: payment_intent.status,
        })
    }
}

pub struct AdditionalRevenueRecoveryResponse(
    revenue_recovery_response::GetAdditionalRevenueRecoveryResponseData,
);
pub struct AdditionalRevenueRecoveryRouterData(AdditionalRevenueRecoveryDetailsRouterData);

impl AdditionalRevenueRecoveryResponse {
    async fn handle_additional_recovery_details_call(
        state: &SessionState,
        merchant_account: &domain::MerchantAccount,
        merchant_connector_account: &hyperswitch_domain_models::merchant_connector_account::MerchantConnectorAccount,
        connector_name: &str,
        id: &str,
    ) -> CustomResult<Self, errors::RevenueRecoveryError> {
        let connector_data = api::ConnectorData::get_connector_by_name(
            &state.conf.connectors,
            connector_name,
            api::GetToken::Connector,
            None,
        )
        .change_context(errors::RevenueRecoveryError::AdditionalRevenueRecoveryCallFailed)
        .attach_printable("invalid connector name received in payment attempt")?;

        let connector_integration: services::BoxedGetAdditionalRecoveryRecoveryDetailsIntegrationInterface<
        router_flow_types::GetAdditionalRevenueRecoveryDetails,
        revenue_recovery_request::GetAdditionalRevenueRecoveryRequestData,
        revenue_recovery_response::GetAdditionalRevenueRecoveryResponseData
        > = connector_data.connector.get_connector_integration();

        let router_data =
            AdditionalRevenueRecoveryRouterData::construct_router_data_for_additional_call(
                state,
                connector_name,
                merchant_connector_account,
                merchant_account,
                id,
            )
            .await
            .change_context(errors::RevenueRecoveryError::AdditionalRevenueRecoveryCallFailed)
            .attach_printable(
                "Failed while constructing additional recovery details call router data",
            )?
            .inner();

        let response = services::execute_connector_processing_step(
            state,
            connector_integration,
            &router_data,
            payments::CallConnectorAction::Trigger,
            None,
        )
        .await
        .change_context(errors::RevenueRecoveryError::AdditionalRevenueRecoveryCallFailed)
        .attach_printable("Failed while fetching additional revenue recovery details")?;

        let additional_recovery_details = match response.response {
            Ok(response) => Ok(response),
            error @ Err(_) => {
                router_env::logger::error!(?error);
                Err(errors::RevenueRecoveryError::AdditionalRevenueRecoveryCallFailed)
                    .attach_printable("Failed while fetching additional revenue recovery details")
            }
        }?;
        Ok(Self(additional_recovery_details))
    }

    async fn get_billing_connector_payment_details(
        should_billing_connector_payment_api_called: bool,
        state: &SessionState,
        merchant_account: &domain::MerchantAccount,
        billing_connector_account: &hyperswitch_domain_models::merchant_connector_account::MerchantConnectorAccount,
        connector_name: &str,
        object_ref_id: &webhooks::ObjectReferenceId,
    ) -> CustomResult<
        Option<revenue_recovery_response::GetAdditionalRevenueRecoveryResponseData>,
        errors::RevenueRecoveryError,
    > {
        let response_data = match should_billing_connector_payment_api_called {
            true => {
                let billing_connector_transaction_id = object_ref_id
                    .clone()
                    .get_connector_transaction_id_as_string()
                    .change_context(
                        errors::RevenueRecoveryError::AdditionalRevenueRecoveryCallFailed,
                    )
                    .attach_printable("Billing connector Payments api call failed")?;
                let billing_connector_payment_details =
                    Self::handle_additional_recovery_details_call(
                        state,
                        merchant_account,
                        billing_connector_account,
                        connector_name,
                        &billing_connector_transaction_id,
                    )
                    .await?;
                Some(billing_connector_payment_details.inner())
            }
            false => None,
        };

        Ok(response_data)
    }

    fn inner(self) -> revenue_recovery_response::GetAdditionalRevenueRecoveryResponseData {
        self.0
    }
}

impl AdditionalRevenueRecoveryRouterData {
    async fn construct_router_data_for_additional_call(
        state: &SessionState,
        connector_name: &str,
        merchant_connector_account: &hyperswitch_domain_models::merchant_connector_account::MerchantConnectorAccount,
        merchant_account: &domain::MerchantAccount,
        additional_revenue_recovery_id: &str,
    ) -> CustomResult<Self, errors::RevenueRecoveryError> {
        let auth_type: types::ConnectorAuthType = helpers::MerchantConnectorAccountType::DbVal(
            Box::new(merchant_connector_account.clone()),
        )
        .get_connector_account_details()
        .parse_value("ConnectorAuthType")
        .change_context(errors::RevenueRecoveryError::AdditionalRevenueRecoveryCallFailed)?;

        let router_data = types::RouterDataV2 {
            flow: PhantomData::<router_flow_types::GetAdditionalRevenueRecoveryDetails>,
            tenant_id: state.tenant.tenant_id.clone(),
            resource_common_data: flow_common_types::GetAdditionalRevenueRecoveryFlowCommonData,
            connector_auth_type: auth_type,
            request: revenue_recovery_request::GetAdditionalRevenueRecoveryRequestData {
                additional_revenue_recovery_id: additional_revenue_recovery_id.to_string(),
            },
            response: Err(types::ErrorResponse::default()),
        };

        let old_router_data =
            flow_common_types::GetAdditionalRevenueRecoveryFlowCommonData::to_old_router_data(
                router_data,
            )
            .change_context(errors::RevenueRecoveryError::AdditionalRevenueRecoveryCallFailed)
            .attach_printable("Cannot construct router data for making the additional call")?;

        Ok(Self(old_router_data))
    }

    fn inner(self) -> AdditionalRevenueRecoveryDetailsRouterData {
        self.0
    }
}
