//! Engine-independent messaging family on `org-store` (`doc-90`, `ADR-0003`, `ADR-0014`,
//! `ADR-0024`).
//!
//! Durable canonical files live under `<data-root>/<org-id>/durable/messaging/` (never inside
//! organization Git `state/`). SQLite projects them exclusively into the isolated `0700`/`0600`
//! `runtime/<office-id>/protected/messaging.db` database (`messaging-schema.sql`), never into
//! shared `office.db`.

pub mod ask_card;
pub mod asks;
pub mod features;
pub mod files;
pub mod mentions;
pub mod ports;
pub mod projector;
pub mod records;
pub mod runtime;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
