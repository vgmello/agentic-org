//! Enabled hints are at least once: committed changes reach a visible recount/hint within2s.
use super::{recount, Hq, Source};
use crate::{
    follow::OrgFeed,
    live::{Live, LiveEvents},
};
use org_proto::{inbox::InboxChangedData, org::Position};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

const GAP: Duration = Duration::from_millis(250);

pub(crate) struct Hint(Mutex<State>);
struct State {
    org: Position,
    mailbox: u64,
    sent: Option<Instant>,
    pending: Option<InboxChangedData>,
}

#[cfg(test)]
mod tests;

impl Hint {
    pub(crate) fn new(org: Position, mailbox: u64) -> Self {
        Self(Mutex::new(State {
            org,
            mailbox,
            sent: None,
            pending: None,
        }))
    }

    /// Seq is monotonic; duplicate notifications may overcount. Recount once per wake.
    /// The clock parameter controls only throttling; source reads use the real HQ clock.
    pub(crate) fn poll(
        &self,
        hq: &Hq,
        feed: &dyn OrgFeed,
        live: &LiveEvents,
        now: impl Fn() -> Instant,
    ) -> Duration {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let mut changed = false;
        if let Ok(upto) = feed.head() {
            while state.org < upto {
                let Ok(batch) = feed.changes_after(state.org, 100) else {
                    eprintln!("org-server: inbox organization feed unavailable");
                    break;
                };
                let mut advanced = false;
                for row in batch.into_iter().take_while(|row| row.position <= upto) {
                    hq.clock.changed();
                    state.org = row.position;
                    changed = true;
                    advanced = true;
                }
                if !advanced {
                    break;
                }
            }
        }
        let mailbox = live.mailbox_head();
        for _ in state.mailbox..mailbox {
            hq.clock.changed();
            changed = true;
        }
        state.mailbox = mailbox;
        if changed {
            let as_of = hq.clock.as_of();
            let sources: Vec<&dyn Source> = hq.sources.iter().map(|s| s.as_ref()).collect();
            let totals = recount(&sources);
            state.pending = Some(InboxChangedData {
                counts: totals.counts,
                as_of,
                degraded: totals.degraded,
            });
        }
        if state.pending.is_some() {
            let due = state.sent.map_or(Duration::ZERO, |at| {
                GAP.saturating_sub(now().saturating_duration_since(at))
            });
            if !due.is_zero() {
                return due;
            }
            live.publish_live(Live::InboxChanged(state.pending.take().expect("pending")));
            state.sent = Some(now());
        }
        Duration::MAX
    }
}
