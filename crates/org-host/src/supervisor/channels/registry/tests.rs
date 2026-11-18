use super::*;

#[test]
fn pending_reservations_share_physical_budgets_and_witness_drop_frees_nothing() {
    let mut registry = Registry::default();
    for n in 0..MAX_WEB_CHANNELS {
        drop(registry.reserve(&n.to_string(), ChannelKind::Web).unwrap());
    }
    assert!(registry.reserve("eighth-web", ChannelKind::Web).is_err());
    let local = registry.reserve("local", ChannelKind::Local).unwrap();
    assert!(registry.reserve("ninth-local", ChannelKind::Local).is_err());
    assert_eq!(registry.len(), MAX_CHANNELS);
    assert!(!registry.remove("local", true), "cannot close before ACK");
    assert!(registry.acknowledge("local"));
    assert!(!registry.acknowledge("local"), "duplicate ACK");
    assert!(!registry.remove("local", false), "cannot refuse after ACK");
    assert!(local.is_live());
    assert!(registry.remove("local", true));
    assert!(!local.is_live());
    assert!(!registry.remove("local", true));
    assert!(registry.reserve("replacement", ChannelKind::Local).is_ok());
}
#[test]
fn local_and_web_counts_are_independent_but_total_and_failure_are_absolute() {
    let mut registry = Registry::default();
    for n in 0..4 {
        registry
            .reserve(&n.to_string(), ChannelKind::Local)
            .unwrap();
    }
    for n in 4..8 {
        registry.reserve(&n.to_string(), ChannelKind::Web).unwrap();
    }
    assert!(registry.reserve("ninth", ChannelKind::Local).is_err());
    assert!(registry.remove("0", false));
    let witness = registry.reserve("web-fifth", ChannelKind::Web).unwrap();
    registry.fail();
    assert!(!witness.is_live());
    assert_eq!(registry.len(), 0);
    assert!(registry.reserve("after-fail", ChannelKind::Local).is_err());
}
#[test]
fn identities_and_refused_reservations_are_exact_without_tombstones() {
    let mut registry = Registry::default();
    for id in ["", "handoff"] {
        assert!(registry.reserve(id, ChannelKind::Local).is_err());
    }
    let first = registry.reserve("Exact", ChannelKind::Local).unwrap();
    assert!(registry.reserve("Exact", ChannelKind::Web).is_err());
    let distinct = registry.reserve("exact", ChannelKind::Web).unwrap();
    assert!(!registry.remove("foreign", false));
    assert!(registry.remove("Exact", false));
    assert!(!first.is_live());
    assert!(distinct.is_live());
    for n in 0..600 {
        let id = format!("closed-{n}");
        let witness = registry.reserve(&id, ChannelKind::Local).unwrap();
        assert!(registry.acknowledge(&id));
        assert!(registry.remove(&id, true));
        assert!(!witness.is_live());
        assert_eq!(registry.len(), 1);
    }
}
