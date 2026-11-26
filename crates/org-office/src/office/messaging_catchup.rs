//! The channel catch-up ladder's office reads (`TASK-99`).

use org_proto::{ApiError, ErrorCode};
use org_store::Id;

use crate::{
    error::OfficeError,
    messaging::features::ChannelCatchup,
    office::{storage, Office},
    store::open::ChannelCatchupRead,
    time,
};

impl Office {
    /// The caller's new messages in a channel after its read marker, oldest first, at most
    /// `limit`, for `channel summarize --since-read`. A pure read: the marker never moves.
    pub fn channel_catchup_for_caller(
        &self,
        caller: &str,
        channel_ref: &str,
        limit: usize,
    ) -> Result<Result<ChannelCatchup, ApiError>, OfficeError> {
        let Ok(caller_id) = Id::parse(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
        let (res, _) = self
            .store
            .read(ChannelCatchupRead {
                owner: self.effective_owner_for_ops(),
                caller: caller_id,
                channel_ref: channel_ref.to_owned(),
                limit,
                now_utc,
            })
            .map_err(storage)?;
        Ok(res)
    }
}
