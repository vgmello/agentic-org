//! Test-owned teardown: the real Client's Drop policy stays identical in every build.
use crate::test_teardown::teardown_failed;
use std::path::Path;
use std::time::Instant;

pub(super) struct Client(org_client::Client);

impl Client {
    pub(super) fn connect(socket: impl AsRef<Path>) -> Result<Self, org_client::ClientError> {
        org_client::Client::connect(socket).map(Self)
    }
}

impl std::ops::Deref for Client {
    type Target = org_client::Client;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Err(error) = self.0.close_and_join(Instant::now() + super::WAIT) {
            teardown_failed(error);
        }
    }
}
