These are public, synthetic test identities for `gateway.test` and loopback. They
grant no instance, host or organization authority. Never use them in a deployment.

Generated 2026-11-10 with OpenSSL, independently for suffixes `a` and `b`:

```sh
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -sha256 \
  -days 3650 -subj /CN=gateway.test \
  -addext 'subjectAltName=DNS:gateway.test,DNS:localhost,IP:127.0.0.1,IP:::1' \
  -addext 'basicConstraints=critical,CA:FALSE' \
  -addext 'keyUsage=critical,digitalSignature' \
  -addext 'extendedKeyUsage=serverAuth' -keyout key.pem -out cert.pem
openssl x509 -in cert.pem -outform DER -out test-cert-a.der
openssl pkcs8 -topk8 -nocrypt -in key.pem -outform DER -out test-key-a.der
```
