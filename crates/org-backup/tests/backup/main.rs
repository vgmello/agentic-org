//! `org-backup` end to end over a real SQLite HQ office in temporary folders: plain and
//! encrypted round trips (backup, verify with a disposable projection rebuild, in-place
//! restore that queries back identically), the layout's exclusions, determinism, held-lock
//! and writer-custody refusals, hostile archives, and HQ's WAL-mode private file space.

mod support;

mod hostile;
mod private_space;
mod refusals;
mod round_trip;
