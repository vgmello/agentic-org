# Event digest v1 vectors

From the repository root, run `python3 crates/org-sync-proto/test-vectors/generate.py`.
The independent Python standard-library reference assembles length-prefixed binary fields
and computes SHA-256. Rust tests consume the frozen JSON, including the digest; they check
the fixed known answer, first event, and restored-HQ epoch continuation. A changed digest
contract requires a new domain tag/version; native command fingerprints remain opaque.

The same generator writes authority-record-v1.json. Rust checks its complete preimage
byte for byte and its SHA-256 known answer. The authority signature covers precisely
authority_record_preimage_v1: domain tag with NUL, length-prefixed organization/HQ IDs,
32-byte CA digest, three u64 big-endian authority/restored-epoch/restored-seq values,
then the 32-byte restored digest. No signature bytes enter their own preimage.
