//! Local reader primitive. The stream facade supplies current-HQ authorization and
//! the native transaction boundary; that contract is pinned before the API is frozen.
use super::*;

pub(super) fn read_head(tx: &mut dyn LocalWorkTxn) -> Result<Option<Change>, SyncError> {
    let context = tx.context()?;
    if versions::protocol_profile(context.protocol_level).is_none() {
        return Err(Refusal::UnsupportedVersion.into());
    }
    if context.restore_pending {
        return Err(Refusal::RestorePending.into());
    }
    let queue = tx.local_outbox()?;
    validate_queue(&context, &queue)?;
    let Some(change) = queue.pending().front() else {
        return Ok(None);
    };
    if !versions::codec_supported(context.protocol_level, &change.codec)
        || !tx.supports_codec(&change.codec)?
    {
        return Err(Refusal::UnsupportedVersion.into());
    }
    Ok(Some(change.clone()))
}
