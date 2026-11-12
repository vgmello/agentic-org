use std::fmt;

use org_proto::location::SocketPathTooLong;
use org_proto::{ApiError, CallError};

#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// Nothing is listening at the socket (the server isn't running, or isn't ours).
    NotRunning(std::io::Error),
    /// Any other connection or read/write failure.
    Io(std::io::Error),
    /// No answer arrived in time. The request may or may not have been carried out.
    Timeout,
    /// The connection closed before the answer arrived.
    Disconnected,
    /// The server answered with an error.
    Api(ApiError),
    /// The server's answer didn't have the shape this client expects.
    Protocol(String),
    /// The request is larger than the protocol allows. Nothing was sent.
    RequestTooLarge,
    /// Neither `ORG_OFFICE_HOME` nor `HOME` is set to an absolute path, so the office home
    /// (and with it every socket) can't be found.
    NoOfficeHome,
    /// The socket path is longer than macOS allows.
    SocketPathTooLong(SocketPathTooLong),
    /// The office has no local socket this machine can reach (it isn't on this Mac, or its
    /// id is not a plain id).
    OfficeNotReachable,
    /// This connection is already subscribed to events.
    AlreadySubscribed,
    /// The server speaks a different protocol version.
    VersionMismatch { client: u32, server: u32 },
    /// A well-formed hello identifies a different instance from the selected context.
    InstanceMismatch,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::NotRunning(_) => {
                write!(
                    f,
                    "nothing is listening there: the host or office server is not running"
                )
            }
            ClientError::Io(e) => write!(f, "connection error: {e}"),
            ClientError::Timeout => write!(f, "the server did not answer in time"),
            ClientError::Disconnected => write!(f, "the connection to the office server closed"),
            // The server's message is for people; the code is in the `ApiError` itself.
            ClientError::Api(e) => write!(f, "{}", e.message),
            ClientError::AlreadySubscribed => write!(f, "already subscribed to events"),
            ClientError::Protocol(why) => write!(f, "unexpected answer from the server: {why}"),
            ClientError::RequestTooLarge => write!(f, "the request is too large to send"),
            ClientError::NoOfficeHome => write!(f, "could not find the office home folder"),
            ClientError::SocketPathTooLong(e) => write!(f, "{e}"),
            ClientError::OfficeNotReachable => {
                write!(f, "that office can't be reached from this Mac")
            }
            ClientError::VersionMismatch { client, server } => write!(
                f,
                "the office server speaks protocol version {server}, this app speaks {client}"
            ),
            ClientError::InstanceMismatch => write!(f, "the server belongs to another instance"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClientError::NotRunning(e) | ClientError::Io(e) => Some(e),
            ClientError::Api(e) => Some(e),
            ClientError::SocketPathTooLong(e) => Some(e),
            _ => None,
        }
    }
}

impl ClientError {
    /// A server message that didn't decode. serde's own text can quote the offending value,
    /// which may be a secret (an app token), and an error is easily logged, so it is dropped
    /// (the position it reports is not meaningful here either): `what` names, from the
    /// client's own knowledge, the message that failed.
    pub(crate) fn undecodable(what: &str) -> Self {
        ClientError::Protocol(format!("{what} doesn't fit the contract"))
    }
}

impl From<CallError> for ClientError {
    fn from(error: CallError) -> Self {
        match error {
            CallError::Api(e) => ClientError::Api(e),
            CallError::Decode(_) => ClientError::undecodable("a result"),
        }
    }
}
