// Prevents an extra console window on Windows in release; harmless elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // First, before any output, logging or thread (host contract): no socket may sit behind stdin, stdout or stderr.
    agentic_org_lib::stdio::sanitize_stdio();
    agentic_org_lib::run();
}
