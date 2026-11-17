//! Physical occupancy belongs to authenticated lifecycle receipts, never witness drops.
use org_office_launch::v3::{ChannelKind, MAX_CHANNELS, MAX_GATEWAY_CHANNELS};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Clone, Debug)]
pub(crate) struct ChannelSlot(Arc<AtomicBool>);
impl ChannelSlot {
    pub(crate) fn is_live(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
    #[cfg(test)]
    pub(crate) fn close_for_test(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
struct Entry {
    kind: ChannelKind,
    acknowledged: bool,
    slot: ChannelSlot,
}
pub(super) struct Registry {
    open: bool,
    entries: BTreeMap<String, Entry>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            open: true,
            entries: BTreeMap::new(),
        }
    }
}
impl Registry {
    pub(super) fn reserve(&mut self, id: &str, kind: ChannelKind) -> std::io::Result<ChannelSlot> {
        if !self.open
            || id.is_empty()
            || id == "handoff"
            || self.entries.contains_key(id)
            || self.entries.len() >= MAX_CHANNELS
            || (kind == ChannelKind::Gateway
                && self.entries.values().filter(|e| e.kind == kind).count() >= MAX_GATEWAY_CHANNELS)
        {
            return Err(super::invalid());
        }
        let slot = ChannelSlot(Arc::new(AtomicBool::new(true)));
        self.entries.insert(
            id.into(),
            Entry {
                kind,
                acknowledged: false,
                slot: slot.clone(),
            },
        );
        Ok(slot)
    }
    pub(super) fn acknowledge(&mut self, id: &str) -> bool {
        let Some(entry) = self.entries.get_mut(id) else {
            return false;
        };
        if entry.acknowledged {
            return false;
        }
        entry.acknowledged = true;
        true
    }
    pub(super) fn remove(&mut self, id: &str, acknowledged: bool) -> bool {
        if !self
            .entries
            .get(id)
            .is_some_and(|e| e.acknowledged == acknowledged)
        {
            return false;
        }
        let entry = self.entries.remove(id).expect("checked entry");
        entry.slot.0.store(false, Ordering::SeqCst);
        true
    }
    pub(super) fn fail(&mut self) {
        self.open = false;
        for entry in std::mem::take(&mut self.entries).into_values() {
            entry.slot.0.store(false, Ordering::SeqCst);
        }
    }
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}
#[cfg(test)]
mod tests;
