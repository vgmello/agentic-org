use super::*;
const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const GROUP: &str = "TEAMID1234.org.agentic.host";
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
    let a = Keychain::new(GROUP, A).unwrap();
    let b = Keychain::new(GROUP, "01BBBBBBBBBBBBBBBBBBBBBBBB").unwrap();
    for raw in ["registry", "recovery.A", "recovery.a"] {
        let n = Name::parse(raw).unwrap();
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
fn invalid_instance_is_rejected_without_keychain_io() {
    for invalid in [
        "",
        "../x",
        "01aaaaaaaaaaaaaaaaaaaaaaaa",
        "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
    ] {
        assert!(matches!(Keychain::new(GROUP, invalid), Err(Error::Unsafe)));
    }
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
        ..Keychain::new(group, id).unwrap()
    }
}
fn fixture() -> (Keychain, Arc<Fake>, Name) {
    let fake = Arc::new(Fake::default());
    (
        store(&fake, GROUP, A),
        fake,
        Name::parse("registry").unwrap(),
    )
}
fn bytes(raw: &[u8]) -> Value {
    Value::new(raw.to_vec()).unwrap()
}
fn loaded(store: &Keychain, n: &Name) -> Vec<u8> {
    store.load(n).unwrap().unwrap().expose().to_vec()
}
#[test]
fn four_call_adapter_insert_replace_delete_reopen_and_scope_conformance() {
    let (s, fake, n) = fixture();
    assert!(s.load(&n).unwrap().is_none());
    assert!(s.list().unwrap().is_empty());
    s.delete(&n).unwrap();
    s.put(&n, &bytes(b"first")).unwrap();
    assert_eq!(
        fake.0.lock().unwrap().calls,
        [Op::Copy, Op::Copy, Op::Delete, Op::Update, Op::Add]
    );
    s.put(&n, &bytes(b"replacement")).unwrap();
    assert_eq!(loaded(&s, &n), b"replacement");
    assert_eq!(loaded(&store(&fake, GROUP, A), &n), b"replacement");
    for isolated in [
        store(&fake, "OTHER.group", A),
        store(&fake, GROUP, "01BBBBBBBBBBBBBBBBBBBBBBBB"),
    ] {
        assert!(isolated.load(&n).unwrap().is_none());
        assert!(isolated.list().unwrap().is_empty());
        isolated.delete(&n).unwrap();
        isolated.put(&n, &bytes(b"isolated")).unwrap();
    }
    assert_eq!(loaded(&s, &n), b"replacement");
    s.delete(&n).unwrap();
    s.delete(&n).unwrap();
    assert!(s.load(&n).unwrap().is_none());
}
#[test]
fn update_add_race_retries_once_and_preserves_state_on_errors() {
    let (s, fake, n) = fixture();
    s.put(&n, &bytes(b"old")).unwrap();
    {
        let mut state = fake.0.lock().unwrap();
        state.calls.clear();
        state.faults.push_back((Op::Update, MISSING));
    }
    // Simulate another writer's item already existing when the missing-update/add race is observed.
    s.put(&n, &bytes(b"new")).unwrap();
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
            s.put(&n, &bytes(b"not committed")),
            Err(Error::Os(expected_error))
        );
        let state = fake.0.lock().unwrap();
        assert_eq!(state.calls, expected);
        assert_eq!(state.items, before);
        assert!(state.faults.is_empty());
    }
}
#[test]
fn tls_is_add_only_and_only_duplicate_is_success() {
    let (s, fake, _) = fixture();
    let n = Name::parse(&format!("tls.{}", "a".repeat(64))).unwrap();
    s.put(&n, &bytes(b"first")).unwrap();
    s.put(&n, &bytes(b"second")).unwrap();
    assert_eq!(fake.0.lock().unwrap().calls, [Op::Add, Op::Add]);
    assert_eq!(loaded(&s, &n), b"first");
    for code in [MISSING, -50] {
        let before = fake.0.lock().unwrap().items.clone();
        fake.0.lock().unwrap().faults.push_back((Op::Add, code));
        assert_eq!(s.put(&n, &bytes(b"third")), Err(Error::Os(code)));
        assert_eq!(fake.0.lock().unwrap().items, before);
    }
    s.delete(&n).unwrap();
    s.put(&n, &bytes(b"after explicit delete")).unwrap();
    assert_eq!(loaded(&s, &n), b"after explicit delete");
}
#[test]
fn values_bound_before_syscalls_and_on_returned_cfdata() {
    let (s, fake, n) = fixture();
    for len in [0, MAX_BYTES - 1, MAX_BYTES] {
        s.put(&n, &Value::new(vec![7; len]).unwrap()).unwrap();
        assert_eq!(loaded(&s, &n), vec![7; len]);
    }
    let before = fake.0.lock().unwrap().items.clone();
    fake.0.lock().unwrap().calls.clear();
    assert_eq!(
        s.put(&n, &Value(vec![7; MAX_BYTES + 1])),
        Err(Error::TooLarge)
    );
    assert!(fake.0.lock().unwrap().calls.is_empty());
    assert_eq!(fake.0.lock().unwrap().items, before);
    fake.0.lock().unwrap().reply = Some(Reply::Bytes(vec![7; MAX_BYTES + 1]));
    assert!(matches!(s.load(&n), Err(Error::Corrupt)));
    for reply in [Reply::Null, Reply::Wrong] {
        fake.0.lock().unwrap().reply = Some(reply);
        assert!(matches!(s.load(&n), Err(Error::Corrupt)));
    }
}
#[test]
fn lists_only_attributes_skips_invalid_names_and_keeps_exact_case() {
    let (s, fake, _) = fixture();
    for raw in ["recovery.A", "recovery.a"] {
        s.put(&Name::parse(raw).unwrap(), &bytes(raw.as_bytes()))
            .unwrap();
    }
    assert_eq!(
        s.list()
            .unwrap()
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>(),
        ["recovery.A", "recovery.a"]
    );
    fake.0.lock().unwrap().reply = Some(Reply::Accounts(vec![
        Some("registry".into()),
        None,
        Some("../invalid".into()),
        Some("registry".into()),
        Some("recovery.a".into()),
    ]));
    assert_eq!(
        s.list()
            .unwrap()
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>(),
        ["recovery.a", "registry"]
    );
    assert_eq!(fake.0.lock().unwrap().items.len(), 2);
    for reply in [Reply::Null, Reply::Wrong, Reply::BadArray] {
        fake.0.lock().unwrap().reply = Some(reply);
        assert!(matches!(s.list(), Err(Error::Corrupt)));
    }
    for raw in ["recovery.A", "recovery.a"] {
        let n = Name::parse(raw).unwrap();
        assert_eq!(loaded(&s, &n), raw.as_bytes());
        s.delete(&n).unwrap();
    }
    assert!(s.list().unwrap().is_empty());
}
#[test]
fn copy_and_delete_os_failures_are_never_missing_or_success() {
    let (s, fake, n) = fixture();
    s.put(&n, &bytes(b"private value")).unwrap();
    let before = fake.0.lock().unwrap().items.clone();
    for code in [-50, DUPLICATE] {
        fake.0.lock().unwrap().faults.push_back((Op::Copy, code));
        assert!(matches!(s.load(&n), Err(Error::Os(e)) if e == code));
        fake.0.lock().unwrap().faults.push_back((Op::Copy, code));
        assert_eq!(s.list(), Err(Error::Os(code)));
        fake.0.lock().unwrap().faults.push_back((Op::Delete, code));
        assert_eq!(s.delete(&n), Err(Error::Os(code)));
        assert_eq!(fake.0.lock().unwrap().items, before);
        assert_eq!(format!("{:?}", Error::Os(code)), format!("Os({code})"));
    }
}

#[test]
fn os_status_bridge_preserves_every_non_success_status() {
    assert_eq!(status(SUCCESS), Ok(()));
    for code in [MISSING, DUPLICATE, -50, 1] {
        assert_eq!(status(code), Err(code));
    }
}
