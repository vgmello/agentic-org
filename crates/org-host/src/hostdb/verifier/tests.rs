use super::*;
use crate::hostdb::tests::first_account::PHC;
use argon2::password_hash::{Output, SaltString};

thread_local! { static CLEARED: std::cell::RefCell<Vec<Vec<u8>>> = const { std::cell::RefCell::new(Vec::new()) }; }
pub(super) fn observe_clear(bytes: &[u8]) {
    CLEARED.with(|slot| slot.borrow_mut().push(bytes.to_vec()));
}
fn changed(index: usize, value: &str) -> String {
    let mut parts: Vec<_> = PHC.split('$').collect();
    parts[index] = value;
    parts.join("$")
}

#[test]
fn only_the_pinned_argon2_profile_salt_and_tag_are_accepted() {
    assert_eq!(Verifier::new(PHC.into()).unwrap().expose(), PHC);
    let mut invalid = vec![
        String::new(),
        "not a PHC secret".into(),
        changed(1, "argon2i"),
        changed(1, "argon2d"),
        changed(2, "v=16"),
        PHC.replace("$v=19", ""),
        changed(3, "m=65535,t=3,p=1"),
        changed(3, "m=65537,t=3,p=1"),
        changed(3, "m=65536,t=2,p=1"),
        changed(3, "m=65536,t=4,p=1"),
        changed(3, "m=65536,t=3,p=2"),
        changed(3, "m=65536,t=3"),
        changed(3, "t=3,p=1"),
        changed(3, "m=65536,p=1"),
        changed(3, "m=65536,t=3,p=1,x=1"),
        changed(4, "not*base64"),
        changed(4, "MDEyMzQ1Njc4OWFiY2RlZh"),
        PHC.rsplit_once('$').unwrap().0.into(),
        changed(5, "not*base64"),
        changed(5, ""),
    ];
    for length in [15, 17] {
        invalid.push(changed(
            4,
            SaltString::encode_b64(&vec![0; length]).unwrap().as_str(),
        ));
    }
    for length in [31, 33] {
        invalid.push(changed(
            5,
            &Output::new(&vec![0; length]).unwrap().to_string(),
        ));
    }
    let rejected = invalid.len();
    CLEARED.with(|slot| slot.borrow_mut().clear());
    for phc in invalid {
        assert!(matches!(Verifier::new(phc), Err(Error::Unsafe)));
    }
    CLEARED.with(|slot| {
        let values = slot.borrow();
        assert_eq!(values.len(), rejected);
        assert!(values.iter().all(|bytes| bytes.iter().all(|b| *b == 0)));
    });
    // Parameter order is representation, not a separate credential policy.
    assert!(Verifier::new(changed(3, "p=1,t=3,m=65536")).is_ok());
}

#[test]
fn success_parse_profile_and_identity_rejections_zero_owned_inputs_before_freeing() {
    CLEARED.with(|slot| slot.borrow_mut().clear());
    let input = Verifier::new(PHC.into()).unwrap();
    assert!(CLEARED.with(|slot| slot.borrow().is_empty()));
    drop(input);
    assert!(Verifier::new("sensitive invalid PHC".into()).is_err());
    assert!(Verifier::new(changed(1, "argon2i")).is_err());
    let value = Verifier::new(PHC.into()).unwrap();
    assert!(crate::hostdb::FirstAccount::new("invalid", "Sage", value).is_err());
    let value = Verifier::new(PHC.into()).unwrap();
    assert!(crate::hostdb::FirstAccount::new("01J00000000000000000000003", "", value).is_err());
    CLEARED.with(|slot| {
        let bytes = slot.borrow();
        assert_eq!(bytes.len(), 5);
        assert_eq!(bytes[0].len(), PHC.len());
        assert_eq!(bytes[1].len(), "sensitive invalid PHC".len());
        assert!(bytes
            .iter()
            .all(|value| !value.is_empty() && value.iter().all(|b| *b == 0)));
    });
}
