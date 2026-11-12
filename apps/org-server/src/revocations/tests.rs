use super::*;

#[test]
fn revocations_are_exact_idempotent_retained_and_batch_overflow_is_atomic() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    let mut state = Revocations::default();
    state.apply_batch(vec![]).unwrap();
    assert!(state.0.is_empty());
    state
        .apply_batch(vec![
            "opaque:id".into(),
            "opaque:id".into(),
            "OPAQUE:id".into(),
        ])
        .unwrap();
    assert_eq!(state.0.len(), 2);
    assert!(state.0.contains("opaque:id") && state.0.contains("OPAQUE:id"));
    let rest: Vec<_> = (2..4095).map(|n| format!("id-{n}")).collect();
    state.apply_batch(rest).unwrap();
    assert_eq!(state.0.len(), 4095);
    state.apply_batch(vec!["id-4095".into()]).unwrap();
    assert_eq!(state.0.len(), 4096);
    let before = state.0.clone();
    assert!(state.apply_batch(vec!["adjacent-new".into()]).is_err());
    assert_eq!(state.0, before, "4097 must refuse atomically");
    state
        .apply_batch(
            before
                .iter()
                .cloned()
                .chain(before.iter().cloned())
                .collect(),
        )
        .unwrap();
    assert_eq!(state.0, before, "duplicates must not consume capacity");
    assert!(state
        .apply_batch(vec![
            "new-prefix".into(),
            "opaque:id".into(),
            "new-tail".into()
        ])
        .is_err());
    assert_eq!(
        state.0, before,
        "overflow must retain every old ID and apply no prefix"
    );
    state.apply_batch(vec![]).unwrap();
    assert_eq!(state.0, before, "no eviction or expiry before worker exit");
}
