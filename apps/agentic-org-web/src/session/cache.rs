//! Validated sessions in memory, keyed by cookie digest (web-api-v1 §4).
//!
//! Inserts and `session.ended` removals both run on the host connection's
//! reader thread in arrival order. An entry answers only for the connection
//! generation that validated it, and only with the proof digest it was
//! validated with. An ended session ID is remembered (bounded) per generation,
//! so a late insert can never bring it back.
use org_host_control::browser::{BrowserSession, Digest32};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Mutex,
};

/// Entries beyond this evict the oldest; a later request revalidates once.
const MAX_ENTRIES: usize = 4096;
/// Ended IDs remembered per generation; the oldest is forgotten first.
const MAX_TOMBSTONES: usize = 4096;

struct Entry {
    proof_digest: Digest32,
    session: BrowserSession,
    generation: u64,
}

#[derive(Default)]
struct State {
    generation: u64,
    entries: HashMap<[u8; 32], Entry>,
    order: VecDeque<[u8; 32]>,
    ended: HashSet<String>,
    ended_order: VecDeque<String>,
}
impl State {
    /// A new connection generation starts empty: nothing validated before it
    /// answers after it.
    fn enter(&mut self, generation: u64) {
        if generation != self.generation {
            *self = Self {
                generation,
                ..Self::default()
            };
        }
    }
}

#[derive(Default)]
pub struct SessionCache {
    state: Mutex<State>,
}
impl SessionCache {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// The cached session for this pair, if validated on `generation` with this
    /// proof and not idle at `now_ms`.
    pub fn lookup(
        &self,
        cookie: &Digest32,
        proof: &Digest32,
        generation: u64,
        now_ms: u64,
    ) -> Option<BrowserSession> {
        let state = self.state();
        let entry = state.entries.get(&cookie.0)?;
        (entry.generation == generation
            && state.generation == generation
            && entry.proof_digest == *proof
            && now_ms < entry.session.idle_expires_at_ms)
            .then(|| entry.session.clone())
    }
    /// Records a session the host just validated or created on `generation`.
    /// Refused when the session already ended on this generation.
    pub fn insert(
        &self,
        cookie: Digest32,
        proof: Digest32,
        session: BrowserSession,
        generation: u64,
    ) -> bool {
        let mut state = self.state();
        if generation < state.generation {
            return false;
        }
        state.enter(generation);
        if state.ended.contains(&session.session_id) {
            return false;
        }
        if state.entries.len() >= MAX_ENTRIES && !state.entries.contains_key(&cookie.0) {
            if let Some(oldest) = state.order.pop_front() {
                state.entries.remove(&oldest);
            }
        }
        if state
            .entries
            .insert(
                cookie.0,
                Entry {
                    proof_digest: proof,
                    session,
                    generation,
                },
            )
            .is_none()
        {
            state.order.push_back(cookie.0);
        }
        true
    }
    /// Drops one cached session entry by its cookie digest so the next read revalidates.
    pub fn invalidate_cookie(&self, cookie: &Digest32) {
        let mut state = self.state();
        state.entries.remove(&cookie.0);
        state.order.retain(|k| k != &cookie.0);
    }

    /// What `account_id` can open changed on `generation`: drop its entries, so its next
    /// request validates again and reads the new links. Nothing is remembered: a later
    /// validation of the same session is current and may be cached.
    pub fn account_changed(&self, account_id: &str, generation: u64) {
        let mut state = self.state();
        if generation < state.generation {
            return;
        }
        state.enter(generation);
        state
            .entries
            .retain(|_, e| e.session.account.id != account_id);
        let entries = &state.entries;
        let kept: VecDeque<[u8; 32]> = state
            .order
            .iter()
            .filter(|k| entries.contains_key(*k))
            .copied()
            .collect();
        state.order = kept;
    }
    /// The host ended `session_id` on `generation`: drop every entry for it
    /// and refuse any later insert of it.
    pub fn ended(&self, session_id: &str, generation: u64) {
        let mut state = self.state();
        if generation < state.generation {
            return;
        }
        state.enter(generation);
        state
            .entries
            .retain(|_, e| e.session.session_id != session_id);
        let entries = &state.entries;
        let kept: VecDeque<[u8; 32]> = state
            .order
            .iter()
            .filter(|k| entries.contains_key(*k))
            .copied()
            .collect();
        state.order = kept;
        if state.ended.insert(session_id.to_owned()) {
            state.ended_order.push_back(session_id.to_owned());
            if state.ended_order.len() > MAX_TOMBSTONES {
                if let Some(old) = state.ended_order.pop_front() {
                    state.ended.remove(&old);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_host_control::browser::BrowserAccount;

    fn session(id: &str) -> BrowserSession {
        BrowserSession {
            session_id: id.into(),
            account: BrowserAccount {
                id: "a".into(),
                login: "alex".into(),
                time_zone: None,
                last_org_id: None,
            },
            admin: true,
            orgs: Vec::new(),
            server_time_ms: 1,
            idle_expires_at_ms: 1_000,
        }
    }
    const C: Digest32 = Digest32([1; 32]);
    const P: Digest32 = Digest32([2; 32]);

    #[test]
    fn an_entry_answers_only_its_generation_proof_and_idle_window() {
        let cache = SessionCache::default();
        assert!(cache.insert(C, P, session("s"), 1));
        assert!(cache.lookup(&C, &P, 1, 10).is_some());
        assert!(cache.lookup(&C, &Digest32([3; 32]), 1, 10).is_none());
        assert!(cache.lookup(&C, &P, 1, 1_000).is_none());
        assert!(cache.lookup(&C, &P, 2, 10).is_none());
        // A newer generation empties the cache; an older one never inserts.
        assert!(cache.insert(Digest32([9; 32]), P, session("t"), 2));
        assert!(cache.lookup(&C, &P, 1, 10).is_none());
        assert!(!cache.insert(C, P, session("s"), 1));
    }

    #[test]
    fn ended_drops_every_entry_and_tombstones_the_session() {
        let cache = SessionCache::default();
        cache.insert(C, P, session("s"), 1);
        cache.insert(Digest32([5; 32]), P, session("s"), 1);
        cache.ended("s", 1);
        assert!(cache.lookup(&C, &P, 1, 10).is_none());
        assert!(cache.lookup(&Digest32([5; 32]), &P, 1, 10).is_none());
        assert!(
            !cache.insert(C, P, session("s"), 1),
            "an ended session never returns"
        );
        assert!(cache.insert(C, P, session("other"), 1));
    }

    #[test]
    fn entries_and_tombstones_stay_bounded() {
        let cache = SessionCache::default();
        for n in 0..(MAX_ENTRIES + 10) {
            let mut key = [0u8; 32];
            key[..8].copy_from_slice(&(n as u64).to_le_bytes());
            cache.insert(Digest32(key), P, session(&n.to_string()), 1);
        }
        for n in 0..(MAX_TOMBSTONES + 10) {
            cache.ended(&format!("gone-{n}"), 1);
        }
        let state = cache.state();
        assert!(state.entries.len() <= MAX_ENTRIES);
        assert_eq!(state.order.len(), state.entries.len());
        assert!(state.ended.len() <= MAX_TOMBSTONES);
    }
}
