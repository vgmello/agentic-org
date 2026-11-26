//! `org-backup` end to end over a real SQLite HQ office in temporary folders: plain and
//! encrypted round trips (backup, verify with a disposable projection rebuild, in-place
//! restore that queries back identically), the layout's exclusions, determinism, held-lock
//! and writer-custody refusals, and hostile archives.

mod support;

mod hostile;
mod refusals;
mod round_trip;
