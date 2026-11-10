//! In-memory TLS identity supplied by the host; never read certificate/key paths.

use std::fmt;
use std::sync::{Arc, RwLock};

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use tokio_rustls::rustls;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsError {
    InvalidIdentity,
    Unavailable,
}

/// One resolver is shared by the server configuration. New handshakes use the
/// current identity; a failed replacement leaves the prior identity intact.
pub struct TlsCertificates {
    resolver: Arc<CertificateResolver>,
    config: Arc<rustls::ServerConfig>,
}

impl fmt::Debug for TlsCertificates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TlsCertificates { .. }")
    }
}

struct CertificateResolver {
    current: RwLock<Arc<CertifiedKey>>,
}

impl fmt::Debug for CertificateResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CertificateResolver { .. }")
    }
}

impl ResolvesServerCert for CertificateResolver {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.current.read().ok().map(|key| Arc::clone(&key))
    }
}

fn certified_key(
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<CertifiedKey>, TlsError> {
    let provider = rustls::crypto::ring::default_provider();
    let certified =
        CertifiedKey::from_der(chain, key, &provider).map_err(|_| TlsError::InvalidIdentity)?;
    certified
        .keys_match()
        .map_err(|_| TlsError::InvalidIdentity)?;
    Ok(Arc::new(certified))
}

impl TlsCertificates {
    pub fn new(
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self, TlsError> {
        let resolver = Arc::new(CertificateResolver {
            current: RwLock::new(certified_key(chain, key)?),
        });
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|_| TlsError::InvalidIdentity)?
        .with_no_client_auth()
        .with_cert_resolver(resolver.clone());
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        config.max_early_data_size = 0;
        Ok(Self {
            resolver,
            config: Arc::new(config),
        })
    }

    /// Called only with a fresh host tls.fetch result following tls.renew.
    pub fn replace(
        &self,
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<(), TlsError> {
        let fresh = certified_key(chain, key)?;
        *self
            .resolver
            .current
            .write()
            .map_err(|_| TlsError::Unavailable)? = fresh;
        Ok(())
    }

    pub fn server_config(&self) -> Arc<rustls::ServerConfig> {
        Arc::clone(&self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::PrivatePkcs8KeyDer;

    fn cert(which: u8) -> Vec<CertificateDer<'static>> {
        let bytes: &[u8] = if which == b'a' {
            include_bytes!("../tests/fixtures/test-cert-a.der")
        } else {
            include_bytes!("../tests/fixtures/test-cert-b.der")
        };
        vec![CertificateDer::from(bytes.to_vec())]
    }

    fn key(which: u8) -> PrivateKeyDer<'static> {
        let bytes: &[u8] = if which == b'a' {
            include_bytes!("../tests/fixtures/test-key-a.der")
        } else {
            include_bytes!("../tests/fixtures/test-key-b.der")
        };
        PrivatePkcs8KeyDer::from(bytes.to_vec()).into()
    }

    #[test]
    fn rotation_updates_the_existing_config_resolver_atomically() {
        let certificates = TlsCertificates::new(cert(b'a'), key(b'a')).unwrap();
        let config = certificates.server_config();
        let before = Arc::clone(&certificates.resolver.current.read().unwrap());
        assert_eq!(before.cert, cert(b'a'));
        certificates.replace(cert(b'b'), key(b'b')).unwrap();
        let after = Arc::clone(&certificates.resolver.current.read().unwrap());
        assert_eq!(after.cert, cert(b'b'));
        assert_eq!(before.cert, cert(b'a'));
        assert!(!Arc::ptr_eq(&before, &after));
        assert!(Arc::ptr_eq(&config, &certificates.server_config()));
        assert_eq!(config.alpn_protocols, [b"http/1.1".to_vec()]);
        assert_eq!(config.max_early_data_size, 0);
    }

    #[test]
    fn rejected_identity_does_not_replace_a_working_one() {
        let certificates = TlsCertificates::new(cert(b'a'), key(b'a')).unwrap();
        let before = Arc::clone(&certificates.resolver.current.read().unwrap());
        assert_eq!(
            certificates.replace(cert(b'b'), key(b'a')),
            Err(TlsError::InvalidIdentity)
        );
        assert!(Arc::ptr_eq(
            &before,
            &certificates.resolver.current.read().unwrap()
        ));
        assert!(matches!(
            TlsCertificates::new(vec![], key(b'a')),
            Err(TlsError::InvalidIdentity)
        ));
        let bad_key = PrivatePkcs8KeyDer::from(vec![1, 2, 3]).into();
        assert!(matches!(
            TlsCertificates::new(cert(b'a'), bad_key),
            Err(TlsError::InvalidIdentity)
        ));
        assert_eq!(format!("{certificates:?}"), "TlsCertificates { .. }");
    }
}
