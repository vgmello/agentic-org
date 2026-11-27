//! Adapter bridging `org-office` attachment delivery records and errors onto the decoupled
//! [`org_turn_attachments::TurnAttachmentResolver`] port (`TASK-255`).

use std::sync::Arc;

use org_office::attachments::records::AttachmentRecord;
use org_office::attachments::{DeliveryAttachmentPayload, DeliveryScope};
use org_office::{Office, OfficeError};
use org_proto::ApiError;
use org_turn_attachments::{
    TurnAttachmentMeta, TurnAttachmentResolveError, TurnAttachmentResolver, TurnDeliveryAttachment,
    TurnDeliveryScope, TurnImageDerivative,
};

/// Maps a decoupled [`TurnDeliveryScope`] onto `org-office`'s [`DeliveryScope`].
pub fn to_office_delivery_scope(scope: &TurnDeliveryScope) -> DeliveryScope {
    match scope {
        TurnDeliveryScope::None => DeliveryScope::None,
        TurnDeliveryScope::ActingForPerson(caller_id) => {
            DeliveryScope::ActingForPerson(caller_id.clone())
        }
        TurnDeliveryScope::DirectSender(sender_id) => {
            DeliveryScope::DirectSender(sender_id.clone())
        }
    }
}

/// Maps `org-office`'s [`DeliveryScope`] onto the decoupled [`TurnDeliveryScope`].
pub fn from_office_delivery_scope(scope: &DeliveryScope) -> TurnDeliveryScope {
    match scope {
        DeliveryScope::None => TurnDeliveryScope::None,
        DeliveryScope::ActingForPerson(caller_id) => {
            TurnDeliveryScope::ActingForPerson(caller_id.clone())
        }
        DeliveryScope::DirectSender(sender_id) => {
            TurnDeliveryScope::DirectSender(sender_id.clone())
        }
    }
}

/// Projects an `org-office` [`AttachmentRecord`] into [`TurnAttachmentMeta`].
pub fn from_office_attachment_record(record: AttachmentRecord) -> TurnAttachmentMeta {
    TurnAttachmentMeta {
        id: record.id.to_string(),
        filename: record.filename,
        title: record.title,
        mime_type: record.mime_type,
        byte_count: record.byte_count,
        description: record.description,
        page_count: record.page_count,
    }
}

/// Projects an `org-office` [`DeliveryAttachmentPayload`] into [`TurnDeliveryAttachment`].
pub fn from_office_delivery_payload(payload: DeliveryAttachmentPayload) -> TurnDeliveryAttachment {
    match payload {
        DeliveryAttachmentPayload::Image { record, derivative } => TurnDeliveryAttachment::Image {
            record: from_office_attachment_record(record),
            derivative: TurnImageDerivative {
                mime_type: derivative.media.mime().to_owned(),
                bytes: derivative.bytes,
                width: derivative.width,
                height: derivative.height,
            },
        },
        DeliveryAttachmentPayload::Document { record, bytes } => TurnDeliveryAttachment::Document {
            record: from_office_attachment_record(record),
            bytes,
        },
    }
}

/// Maps an `Office::attachment_read_for_delivery_with` result onto the decoupled
/// [`TurnAttachmentResolver::resolve_for_turn`] return type.
pub fn map_office_delivery_result(
    result: Result<Result<DeliveryAttachmentPayload, ApiError>, OfficeError>,
) -> Result<Result<TurnDeliveryAttachment, ApiError>, TurnAttachmentResolveError> {
    match result {
        Ok(Ok(payload)) => Ok(Ok(from_office_delivery_payload(payload))),
        Ok(Err(api_err)) => Ok(Err(api_err)),
        Err(err) => Err(TurnAttachmentResolveError::new(err.to_string())),
    }
}

/// Default local-office implementation of [`TurnAttachmentResolver`].
pub struct LocalOfficeAttachmentResolver {
    office: Arc<Office>,
}

impl LocalOfficeAttachmentResolver {
    pub fn new(office: Arc<Office>) -> Self {
        Self { office }
    }
}

impl TurnAttachmentResolver for LocalOfficeAttachmentResolver {
    fn resolve_for_turn(
        &self,
        employee_id: &str,
        delivery_scope: &TurnDeliveryScope,
        attachment_id: &str,
        remaining_staging_budget: u64,
    ) -> Result<Result<TurnDeliveryAttachment, ApiError>, TurnAttachmentResolveError> {
        let office_scope = to_office_delivery_scope(delivery_scope);
        map_office_delivery_result(self.office.attachment_read_for_delivery_with(
            employee_id,
            attachment_id,
            &office_scope,
            remaining_staging_budget,
            None,
        ))
    }
}
