# Event digest v1 vectors

From the repository root, run `python3 crates/org-sync-proto/test-vectors/generate.py`.
The independent Python standard-library reference assembles length-prefixed binary fields
and computes SHA-256. Rust tests consume the frozen JSON, including preimage hex, and check
the fixed known answer, first event, and restored-HQ epoch continuation. A changed digest
contract requires a new domain tag/version; native command fingerprints remain opaque.
