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

Run `python3 crates/org-sync-proto/test-vectors/generate_work.py` for the independent Work
record/source digest vectors. Both use SHA-256, NUL-terminated versioned domain tags and
big-endian integers. `work_record_digest_v1` hashes the u32-length-prefixed record ID,
u64 generation/version, u32-length-prefixed codec ID, u32 codec version, and u64-length-prefixed
payload. `work_source_digest_v1` hashes u32-length-prefixed office/incarnation, u64 source
sequence, the 32-byte record digest, and u64 Org epoch/sequence/creation time.

The two vectors include binary payload, different generation/version, a ULID incarnation,
and maximum u64 values. Rust checks the independent known answers and tampering of every
field. The hash helper validates representable lengths; core queue policy enforces business
shape, positive versions and frame bounds.
