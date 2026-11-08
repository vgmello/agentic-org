//! Resolving item references against the store.

use org_proto::ErrorCode;

use super::support::*;
use crate::{resolve_item, short_id, ulid_from_parts};

#[test]
fn resolves_ids_numbers_and_short_ids() {
    let w = World::new();
    let id = w.task();
    let second = w.task();
    let org = w.store.org.lock().unwrap().clone();
    let r = |text: &str, project: Option<&str>| {
        resolve_item(&org, w.store.as_ref(), text, project).unwrap()
    };
    assert_eq!(r(&id, None).unwrap().id, id);
    assert_eq!(r("chk-T-2", None).unwrap().id, second);
    assert_eq!(r("T-1", Some(PROJECT)).unwrap().id, id);
    assert_eq!(r("T-1", None).unwrap_err().code, ErrorCode::InvalidRequest);
    assert_eq!(r("chk-T-9", None).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(r("zzz-T-1", None).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(r("chk-B-1", None).unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(r(&short_id(&id), None).unwrap().id, id);
    assert_eq!(
        r("what?", None).unwrap_err().code,
        ErrorCode::InvalidRequest
    );

    // Two ids that end alike make the short id ambiguous.
    let twin = format!("{}{}", &ulid_from_parts(T0 + 5, 1)[..20], &id[20..]);
    let mut copy = w.item(&id);
    copy.id = twin.clone();
    w.store.work.lock().unwrap().items.insert(twin, copy);
    assert_eq!(
        r(&short_id(&id), None).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}
