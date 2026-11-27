//! Batched session activity touches (`TASK-38` / A6): coalesces per-session
//! `last_activity_ms` updates in memory and flushes them in a single SQLite transaction
//! periodically (and on orderly shutdown / `Drop`) instead of writing SQLite on every
//! `session.validate` request.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::sessions::BrowserSessions;

/// Flush coalesced session touches at most every 5 seconds (or when `MAX_PENDING` sessions
/// accumulate, or on shutdown).
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const FLUSH_INTERVAL_MS: i64 = 5_000;
/// Synchronous flush margin near idle expiry (2x `FLUSH_INTERVAL_MS`).
pub(super) const NEAR_EXPIRY_WINDOW_MS: i64 = 2 * FLUSH_INTERVAL_MS;
const MAX_PENDING: usize = 256;

pub(crate) struct TouchBatcher {
    inner: Mutex<State>,
}

struct State {
    pending: BTreeMap<String, i64>,
    last_flush: Instant,
    last_flush_ms: Option<i64>,
}

impl TouchBatcher {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(State {
                pending: BTreeMap::new(),
                last_flush: Instant::now(),
                last_flush_ms: None,
            }),
        }
    }

    /// Record activity for `session_id` at `now_ms`. Flushes immediately if the batch reached
    /// `MAX_PENDING`, wall time elapsed `FLUSH_INTERVAL`, or `now_ms` advanced past
    /// `FLUSH_INTERVAL_MS` since the last flush (ensuring `last_activity_ms` is never stale by
    /// more than 5 seconds under either wall or injected clocks).
    pub(crate) fn record(&self, sessions: &dyn BrowserSessions, session_id: &str, now_ms: i64) {
        let flush_batch = {
            let Ok(mut state) = self.inner.lock() else {
                return;
            };
            let origin_ms = *state.last_flush_ms.get_or_insert(now_ms);
            state
                .pending
                .entry(session_id.to_owned())
                .and_modify(|prev| *prev = (*prev).max(now_ms))
                .or_insert(now_ms);
            let time_due = state.last_flush.elapsed() >= FLUSH_INTERVAL
                || now_ms.saturating_sub(origin_ms) >= FLUSH_INTERVAL_MS;
            if state.pending.len() >= MAX_PENDING || time_due {
                state.last_flush = Instant::now();
                state.last_flush_ms = Some(now_ms);
                Some(std::mem::take(&mut state.pending))
            } else {
                None
            }
        };
        if let Some(batch) = flush_batch {
            flush_to_store(sessions, batch, now_ms);
        }
    }

    /// Flush pending touches if `force` is true (shutdown / tests) or `FLUSH_INTERVAL` elapsed.
    pub(crate) fn flush(&self, sessions: &dyn BrowserSessions, now_ms: i64, force: bool) {
        let batch = {
            let Ok(mut state) = self.inner.lock() else {
                return;
            };
            let due = force
                || state.last_flush.elapsed() >= FLUSH_INTERVAL
                || state
                    .last_flush_ms
                    .is_some_and(|origin| now_ms.saturating_sub(origin) >= FLUSH_INTERVAL_MS);
            if state.pending.is_empty() || !due {
                return;
            }
            state.last_flush = Instant::now();
            state.last_flush_ms = Some(now_ms);
            std::mem::take(&mut state.pending)
        };
        flush_to_store(sessions, batch, now_ms);
    }

    /// Drop any buffered touch for ended sessions so they aren't pointlessly re-flushed.
    pub(crate) fn forget(&self, session_ids: &[&str]) {
        if session_ids.is_empty() {
            return;
        }
        if let Ok(mut state) = self.inner.lock() {
            for id in session_ids {
                state.pending.remove(*id);
            }
        }
    }
}

fn flush_to_store(sessions: &dyn BrowserSessions, batch: BTreeMap<String, i64>, now_ms: i64) {
    if batch.is_empty() {
        return;
    }
    let refs: Vec<(&str, i64)> = batch.iter().map(|(id, ms)| (id.as_str(), *ms)).collect();
    let _ = sessions.touch(&refs, now_ms);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hostdb::{HostDb, HostDbInit};
    use crate::sessions::{NewSession, Origin, Scheme, SessionDigest};

    struct NoSecrets;
    impl crate::host_secrets::HostSecrets for NoSecrets {
        fn put(
            &self,
            _: &crate::host_secrets::Name,
            _: &crate::host_secrets::Value,
        ) -> Result<(), crate::host_secrets::Error> {
            Ok(())
        }
        fn load(
            &self,
            _: &crate::host_secrets::Name,
        ) -> Result<Option<crate::host_secrets::Value>, crate::host_secrets::Error> {
            Ok(None)
        }
        fn delete(&self, _: &crate::host_secrets::Name) -> Result<(), crate::host_secrets::Error> {
            Ok(())
        }
        fn list(&self) -> Result<Vec<crate::host_secrets::Name>, crate::host_secrets::Error> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn coalesces_multiple_touches_into_one_batched_flush() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let db = HostDb::create(
            home.path(),
            &NoSecrets,
            &HostDbInit {
                instance_id: crate::ids::uuid_v7().unwrap(),
                clock_high_ms: 1_000,
            },
        )
        .unwrap();
        let account = crate::ids::uuid_v7().unwrap();
        let phc = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        db.seed_account(
            &account,
            "BatchOwner",
            &crate::hostdb::journal::Verifier::new(phc.into()).unwrap(),
            true,
            1_000,
        )
        .unwrap();
        let origin = Origin::new(Scheme::Http, "127.0.0.1", 7420).unwrap();
        let s1 = NewSession::new(
            &account,
            SessionDigest::cookie(b"c1"),
            SessionDigest::proof(b"p1"),
            origin.clone(),
        )
        .unwrap();
        let s2 = NewSession::new(
            &account,
            SessionDigest::cookie(b"c2"),
            SessionDigest::proof(b"p2"),
            origin.clone(),
        )
        .unwrap();
        db.create(&s1, 10_000).unwrap();
        db.create(&s2, 10_000).unwrap();

        let batcher = TouchBatcher::new();
        batcher.record(&db, &s1.id, 11_000);
        batcher.record(&db, &s1.id, 12_000);
        batcher.record(&db, &s2.id, 11_500);
        // Not flushed yet (within interval and below MAX_PENDING)
        let r1 = db
            .find(
                &SessionDigest::cookie(b"c1"),
                &SessionDigest::proof(b"p1"),
                &origin,
                12_000,
            )
            .unwrap()
            .unwrap();
        assert_eq!(r1.last_activity_ms, 10_000);

        // Forced flush writes the latest timestamp for each session in one transaction.
        batcher.flush(&db, 12_500, true);
        let r1 = db
            .find(
                &SessionDigest::cookie(b"c1"),
                &SessionDigest::proof(b"p1"),
                &origin,
                12_500,
            )
            .unwrap()
            .unwrap();
        let r2 = db
            .find(
                &SessionDigest::cookie(b"c2"),
                &SessionDigest::proof(b"p2"),
                &origin,
                12_500,
            )
            .unwrap()
            .unwrap();
        assert_eq!(r1.last_activity_ms, 12_000);
        assert_eq!(r2.last_activity_ms, 11_500);
    }
}
