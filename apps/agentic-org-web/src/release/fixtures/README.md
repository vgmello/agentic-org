Public input-verifier fixture, generated once with OpenSSL 4.1.0 Ed25519 in a
temporary directory. Only public registry, exact manifest, raw signature and
public payload bytes are retained. The generated private key was discarded.

The four three-byte executable payloads are deliberately not executable images.
The Linux manifest is used only by private unit-test verification and preload;
it cannot admit a running gateway, platform signing, release activation or a
production trust registry. Tests never inject this registry through a public
runtime configuration.
