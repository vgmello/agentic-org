//! The Keychain adapter's tests. None touches the Keychain: they read the attributes the
//! adapter builds, and drive its operations through [`Calls`] with a recording fake. The
//! service and account strings are the host's two views' (office keys and host secrets), so
//! that what each of them sent before it moved here is what is checked here.
use super::*;
const A: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e0";
const GROUP: &str = "TEAMID1234.org.agentic.host";
/// The office keys' service (the host's `keys::SERVICE`).
const OFFICE_KEYS: &str = "org.agentic.office-key";
/// The service of an instance's host secrets, as the host composes it.
fn host_secrets(instance: &str) -> String {
    format!("org.agentic.host-secret.instance.{instance}")
}
fn keychain(group: &str, service: &str) -> Keychain {
    Keychain::new(&KeychainItems {
        access_group: group,
        service,
    })
}
fn name(raw: &str) -> ItemName {
    ItemName::new(raw).unwrap()
}
/// Every listed name, with no rule of the consumer's.
fn names(store: &Keychain) -> Vec<String> {
    store
        .list(&|_| true)
        .unwrap()
        .names
        .iter()
        .map(|name| name.as_str().to_owned())
        .collect()
}
fn value(attrs: &Attributes, key: CFStringRef) -> CFType {
    // SAFETY: tests supply immutable framework constants.
    let key = unsafe { CFString::wrap_under_get_rule(key) };
    let found: Vec<_> = attrs.iter().filter(|(k, _)| *k == key).collect();
    assert_eq!(found.len(), 1);
    found[0].1.clone()
}
fn string(value: CFType) -> String {
    value.downcast::<CFString>().unwrap().to_string()
}
#[test]
fn exact_instance_service_accounts_group_and_data_protection() {
    let a = keychain(GROUP, &host_secrets(A));
    let b = keychain(GROUP, &host_secrets("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e1"));
    for raw in ["registry", "recovery.A", "recovery.a"] {
        let n = name(raw);
        let q = a.query(Some(&n));
        assert_eq!(q.len(), 5);
        // SAFETY: each constant is immutable and exported by Security.framework.
        unsafe {
            assert_eq!(
                string(value(&q, kSecAttrService)),
                format!("org.agentic.host-secret.instance.{A}")
            );
            assert_eq!(string(value(&q, kSecAttrAccount)), raw);
            assert_eq!(string(value(&q, kSecAttrAccessGroup)), GROUP);
            assert_eq!(
                string(value(&q, kSecClass)),
                CFString::wrap_under_get_rule(kSecClassGenericPassword).to_string()
            );
            assert_eq!(
                value(&q, kSecUseDataProtectionKeychain),
                CFBoolean::true_value().into_CFType()
            );
            assert_ne!(
                value(&q, kSecAttrService),
                value(&b.query(Some(&n)), kSecAttrService)
            );
        }
    }
}
#[test]
fn write_policy_matches_office_key_access_class() {
    let attrs = Keychain::add_policy();
    assert_eq!(attrs.len(), 2);
    // SAFETY: each constant is immutable and exported by Security.framework.
    unsafe {
        assert_eq!(
            string(value(&attrs, kSecAttrAccessible)),
            CFString::wrap_under_get_rule(kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly)
                .to_string()
        );
        assert_eq!(
            value(&attrs, kSecAttrSynchronizable),
            CFBoolean::false_value().into_CFType()
        );
    }
}
#[test]
fn instance_queries_preserve_the_office_account_and_scope_every_service() {
    let a = keychain(GROUP, &format!("{OFFICE_KEYS}.instance.INSTANCEA"));
    let b = keychain(GROUP, &format!("{OFFICE_KEYS}.instance.INSTANCEB"));
    let query_a = a.query(Some(&name(A)));
    let query_b = b.query(Some(&name(A)));
    // SAFETY: immutable Security.framework CFString constants.
    unsafe {
        assert_eq!(string(value(&query_a, kSecAttrAccount)), A);
        assert_eq!(string(value(&query_b, kSecAttrAccount)), A);
        assert_eq!(
            string(value(&query_a, kSecAttrService)),
            format!("{OFFICE_KEYS}.instance.INSTANCEA")
        );
        assert_eq!(
            string(value(&query_b, kSecAttrService)),
            format!("{OFFICE_KEYS}.instance.INSTANCEB")
        );
    }
}
/// What an add sends without its data: the query and the write policy.
fn item_attributes(store: &Keychain, account: &str) -> Attributes {
    let mut attributes = store.query(Some(&name(account)));
    attributes.extend(Keychain::add_policy());
    attributes
}
// SAFETY (each `unsafe` below): reads an immutable framework constant.
#[test]
fn an_item_is_a_generic_password_for_this_office_in_the_data_protection_keychain() {
    let attributes = item_attributes(&keychain(GROUP, OFFICE_KEYS), A);
    assert_eq!(attributes.len(), 7);
    let class = value(&attributes, unsafe { kSecClass });
    assert_eq!(string(class), unsafe {
        CFString::wrap_under_get_rule(kSecClassGenericPassword).to_string()
    });
    assert_eq!(
        string(value(&attributes, unsafe { kSecAttrService })),
        OFFICE_KEYS
    );
    assert_eq!(string(value(&attributes, unsafe { kSecAttrAccount })), A);
    assert_eq!(
        string(value(&attributes, unsafe { kSecAttrAccessGroup })),
        GROUP
    );
    assert_eq!(
        value(&attributes, unsafe { kSecUseDataProtectionKeychain }),
        CFBoolean::true_value().into_CFType()
    );
}
#[test]
fn an_item_stays_on_this_mac_and_is_readable_after_the_first_unlock() {
    let attributes = item_attributes(&keychain(GROUP, OFFICE_KEYS), A);
    let accessible = value(&attributes, unsafe { kSecAttrAccessible });
    assert_eq!(string(accessible), unsafe {
        CFString::wrap_under_get_rule(kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly).to_string()
    });
    assert_eq!(
        value(&attributes, unsafe { kSecAttrSynchronizable }),
        CFBoolean::false_value().into_CFType()
    );
}
/// L3d: `load` and `delete` look the item up by its class, service and account, in the Data
/// Protection keychain and the access group, and nothing else (no data, no accessibility).
/// The fake below refuses a load or a delete whose query is anything else.
#[test]
fn the_load_and_delete_query_is_this_offices_item_in_the_group() {
    let query = keychain(GROUP, OFFICE_KEYS).query(Some(&name(A)));
    assert_eq!(query.len(), 5);
    let class = value(&query, unsafe { kSecClass });
    assert_eq!(string(class), unsafe {
        CFString::wrap_under_get_rule(kSecClassGenericPassword).to_string()
    });
    assert_eq!(
        string(value(&query, unsafe { kSecAttrService })),
        OFFICE_KEYS
    );
    assert_eq!(string(value(&query, unsafe { kSecAttrAccount })), A);
    assert_eq!(string(value(&query, unsafe { kSecAttrAccessGroup })), GROUP);
    assert_eq!(
        value(&query, unsafe { kSecUseDataProtectionKeychain }),
        CFBoolean::true_value().into_CFType()
    );
    // SAFETY: immutable framework constants.
    let (accessible, synchronizable) = unsafe {
        (
            CFString::wrap_under_get_rule(kSecAttrAccessible),
            CFString::wrap_under_get_rule(kSecAttrSynchronizable),
        )
    };
    assert!(query
        .iter()
        .all(|(key, _)| *key != accessible && *key != synchronizable));
}
#[test]
fn the_attributes_never_carry_a_key() {
    let store = keychain(GROUP, OFFICE_KEYS);
    // SAFETY: an immutable framework constant.
    let data = unsafe { CFString::wrap_under_get_rule(kSecValueData) };
    assert!(item_attributes(&store, A)
        .iter()
        .all(|(key, _)| *key != data));
    // A write's three dictionaries: only the change and the add carry the value.
    let (query, changes, add) = store.write(&name(A), &[7; 32]);
    assert_eq!((query.len(), changes.len(), add.len()), (5, 1, 8));
    assert!(query.find(&data).is_none());
    assert!(changes.find(&data).is_some());
    assert!(add.find(&data).is_some());
}

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Add,
    Update,
    Copy,
    Delete,
}
#[derive(Default)]
struct State {
    items: BTreeMap<(String, String, String), Vec<u8>>,
    calls: Vec<Op>,
    faults: VecDeque<(Op, i32)>,
    reply: Option<Reply>,
}
enum Reply {
    Null,
    Wrong,
    Bytes(Vec<u8>),
    Accounts(Vec<Option<String>>),
    BadArray,
}
#[derive(Default)]
struct Fake(Mutex<State>);
fn attr(d: &Dictionary, k: CFStringRef) -> Option<CFType> {
    // SAFETY: callers use immutable Security.framework constants.
    d.find(unsafe { CFString::wrap_under_get_rule(k) })
        .map(|v| (*v).clone())
}
fn text(d: &Dictionary, k: CFStringRef) -> String {
    string(attr(d, k).unwrap())
}
fn data(d: &Dictionary) -> Vec<u8> {
    // SAFETY: immutable Security.framework constant.
    unsafe {
        attr(d, kSecValueData)
            .unwrap()
            .downcast::<CFData>()
            .unwrap()
            .bytes()
            .to_vec()
    }
}
fn scope(d: &Dictionary) -> (String, String) {
    // SAFETY: immutable Security.framework constants.
    unsafe {
        assert_eq!(
            attr(d, kSecClass).unwrap(),
            CFString::wrap_under_get_rule(kSecClassGenericPassword).into_CFType()
        );
        assert_eq!(
            attr(d, kSecUseDataProtectionKeychain).unwrap(),
            CFBoolean::true_value().into_CFType()
        );
        assert!(attr(d, kSecAttrAccessible).is_none());
        assert!(attr(d, kSecAttrSynchronizable).is_none());
        (text(d, kSecAttrService), text(d, kSecAttrAccessGroup))
    }
}
fn key(d: &Dictionary) -> (String, String, String) {
    let (service, group) = scope(d);
    // SAFETY: immutable Security.framework constant.
    (service, group, unsafe { text(d, kSecAttrAccount) })
}
fn record(state: &mut State, op: Op) -> Result<(), i32> {
    state.calls.push(op);
    if state.faults.front().is_some_and(|(at, _)| *at == op) {
        return Err(state.faults.pop_front().unwrap().1);
    }
    Ok(())
}
fn accounts(names: Vec<Option<String>>) -> CFType {
    let items: Vec<CFType> = names
        .into_iter()
        .map(|name| {
            let attrs = name
                .map(|name| {
                    // SAFETY: immutable Security.framework constant.
                    (
                        unsafe { CFString::wrap_under_get_rule(kSecAttrAccount) },
                        CFString::new(&name).into_CFType(),
                    )
                })
                .into_iter()
                .collect();
            dictionary(attrs).into_CFType()
        })
        .collect();
    CFArray::from_CFTypes(&items).into_CFType()
}
impl Calls for Fake {
    fn add(&self, attrs: &Dictionary) -> Result<(), i32> {
        assert_eq!(attrs.len(), 8);
        // SAFETY: immutable Security.framework constants.
        let (key, bytes) = unsafe {
            assert_eq!(
                attr(attrs, kSecAttrAccessible).unwrap(),
                CFString::wrap_under_get_rule(kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly)
                    .into_CFType()
            );
            assert_eq!(
                attr(attrs, kSecAttrSynchronizable).unwrap(),
                CFBoolean::false_value().into_CFType()
            );
            assert_eq!(
                attr(attrs, kSecClass).unwrap(),
                CFString::wrap_under_get_rule(kSecClassGenericPassword).into_CFType()
            );
            assert_eq!(
                attr(attrs, kSecUseDataProtectionKeychain).unwrap(),
                CFBoolean::true_value().into_CFType()
            );
            (
                (
                    text(attrs, kSecAttrService),
                    text(attrs, kSecAttrAccessGroup),
                    text(attrs, kSecAttrAccount),
                ),
                data(attrs),
            )
        };
        let mut state = self.0.lock().unwrap();
        record(&mut state, Op::Add)?;
        if state.items.contains_key(&key) {
            return Err(DUPLICATE);
        }
        state.items.insert(key, bytes);
        Ok(())
    }
    fn update(&self, query: &Dictionary, changes: &Dictionary) -> Result<(), i32> {
        assert_eq!(query.len(), 5);
        assert_eq!(
            changes.len(),
            1,
            "updates change data only, preserving custody policy"
        );
        let key = key(query);
        let bytes = data(changes);
        let mut state = self.0.lock().unwrap();
        record(&mut state, Op::Update)?;
        let item = state.items.get_mut(&key).ok_or(MISSING)?;
        *item = bytes;
        Ok(())
    }
    fn copy_matching(&self, query: &Dictionary) -> Result<Option<CFType>, i32> {
        let (service, group) = scope(query);
        // SAFETY: immutable Security.framework constants.
        let list = unsafe { attr(query, kSecReturnAttributes).is_some() };
        unsafe {
            assert_eq!(query.len(), 6);
            if list {
                assert_eq!(
                    attr(query, kSecReturnAttributes).unwrap(),
                    CFBoolean::true_value().into_CFType()
                );
                assert_eq!(
                    attr(query, kSecMatchLimit).unwrap(),
                    CFString::wrap_under_get_rule(kSecMatchLimitAll).into_CFType()
                );
                assert!(attr(query, kSecReturnData).is_none());
                assert!(attr(query, kSecAttrAccount).is_none());
            } else {
                assert_eq!(
                    attr(query, kSecReturnData).unwrap(),
                    CFBoolean::true_value().into_CFType()
                );
                assert!(attr(query, kSecMatchLimit).is_none());
            }
        }
        let mut state = self.0.lock().unwrap();
        record(&mut state, Op::Copy)?;
        if let Some(reply) = state.reply.take() {
            return Ok(match reply {
                Reply::Null => None,
                Reply::Wrong => Some(CFString::new("not data or an array").into_CFType()),
                Reply::Bytes(bytes) => Some(CFData::from_buffer(&bytes).into_CFType()),
                Reply::Accounts(names) => Some(accounts(names)),
                Reply::BadArray => Some(
                    CFArray::from_CFTypes(&[CFString::new("not a dictionary").into_CFType()])
                        .into_CFType(),
                ),
            });
        }
        if list {
            let names: Vec<_> = state
                .items
                .keys()
                .filter(|(s, g, _)| s == &service && g == &group)
                .map(|(_, _, name)| Some(name.clone()))
                .collect();
            if names.is_empty() {
                Err(MISSING)
            } else {
                Ok(Some(accounts(names)))
            }
        } else {
            // SAFETY: immutable Security.framework constant.
            let name = unsafe { text(query, kSecAttrAccount) };
            state
                .items
                .get(&(service, group, name))
                .map(|bytes| Some(CFData::from_buffer(bytes).into_CFType()))
                .ok_or(MISSING)
        }
    }
    fn delete(&self, query: &Dictionary) -> Result<(), i32> {
        assert_eq!(query.len(), 5);
        let key = key(query);
        let mut state = self.0.lock().unwrap();
        record(&mut state, Op::Delete)?;
        state.items.remove(&key).map(|_| ()).ok_or(MISSING)
    }
}
fn store(fake: &Arc<Fake>, group: &str, id: &str) -> Keychain {
    Keychain {
        calls: fake.clone(),
        ..keychain(group, &host_secrets(id))
    }
}
fn fixture() -> (Keychain, Arc<Fake>, ItemName) {
    let fake = Arc::new(Fake::default());
    (store(&fake, GROUP, A), fake, name("registry"))
}
fn loaded(store: &Keychain, n: &ItemName) -> Vec<u8> {
    store.load(n).unwrap().unwrap().expose().to_vec()
}
#[test]
fn four_call_adapter_insert_replace_delete_reopen_and_scope_conformance() {
    let (s, fake, n) = fixture();
    assert!(s.load(&n).unwrap().is_none());
    assert!(names(&s).is_empty());
    s.delete(&n).unwrap();
    s.replace(&n, b"first").unwrap();
    assert_eq!(
        fake.0.lock().unwrap().calls,
        [Op::Copy, Op::Copy, Op::Delete, Op::Update, Op::Add]
    );
    s.replace(&n, b"replacement").unwrap();
    assert_eq!(loaded(&s, &n), b"replacement");
    assert_eq!(loaded(&store(&fake, GROUP, A), &n), b"replacement");
    for isolated in [
        store(&fake, "OTHER.group", A),
        store(&fake, GROUP, "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e1"),
    ] {
        assert!(isolated.load(&n).unwrap().is_none());
        assert!(names(&isolated).is_empty());
        isolated.delete(&n).unwrap();
        isolated.replace(&n, b"isolated").unwrap();
    }
    assert_eq!(loaded(&s, &n), b"replacement");
    s.delete(&n).unwrap();
    s.delete(&n).unwrap();
    assert!(s.load(&n).unwrap().is_none());
}
#[test]
fn update_add_race_retries_once_and_preserves_state_on_errors() {
    let (s, fake, n) = fixture();
    s.replace(&n, b"old").unwrap();
    {
        let mut state = fake.0.lock().unwrap();
        state.calls.clear();
        state.faults.push_back((Op::Update, MISSING));
    }
    // Simulate another writer's item already existing when the missing-update/add race is observed.
    s.replace(&n, b"new").unwrap();
    assert_eq!(
        fake.0.lock().unwrap().calls,
        [Op::Update, Op::Add, Op::Update]
    );
    assert_eq!(loaded(&s, &n), b"new");
    for (faults, expected) in [
        (vec![(Op::Update, -50)], vec![Op::Update]),
        (
            vec![(Op::Update, MISSING), (Op::Add, -50)],
            vec![Op::Update, Op::Add],
        ),
        (
            vec![
                (Op::Update, MISSING),
                (Op::Add, DUPLICATE),
                (Op::Update, MISSING),
            ],
            vec![Op::Update, Op::Add, Op::Update],
        ),
        (
            vec![
                (Op::Update, MISSING),
                (Op::Add, DUPLICATE),
                (Op::Update, -50),
            ],
            vec![Op::Update, Op::Add, Op::Update],
        ),
    ] {
        let mut state = fake.0.lock().unwrap();
        state.faults = faults.into();
        state.calls.clear();
        let before = state.items.clone();
        drop(state);
        let expected_error =
            if expected.len() == 3 && fake.0.lock().unwrap().faults.back().unwrap().1 == MISSING {
                MISSING
            } else {
                -50
            };
        assert_eq!(
            s.replace(&n, b"not committed"),
            Err(CredentialError::Os(expected_error))
        );
        let state = fake.0.lock().unwrap();
        assert_eq!(state.calls, expected);
        assert_eq!(state.items, before);
        assert!(state.faults.is_empty());
    }
}
/// The host's add-only names (`tls.*`, `writer-device`) and every office key go through
/// `add`: one `SecItemAdd` and nothing else. A duplicate is `Exists` and keeps the first value
/// (the host-secrets view then reports success; the office-keys view reports `Exists`).
#[test]
fn an_add_is_one_add_and_only_a_duplicate_is_exists() {
    let (s, fake, _) = fixture();
    let n = name(&format!("tls.{}", "a".repeat(64)));
    s.add(&n, b"first").unwrap();
    assert_eq!(s.add(&n, b"second"), Err(CredentialError::Exists));
    assert_eq!(fake.0.lock().unwrap().calls, [Op::Add, Op::Add]);
    assert_eq!(loaded(&s, &n), b"first");
    for code in [MISSING, -50] {
        let before = fake.0.lock().unwrap().items.clone();
        fake.0.lock().unwrap().faults.push_back((Op::Add, code));
        assert_eq!(s.add(&n, b"third"), Err(CredentialError::Os(code)));
        assert_eq!(fake.0.lock().unwrap().items, before);
    }
    s.delete(&n).unwrap();
    s.add(&n, b"after explicit delete").unwrap();
    assert_eq!(loaded(&s, &n), b"after explicit delete");
}
/// An office key: created with a direct `SecItemAdd`, never updated.
#[test]
fn an_office_key_is_added_under_its_office_id_and_never_updated() {
    let fake = Arc::new(Fake::default());
    let s = Keychain {
        calls: fake.clone(),
        ..keychain(GROUP, OFFICE_KEYS)
    };
    let office = name(A);
    s.add(&office, &[1; 32]).unwrap();
    assert_eq!(s.add(&office, &[2; 32]), Err(CredentialError::Exists));
    assert_eq!(loaded(&s, &office), [1; 32]);
    assert_eq!(names(&s), [A]);
    s.delete(&office).unwrap();
    s.delete(&office).unwrap();
    assert!(s.load(&office).unwrap().is_none());
    let state = fake.0.lock().unwrap();
    assert!(!state.calls.contains(&Op::Update));
    assert_eq!(
        state.calls,
        [
            Op::Add,
            Op::Add,
            Op::Copy,
            Op::Copy,
            Op::Delete,
            Op::Delete,
            Op::Copy
        ]
    );
    assert!(state.items.is_empty());
}
#[test]
fn values_bound_before_syscalls_and_on_returned_cfdata() {
    let (s, fake, n) = fixture();
    for len in [0, MAX_ITEM_BYTES - 1, MAX_ITEM_BYTES] {
        s.replace(&n, &vec![7; len]).unwrap();
        assert_eq!(loaded(&s, &n), vec![7; len]);
    }
    let before = fake.0.lock().unwrap().items.clone();
    fake.0.lock().unwrap().calls.clear();
    assert_eq!(
        s.replace(&n, &vec![7; MAX_ITEM_BYTES + 1]),
        Err(CredentialError::TooLarge)
    );
    assert_eq!(
        s.add(&name("another"), &vec![7; MAX_ITEM_BYTES + 1]),
        Err(CredentialError::TooLarge)
    );
    assert!(fake.0.lock().unwrap().calls.is_empty());
    assert_eq!(fake.0.lock().unwrap().items, before);
    fake.0.lock().unwrap().reply = Some(Reply::Bytes(vec![7; MAX_ITEM_BYTES + 1]));
    assert!(matches!(s.load(&n), Err(CredentialError::Corrupt)));
    // A reply that is not data is not an item's value, and not a missing item either.
    for reply in [Reply::Null, Reply::Wrong] {
        fake.0.lock().unwrap().reply = Some(reply);
        assert!(matches!(s.load(&n), Err(CredentialError::Malformed)));
    }
}
#[test]
fn lists_only_attributes_skips_invalid_names_and_keeps_exact_case() {
    let (s, fake, _) = fixture();
    for raw in ["recovery.A", "recovery.a"] {
        s.replace(&name(raw), raw.as_bytes()).unwrap();
    }
    assert_eq!(names(&s), ["recovery.A", "recovery.a"]);
    fake.0.lock().unwrap().reply = Some(Reply::Accounts(vec![
        Some("registry".into()),
        None,
        Some("../invalid".into()),
        Some("registry".into()),
        Some("recovery.a".into()),
    ]));
    // An item without an account, and an account that is no item name, are counted and left
    // out; a name listed twice is listed once.
    let listing = s.list(&|_| true).unwrap();
    assert_eq!(
        listing
            .names
            .iter()
            .map(ItemName::as_str)
            .collect::<Vec<_>>(),
        ["recovery.a", "registry"]
    );
    assert_eq!(listing.ignored, 2);
    assert_eq!(fake.0.lock().unwrap().items.len(), 2);
    // The consumer's own name rule leaves out, and counts, what it refuses.
    fake.0.lock().unwrap().reply = Some(Reply::Accounts(vec![
        Some("registry".into()),
        Some("recovery.a".into()),
        Some("Registry".into()),
    ]));
    let listing = s.list(&|name| name.starts_with("re")).unwrap();
    assert_eq!(
        listing
            .names
            .iter()
            .map(ItemName::as_str)
            .collect::<Vec<_>>(),
        ["recovery.a", "registry"]
    );
    assert_eq!(listing.ignored, 1);
    for reply in [Reply::Null, Reply::Wrong, Reply::BadArray] {
        fake.0.lock().unwrap().reply = Some(reply);
        assert!(matches!(s.list(&|_| true), Err(CredentialError::Malformed)));
    }
    for raw in ["recovery.A", "recovery.a"] {
        let n = name(raw);
        assert_eq!(loaded(&s, &n), raw.as_bytes());
        s.delete(&n).unwrap();
    }
    assert!(names(&s).is_empty());
}
#[test]
fn copy_and_delete_os_failures_are_never_missing_or_success() {
    let (s, fake, n) = fixture();
    s.replace(&n, b"private value").unwrap();
    let before = fake.0.lock().unwrap().items.clone();
    for code in [-50, DUPLICATE] {
        fake.0.lock().unwrap().faults.push_back((Op::Copy, code));
        assert!(matches!(s.load(&n), Err(CredentialError::Os(e)) if e == code));
        fake.0.lock().unwrap().faults.push_back((Op::Copy, code));
        assert_eq!(s.list(&|_| true), Err(CredentialError::Os(code)));
        fake.0.lock().unwrap().faults.push_back((Op::Delete, code));
        assert_eq!(s.delete(&n), Err(CredentialError::Os(code)));
        assert_eq!(fake.0.lock().unwrap().items, before);
        assert_eq!(
            format!("{:?}", CredentialError::Os(code)),
            format!("Os({code})")
        );
    }
}

#[test]
fn os_status_bridge_preserves_every_non_success_status() {
    assert_eq!(status(SUCCESS), Ok(()));
    for code in [MISSING, DUPLICATE, -50, 1] {
        assert_eq!(status(code), Err(code));
    }
}

#[test]
fn the_keychain_is_this_systems_protected_store_and_opening_it_calls_nothing() {
    assert_eq!(protected_store(), Ok(ProtectedStore::Keychain));
    let opened = open(CredentialLocation::Keychain(KeychainItems {
        access_group: GROUP,
        service: OFFICE_KEYS,
    }));
    // Construction does not call SecItem* or create any item.
    assert!(matches!(opened, Ok(Store::Keychain(_))));
}

/// A release build on macOS keeps its secrets in the Keychain: a release directory is
/// refused before its location is read, and nothing is opened, locked or made.
#[test]
fn a_release_directory_is_refused_and_nothing_is_made() {
    let fixture = tempfile::tempdir().unwrap();
    let dir = fixture.path().canonicalize().unwrap();
    let data_root = File::open(&dir).unwrap();
    for (path, mount_point_required, expect_items) in [
        (dir.join("store"), false, false),
        (dir.clone(), true, true),
        (std::path::PathBuf::from("relative/../path"), false, true),
    ] {
        let location = ReleaseDirectory {
            path: &path,
            data_root: std::os::fd::AsFd::as_fd(&data_root),
            mount_point_required,
            expect_items,
        };
        let stores = [ItemStore {
            directory: "office-keys",
            suffix: ".key",
            is_item: |_| true,
        }];
        for item_stores in [&stores[..], &[]] {
            let refused = release_directory(&location, item_stores, &[]);
            assert_eq!(refused.err(), Some(CredentialError::Refused(refusal())));
        }
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}
