use std::fs;
use std::io::{self, Read};

use org_cli_shell::error::CliDiagnostic;

use super::{InvitationArgs, MAX_INVITATION_BYTES};

fn from_file(path: &std::path::Path) -> InvitationArgs {
    InvitationArgs {
        invitation_stdin: false,
        invitation_file: Some(path.display().to_string()),
    }
}

const FROM_STDIN: InvitationArgs = InvitationArgs {
    invitation_stdin: true,
    invitation_file: None,
};

/// An endless standard input: a reader that never ends.
struct Endless;

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        buf.fill(b'a');
        Ok(buf.len())
    }
}

#[test]
fn a_token_is_read_from_a_file_or_standard_input_and_trimmed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invite.txt");
    fs::write(&path, b"  tok-123\n").unwrap();
    let secret = from_file(&path).read_from(&mut io::empty()).unwrap();
    assert_eq!(secret.expose(), "tok-123");
    let secret = FROM_STDIN
        .read_from(&mut io::Cursor::new(b"tok-456\n".to_vec()))
        .unwrap();
    assert_eq!(secret.expose(), "tok-456");
    let exact = "t".repeat(usize::try_from(MAX_INVITATION_BYTES).unwrap());
    let secret = FROM_STDIN
        .read_from(&mut io::Cursor::new(exact.clone().into_bytes()))
        .unwrap();
    assert_eq!(secret.expose(), exact);
}

/// P1-4: an oversized file or an endless standard input is refused at the bound (exit 2),
/// never read whole.
#[test]
fn an_oversized_file_or_endless_standard_input_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("invite.txt");
    let over = vec![b't'; usize::try_from(MAX_INVITATION_BYTES).unwrap() + 1];
    fs::write(&path, &over).unwrap();
    let error = from_file(&path).read_from(&mut io::empty()).unwrap_err();
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("larger than 64 KiB"), "{error}");
    let error = FROM_STDIN.read_from(&mut Endless).unwrap_err();
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("larger than 64 KiB"), "{error}");
    let error = FROM_STDIN
        .read_from(&mut io::Cursor::new(over))
        .unwrap_err();
    assert!(error.to_string().contains("larger than 64 KiB"), "{error}");
}

/// P1-4: a symbolic link (live or dangling), a directory or a FIFO is refused without being
/// followed or blocked on; the token is never echoed.
#[test]
fn a_linked_or_special_invitation_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real.txt");
    fs::write(&target, b"secret-token").unwrap();
    let link = dir.path().join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let dangling = dir.path().join("dangling.txt");
    std::os::unix::fs::symlink(dir.path().join("missing"), &dangling).unwrap();
    let fifo = dir.path().join("fifo.txt");
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success());
    for path in [link, dangling, dir.path().to_path_buf(), fifo] {
        let error = from_file(&path).read_from(&mut io::empty()).unwrap_err();
        let shown = format!("{error} {error:?}");
        assert!(
            shown.contains("not a regular file"),
            "{}: {shown}",
            path.display()
        );
        assert!(!shown.contains("secret-token"), "{shown}");
        assert_eq!(error.exit_code(), 2);
    }
    let missing = from_file(&dir.path().join("missing"))
        .read_from(&mut io::empty())
        .unwrap_err();
    assert_eq!(missing.exit_code(), 1, "{missing}");
}
