//! The refusing adapter: no account database is read and no attribute is looked at.
//!
//! Both questions answer with the refusal, as an error. "No private group" would keep
//! the first caller closed too; "no ACL" would open the second. An error closes both.

use std::io;
use std::os::fd::BorrowedFd;

use super::refusal;

pub(super) fn private_group_of(_user: u32) -> io::Result<Option<u32>> {
    Err(refusal().into())
}

pub(super) fn has_access_acl(_file: BorrowedFd<'_>) -> io::Result<bool> {
    Err(refusal().into())
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsFd;

    use super::*;

    #[test]
    fn both_questions_are_refused_as_errors_and_nothing_is_granted() {
        // SAFETY: no arguments and no side effects; it cannot fail.
        let user = unsafe { libc::geteuid() };
        for user in [0, user, u32::MAX] {
            let error = private_group_of(user).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Unsupported);
            // What every caller makes of it: no private group, so no exception.
            assert_eq!(private_group_of(user).ok().flatten(), None);
        }
        let fixture = tempfile::tempdir().unwrap();
        let directory = std::fs::File::open(fixture.path()).unwrap();
        let error = has_access_acl(directory.as_fd()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        let text = error.to_string();
        assert!(text.contains("the group-write exception"), "{text}");
        assert!(text.contains(std::env::consts::OS), "{text}");
    }
}
