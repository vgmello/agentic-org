//! Private subprocess crash rendezvous. This module and every call disappear
//! from production; a test child explicitly arms it after coherent startup.
use std::sync::atomic::{AtomicU8, Ordering};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Point {
    Prepared = 1,
    BlobFlushed,
    BlobPublished,
    ManifestFlushed,
    ManifestPublished,
    SnapshotPublished,
    EventPublished,
    ReceiptPublished,
    SqlCommitted,
    BeforeReply,
    AnchorInstalled = 11,
    ManifestInstalled = 12,
}
static POINT: AtomicU8 = AtomicU8::new(0);
pub(crate) fn arm(point: Point) {
    POINT.store(point as u8, Ordering::SeqCst);
}
pub(crate) fn hit(point: Point) {
    if POINT.load(Ordering::SeqCst) == point as u8 {
        use std::io::Write;
        println!("ACCEPTANCE_CRASH_RENDEZVOUS");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    }
}
pub(super) fn file(name: &str, flushed_only: bool) {
    let point = if name.starts_with("blobs/sha256/") {
        if flushed_only {
            Some(Point::BlobFlushed)
        } else {
            Some(Point::BlobPublished)
        }
    } else if name.contains("/journals/work-acceptance/") && name.contains("/transactions/") {
        if flushed_only {
            Some(Point::ManifestFlushed)
        } else {
            Some(Point::ManifestPublished)
        }
    } else if !flushed_only && name.ends_with(".md") {
        Some(Point::SnapshotPublished)
    } else if !flushed_only && name.starts_with("work-history/tasks/") {
        Some(Point::EventPublished)
    } else if !flushed_only && name.contains("/receipts/") {
        Some(Point::ReceiptPublished)
    } else {
        None
    };
    if let Some(point) = point {
        hit(point);
    }
}

pub(super) fn installed(name: &str) {
    if name.contains("/journals/work-acceptance/") {
        if name.ends_with("/anchor.json") {
            hit(Point::AnchorInstalled);
        } else if name.contains("/transactions/") {
            hit(Point::ManifestInstalled);
        }
    }
}
