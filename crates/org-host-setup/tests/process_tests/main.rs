//! Real-process integration tests for host + SQLite registry + setup/CLI + web/office binaries.
#![cfg(debug_assertions)]

mod create_server_helper;
mod enc1_process_tests;
mod https_process_tests;
mod web_process_tests;
