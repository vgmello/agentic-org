//! Tests of the contract's own code that run on every target.

use crate::Refused;

#[test]
fn the_refusal_text_names_the_capability_and_the_system() {
    let text = Refused::unsupported(super::refused::CAPABILITY).to_string();
    assert!(text.contains("web process confinement"), "{text}");
    assert!(text.contains(std::env::consts::OS), "{text}");
}
