//! Password bodies have a separate small, preallocated, zeroed input path.
use crate::body::{BodyError, BODY_READ_TIMEOUT};
use http_body_util::BodyExt;
use hyper::body::{Body, Bytes};
use zeroize::Zeroizing;

pub const SECRET_BODY_BYTES: usize = 16 * 1024;
pub struct SecretBody(Zeroizing<Vec<u8>>);
impl SecretBody {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
impl std::fmt::Debug for SecretBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBody(<redacted>)")
    }
}
pub async fn read_secret_body<B>(body: B) -> Result<SecretBody, BodyError>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    if body.size_hint().lower() > SECRET_BODY_BYTES as u64 {
        return Err(BodyError::TooLarge);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(SECRET_BODY_BYTES));
    let mut body = Box::pin(body);
    tokio::time::timeout(BODY_READ_TIMEOUT, async {
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| BodyError::Malformed)?;
            if let Ok(data) = frame.into_data() {
                if data.len() > SECRET_BODY_BYTES - bytes.len() {
                    return Err(BodyError::TooLarge);
                }
                bytes.extend_from_slice(&data);
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| BodyError::TimedOut)??;
    Ok(SecretBody(bytes))
}
