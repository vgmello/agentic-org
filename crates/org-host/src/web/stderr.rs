//! The web process's standard error: copied to the host log, with its refusal, confinement
//! and renewal reasons recognized for the host.

use std::{
    io::{self, Read},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

use org_platform_host::admission::Child;

use crate::sync::lock;

use super::control::{exited, failed, ControlEof};

/// What one instance's web processes wrote to stderr, for a test.
#[cfg(test)]
#[derive(Clone, Default)]
pub(super) struct Captured {
    pub(super) bytes: Vec<u8>,
    /// Readers that have not reached the end of their pipe yet.
    reading: usize,
    /// Why a reader stopped before the end of its pipe.
    failed: Option<String>,
}

/// What each instance's web processes wrote to stderr. The host keeps none of it; a
/// real-process test reads it to prove what the web process never prints.
#[cfg(test)]
pub(super) static CAPTURED_STDERR: Mutex<std::collections::BTreeMap<String, Captured>> =
    Mutex::new(std::collections::BTreeMap::new());

/// Everything `instance`'s web processes wrote to stderr, once every reader has read its
/// pipe to the end. A reader that failed, or one still reading after ten seconds, fails
/// the test: a partial capture would prove nothing about what was never printed.
#[cfg(test)]
pub(crate) fn captured_stderr(instance: &str) -> Option<Vec<u8>> {
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let captured = lock(&CAPTURED_STDERR).get(instance).cloned()?;
        if let Some(failed) = captured.failed {
            panic!("a reader of {instance}'s web stderr failed: {failed}");
        }
        if captured.reading == 0 {
            return Some(captured.bytes);
        }
        assert!(
            std::time::Instant::now() < until,
            "{instance}'s web stderr was still being read after ten seconds"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// The longest reason the host shows of a confinement refusal, in bytes.
pub(super) const REASON_BYTES: usize = 240;

/// A confinement adapter's reason, or a web process's own, made fit for a log line:
/// printable ASCII only (anything else becomes `?`) and at most [`REASON_BYTES`]
/// long. The adapters' reasons are call names, error names and small numbers; this
/// is the bound that holds whatever the text is. It is shown and never parsed.
pub(super) fn shown(reason: &str) -> String {
    reason
        .chars()
        .take(REASON_BYTES)
        .map(|c| if matches!(c, ' '..='~') { c } else { '?' })
        .collect()
}

/// What a web process that could not confine itself writes to its standard error
/// before it exits (`agentic-org-web`'s `process::REFUSAL_PREFIX`):
///
/// ```text
/// line   = "agentic-org-web: startup refused: confinement: " reason LF
/// reason = 1*240 ( %x20-7E )
/// ```
///
/// The host looks for this one line in the first [`STDERR_KEPT`] bytes of the
/// standard error of a child that ended before it reported ready, and only shows its
/// reason. Nothing else a child writes there is kept, and the reason decides nothing.
pub(super) const REFUSAL_PREFIX: &str = "agentic-org-web: startup refused: confinement: ";

/// What a web process that confined itself without Landlock, where its adapter has
/// that layer, writes to its standard error before its ready line
/// (`agentic-org-web`'s `process::LANDLOCK_NOT_APPLIED_PREFIX`):
///
/// ```text
/// line   = "agentic-org-web: confinement: landlock not applied: " reason LF
/// reason = 1*240 ( %x20-7E )
/// ```
///
/// The ready line cannot carry a reason, so it travels here. The host looks for
/// this one line in the first [`STDERR_KEPT`] bytes of the standard error of a child
/// whose report it has verified, and only when the platform contract says the
/// report lacks a Landlock layer. It shows the reason once per start and decides
/// nothing by it: whether the child is confined is what the host observed.
pub(super) const LANDLOCK_NOT_APPLIED_PREFIX: &str =
    "agentic-org-web: confinement: landlock not applied: ";

/// The start of the line a running web process writes when a `tls.renew` did not replace
/// its pair (`agentic-org-web`'s `process::RENEWAL_KEPT_PREFIX`); its reason is one of
/// [`RENEWAL_KEPT_REASONS`]. The host logs its own text for a known reason, never the line.
pub(super) const RENEWAL_KEPT_PREFIX: &str = "agentic-org-web: tls renewal kept the current pair: ";

const RENEWAL_KEPT_REASONS: [&str; 4] = [
    "fetch_failed",
    "unavailable",
    "unusable_pair",
    "replace_failed",
];

/// How many such lines one web process has forwarded to the host log at most.
pub(super) const RENEWAL_KEPT_LOGGED: usize = 16;

/// Finds the [`RENEWAL_KEPT_PREFIX`] lines in a child's standard error as it is read, across
/// reads. A line longer than the longest such line is skipped whole.
#[derive(Default)]
pub(super) struct RenewalKept {
    line: Vec<u8>,
    overlong: bool,
    logged: usize,
}

impl RenewalKept {
    const LINE_BYTES: usize = 128;

    /// The reasons of the lines that `bytes` completes, in order, up to
    /// [`RENEWAL_KEPT_LOGGED`] for the life of the child.
    pub(super) fn read(&mut self, bytes: &[u8]) -> Vec<&'static str> {
        let mut found = Vec::new();
        for &byte in bytes {
            if byte != b'\n' {
                if self.line.len() < Self::LINE_BYTES {
                    self.line.push(byte);
                } else {
                    self.overlong = true;
                }
                continue;
            }
            let reason = (!self.overlong)
                .then(|| self.line.strip_prefix(RENEWAL_KEPT_PREFIX.as_bytes()))
                .flatten()
                .and_then(|reason| {
                    RENEWAL_KEPT_REASONS
                        .into_iter()
                        .find(|known| known.as_bytes() == reason)
                });
            if let Some(reason) = reason.filter(|_| self.logged < RENEWAL_KEPT_LOGGED) {
                self.logged += 1;
                found.push(reason);
            }
            self.line.clear();
            self.overlong = false;
        }
        found
    }
}

/// What the host logs for a web process that kept its pair after `tls.renew`.
pub(super) fn renewal_kept_text(reason: &str) -> String {
    format!(
        "the web process kept serving its current TLS pair after a renewal: {reason}; it \
         serves the new pair from its next start"
    )
}

/// How much of a child's standard error is kept; the rest is read and dropped.
pub(super) const STDERR_KEPT: usize = 1024;

/// How long a child whose control stream ended may take to exit by itself, so that
/// its last line is whole when it is read. After that it is killed as before.
pub(super) const EXIT_GRACE: Duration = Duration::from_millis(250);

/// How long the end of an exited child's standard error is waited for.
pub(super) const STDERR_GRACE: Duration = Duration::from_millis(250);

/// The reason of the first well-formed refusal line in `captured`, if there is one.
/// A line that is cut off, empty, too long or not printable ASCII is no refusal line.
pub(super) fn refusal_reason(captured: &[u8]) -> Option<&str> {
    line_reason(captured, REFUSAL_PREFIX)
}

/// The reason of the first line in `captured` that starts with `prefix`, if that
/// line is whole and its reason is 1 to [`REASON_BYTES`] bytes of printable ASCII.
pub(super) fn line_reason<'a>(captured: &'a [u8], prefix: &str) -> Option<&'a str> {
    let mut rest = captured;
    while let Some(end) = rest.iter().position(|byte| *byte == b'\n') {
        let (line, after) = rest.split_at(end);
        rest = &after[1..];
        let Some(reason) = line.strip_prefix(prefix.as_bytes()) else {
            continue;
        };
        let printable = reason.iter().all(|byte| (0x20..=0x7e).contains(byte));
        if reason.is_empty() || reason.len() > REASON_BYTES || !printable {
            return None;
        }
        return std::str::from_utf8(reason).ok();
    }
    None
}

/// The start of a child's standard error, read by its own thread until the child's
/// end closes it. `changed` is notified when bytes are kept and when it has ended.
pub(super) struct Stderr {
    pub(super) kept: Mutex<(Vec<u8>, bool)>,
    changed: Condvar,
}

impl Stderr {
    /// Reads `stderr` to its end on a new thread, keeping its first [`STDERR_KEPT`]
    /// bytes and dropping the rest, so that a child never blocks on a full pipe.
    /// A test build also copies all of it into [`CAPTURED_STDERR`] under `instance`.
    pub(super) fn capture(
        mut stderr: impl Read + Send + 'static,
        instance: &str,
        on_renewal_kept: Option<Arc<dyn Fn(&'static str) + Send + Sync>>,
    ) -> io::Result<Arc<Self>> {
        #[cfg(test)]
        let instance = instance.to_owned();
        #[cfg(not(test))]
        let _ = instance;
        let captured = Arc::new(Self {
            kept: Mutex::new((Vec::new(), false)),
            changed: Condvar::new(),
        });
        let filling = Arc::clone(&captured);
        // Counted before the reader starts, so a test that reads the capture after the
        // process ended waits for this reader too.
        #[cfg(test)]
        {
            lock(&CAPTURED_STDERR)
                .entry(instance.clone())
                .or_default()
                .reading += 1;
        }
        #[cfg(test)]
        let ended = {
            let instance = instance.clone();
            move |failed: Option<String>| {
                if let Some(entry) = lock(&CAPTURED_STDERR).get_mut(&instance) {
                    entry.reading -= 1;
                    entry.failed = entry.failed.take().or(failed);
                }
            }
        };
        #[cfg(test)]
        let on_spawn_failure = ended.clone();
        let spawned = thread::Builder::new()
            .name("web-stderr".into())
            .spawn(move || {
                let mut bytes = [0u8; 4096];
                let mut renewals = RenewalKept::default();
                // Reads until the end of the pipe or an error, which also ends it.
                let stopped = loop {
                    let read = match stderr.read(&mut bytes) {
                        Ok(0) => break None,
                        Ok(read) => read,
                        Err(error) => break Some(error),
                    };
                    for reason in renewals.read(&bytes[..read]) {
                        crate::host_log::log(&renewal_kept_text(reason));
                        if matches!(reason, "fetch_failed" | "unavailable") {
                            if let Some(callback) = &on_renewal_kept {
                                callback(reason);
                            }
                        }
                    }
                    #[cfg(test)]
                    lock(&CAPTURED_STDERR)
                        .entry(instance.clone())
                        .or_default()
                        .bytes
                        .extend_from_slice(&bytes[..read]);
                    let mut kept = lock(&filling.kept);
                    let room = STDERR_KEPT - kept.0.len();
                    kept.0.extend_from_slice(&bytes[..read.min(room)]);
                    drop(kept);
                    filling.changed.notify_all();
                };
                lock(&filling.kept).1 = true;
                filling.changed.notify_all();
                #[cfg(test)]
                ended(stopped.map(|error| error.to_string()));
                #[cfg(not(test))]
                let _ = stopped;
            });
        #[cfg(test)]
        if let Err(error) = &spawned {
            on_spawn_failure(Some(format!("the reader did not start: {error}")));
        }
        spawned?;
        Ok(captured)
    }

    /// What the child said of its end, once its standard error has ended or `wait` has
    /// passed: the reason of its refusal line, or else the last whole line it wrote.
    pub(super) fn ending(&self, wait: Duration) -> Ending {
        let (kept, _) = self
            .changed
            .wait_timeout_while(lock(&self.kept), wait, |kept| !kept.1)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match refusal_reason(&kept.0) {
            Some(reason) => Ending::Refused(shown(reason)),
            None => last_line(&kept.0).map_or(Ending::Silent, |line| Ending::Said(shown(line))),
        }
    }

    /// The reason of the child's "landlock not applied" line, as soon as the line is
    /// among the kept bytes. The child wrote it before its ready line, so it is
    /// there once this reader has caught up; without it this returns when the
    /// standard error has ended, the kept bytes are full or `wait` has passed.
    pub(super) fn landlock_not_applied(&self, wait: Duration) -> Option<String> {
        let found = |kept: &[u8]| line_reason(kept, LANDLOCK_NOT_APPLIED_PREFIX).map(shown);
        let (kept, _) = self
            .changed
            .wait_timeout_while(lock(&self.kept), wait, |kept| {
                !kept.1 && kept.0.len() < STDERR_KEPT && found(&kept.0).is_none()
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        found(&kept.0)
    }
}

/// What the host logs of a started web process's confinement: the layers the
/// platform contract names for its verified report, and, where a Landlock layer is
/// missing from them, the reason the web process gave (`reason` is asked only then).
pub(super) fn confined_by(
    layers: &org_platform::confinement::Layers,
    reason: impl FnOnce() -> Option<String>,
) -> String {
    let by = &layers.description;
    if !layers.landlock_missing {
        return format!("confined by {by}");
    }
    match reason() {
        Some(reason) => format!("confined by {by}: Landlock was not applied: {reason}"),
        None => format!(
            "confined by {by}: Landlock was not applied, and the web process gave no reason"
        ),
    }
}

/// What an ended child's standard error says of its end.
pub(super) enum Ending {
    /// Its refusal line's reason: it could not confine itself.
    Refused(String),
    /// No refusal line: the last whole line it wrote (its one failure line, such as the UI
    /// build's refusal or the fixed "startup unavailable"), shown as printable ASCII.
    Said(String),
    /// Nothing whole.
    Silent,
}

/// The last whole, non-empty line of `captured`: the one the child wrote as it ended, after
/// any notice before it. Only the kept start of its standard error is looked at.
fn last_line(captured: &[u8]) -> Option<&str> {
    let whole = &captured[..captured.iter().rposition(|byte| *byte == b'\n')?];
    whole
        .split(|byte| *byte == b'\n')
        .rev()
        .find(|line| !line.is_empty())
        .map(|line| std::str::from_utf8(line).unwrap_or("(not text)"))
}

/// The cause of a start that failed before the ready line, with the reason of the
/// child's own refusal when it left one, or else the last whole line it wrote. Only a
/// child that has ended by itself is asked: one that is already reaped, or, when its
/// control stream ended, one that exits within [`EXIT_GRACE`]. Any other child is left
/// to be killed at once.
pub(super) fn with_refusal(error: io::Error, captured: &Stderr, child: &mut Child) -> io::Error {
    let went_away = error
        .get_ref()
        .is_some_and(|inner| inner.is::<ControlEof>());
    let until = Instant::now()
        + if went_away {
            EXIT_GRACE
        } else {
            Duration::ZERO
        };
    while !matches!(child.try_wait(), Ok(true)) {
        if Instant::now() >= until {
            return error;
        }
        thread::sleep(Duration::from_millis(5));
    }
    match captured.ending(STDERR_GRACE) {
        Ending::Refused(reason) => io::Error::other(format!(
            "{error}; it reported that it cannot confine itself: {reason}"
        )),
        // X5 review, fourth round (V-c): any other end says what the child last wrote, so
        // that the log says why it ended before it connected.
        Ending::Said(line) => io::Error::other(format!("{error}; it wrote: {line}")),
        Ending::Silent => error,
    }
}

/// The cause of a rendezvous peer that is not the admitted child. A child that has
/// ended is no reader either, and it may have connected first: that is said as what
/// it is, with the reason of its own refusal when it left one.
pub(super) fn not_the_reader(captured: &Stderr, child: &mut Child) -> io::Error {
    if !matches!(child.try_wait(), Ok(true)) {
        return failed("the rendezvous peer is not the admitted child");
    }
    let ended = failed(&format!(
        "the child exited before its ready line ({})",
        exited(child)
    ));
    with_refusal(ended, captured, child)
}
