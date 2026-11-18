//! The refusing admission adapter: nothing is started, nothing is checked and no peer is
//! identified.
//!
//! [`Admission`], [`Child`] and [`StaticImages`] have no value, so the methods that would
//! act on one cannot be reached. The functions that answer a question answer "no": no
//! peer satisfies a requirement and no signing information is read.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::unix::net::UnixStream;
#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::sync::Mutex;

use org_platform::Refused;

use super::{
    peer_refusal, refusal, ChildEnv, ExpectedImage, ImageRefusal, Peer, Signing, SigningRule,
    Started, StaticImageError, StaticSignature,
};

pub(super) enum Admission {}

impl Admission {
    pub(super) fn new(_expected: ExpectedImage) -> Result<Self, Refused> {
        Err(refusal())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn scripted(
        _expected: ExpectedImage,
        _script: super::test_support::Script,
    ) -> Result<Self, Refused> {
        Err(refusal())
    }

    pub(super) fn start(
        &self,
        _args: &[&OsStr],
        _env: &ChildEnv,
        _fence: &Mutex<()>,
    ) -> io::Result<Started> {
        match *self {}
    }

    pub(super) fn recheck(&self, _child: &Child) -> Result<(), ImageRefusal> {
        match *self {}
    }

    pub(super) fn readmit(&self, _child: &Child) -> Result<(), ImageRefusal> {
        match *self {}
    }

    pub(super) fn verify_reader(&self, _child: &Child, _stream: &UnixStream) -> io::Result<()> {
        match *self {}
    }
}

pub(super) enum Child {}

impl Child {
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn unowned(_pid: u32, _admitted: Option<Peer>) -> Result<Self, Refused> {
        Err(refusal())
    }

    pub(super) fn id(&self) -> u32 {
        match *self {}
    }

    pub(super) fn release(&self) -> io::Result<()> {
        match *self {}
    }

    pub(super) fn try_wait(&mut self) -> io::Result<bool> {
        match *self {}
    }

    pub(super) fn ended(&self) -> Option<String> {
        match *self {}
    }

    pub(super) fn kill(&mut self) {
        match *self {}
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn child_peer(_pid: u32) -> io::Result<Peer> {
    Err(peer_refusal().into())
}

pub(super) fn peer(_stream: &UnixStream) -> io::Result<Peer> {
    Err(peer_refusal().into())
}

pub(super) fn native_peers() -> Result<(), Refused> {
    Err(peer_refusal())
}

pub(super) fn peer_satisfies(_peer: &Peer, _requirement: &str, _rule: &SigningRule) -> bool {
    false
}

#[cfg(debug_assertions)]
pub(super) fn peer_executable(_stream: &UnixStream) -> io::Result<PathBuf> {
    Err(peer_refusal().into())
}

pub(super) fn own_signing() -> Option<Signing> {
    None
}

pub(super) enum StaticImages {}

impl StaticImages {
    pub(super) fn new() -> Result<Self, Refused> {
        Err(refusal())
    }

    pub(super) fn signature(
        &self,
        _file: &File,
        _requirement: &str,
    ) -> Result<StaticSignature, StaticImageError> {
        match *self {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A rule that would let any signed code through, were there any.
    const ANY: SigningRule = SigningRule {
        required_flags: 0,
        rejected_entitlements: &[],
        absent_entitlements: &[],
    };

    fn development() -> ExpectedImage {
        ExpectedImage {
            program: PathBuf::from("/bin/sh"),
            sha256: None,
            macos_cdhash: None,
            macos_requirement: None,
            macos_signing: ANY,
            development: true,
        }
    }

    fn assert_refused<T>(result: io::Result<T>, refusal: Refused) {
        let Err(error) = result else {
            panic!("the refusing adapter succeeded");
        };
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert_eq!(
            error
                .get_ref()
                .and_then(|source| source.downcast_ref::<Refused>()),
            Some(&refusal)
        );
    }

    #[test]
    fn no_admission_exists_so_nothing_is_started_or_checked() {
        let Refused::Unsupported { capability, os } = Admission::new(development())
            .err()
            .expect("the refusing adapter made an admission");
        assert_eq!(capability, "process admission");
        assert_eq!(os, std::env::consts::OS);
        // Not for the least demanding image, a scripted check or a process the test names.
        assert_eq!(
            Admission::scripted(development(), Arc::new(|_, _| Ok(None))).err(),
            Some(refusal())
        );
        assert_eq!(
            Child::unowned(std::process::id(), None).err(),
            Some(refusal())
        );
        assert_eq!(StaticImages::new().err(), Some(refusal()));
    }

    #[test]
    fn no_peer_is_identified_and_none_satisfies_anything() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_refused(peer(&a), peer_refusal());
        assert_refused(child_peer(std::process::id()), peer_refusal());
        #[cfg(debug_assertions)]
        assert_refused(peer_executable(&a), peer_refusal());
        let Refused::Unsupported { capability, os } = native_peers().unwrap_err();
        assert_eq!(capability, "peer identity");
        assert_eq!(os, std::env::consts::OS);
        for token in [[0; 32], [7; 32]] {
            let peer = Peer {
                macos_audit_token: token,
            };
            assert!(!peer_satisfies(&peer, "anchor apple", &ANY));
        }
    }

    #[test]
    fn no_signing_information_is_read() {
        assert!(own_signing().is_none());
    }
}
