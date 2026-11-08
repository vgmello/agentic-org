//! # org-office-server: the office daemon
//!
//! A thin binary: configuration, starting `org-office`, serving `org-proto` on the office
//! socket, and installing itself as a macOS login service. All logic lives in `crates/`.

fn main() {
    println!(
        "{} {}: not implemented yet",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
}
