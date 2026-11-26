//! Opening a connection, and closing it in tests.

use std::io::ErrorKind;
use std::path::Path;
use std::sync::mpsc;

use org_proto::OfficeInfo;

use super::*;
use crate::paths::{
    check_socket_path, host_socket_in, office_socket, office_socket_by_id, selected_office_socket,
};
use crate::transport::{read_loop, write_loop};

impl Client {
    /// Connects to this machine's host (see [`default_host_socket_path`]).
    pub fn connect_host() -> Result<Self, ClientError> {
        Self::connect(default_host_socket_path()?)
    }

    /// Connects to the host of the office home `home` (an absolute path).
    pub fn connect_host_in(home: &Path) -> Result<Self, ClientError> {
        Self::connect(host_socket_in(home)?)
    }

    /// Connects to an office server on this machine, as the host listed it: by its
    /// `endpoint` if it gave one and that is a socket inside `<home>/run/`, else by the
    /// rule in `org_proto::location`. `home` must be absolute.
    pub fn connect_office(office: &OfficeInfo, home: &Path) -> Result<Self, ClientError> {
        Self::connect(office_socket(office, home)?)
    }

    /// Connects to office `office_id` on this machine by the rule in `org_proto::location`
    /// (`<home>/run/<handle>.sock`), without asking the host. `home` must be absolute. For a
    /// client that knows its office's id, such as an agent's CLI (see [`agent`]).
    pub fn connect_office_id(office_id: &str, home: &Path) -> Result<Self, ClientError> {
        Self::connect(office_socket_by_id(home, office_id)?)
    }

    /// Connects to a selected UUIDv7 office inside `runtime_dir` (`selected_office_socket_in`),
    /// refusing non-UUIDv7 office IDs or mismatched server office identities immediately.
    pub fn connect_selected_office(
        office_id: &str,
        runtime_dir: &Path,
    ) -> Result<Self, ClientError> {
        let client = Self::connect(selected_office_socket(runtime_dir, office_id)?)?;
        let matched = client
            .hello()?
            .office
            .as_ref()
            .is_some_and(|office| office.office.id == office_id);
        if !matched {
            return Err(ClientError::OfficeNotReachable);
        }
        Ok(client)
    }

    /// Connects to the server listening on `socket`.
    pub fn connect(socket: impl AsRef<Path>) -> Result<Self, ClientError> {
        let socket = socket.as_ref();
        check_socket_path(socket)?;
        let stream = UnixStream::connect(socket).map_err(|e| match e.kind() {
            ErrorKind::NotFound | ErrorKind::ConnectionRefused => ClientError::NotRunning(e),
            _ => ClientError::Io(e),
        })?;
        let io = ClientError::Io;
        let reader_stream = stream.try_clone().map_err(io)?;
        let writer_stream = stream.try_clone().map_err(io)?;

        let shared = Arc::new(Shared {
            closed: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            events: Arc::new(Mutex::new(None)),
            next_subscription: AtomicU64::new(1),
            queued_bytes: AtomicUsize::new(0),
            stream,
        });
        let (outbound, queue) = mpsc::sync_channel(WRITE_QUEUE);
        let reader = {
            let shared = Arc::clone(&shared);
            Helper::spawn("org-client-reader", move || {
                read_loop(&shared, reader_stream)
            })
            .map_err(ClientError::Io)?
        };
        let writer = {
            let shared = Arc::clone(&shared);
            Helper::spawn("org-client-writer", move || {
                write_loop(&shared, writer_stream, queue)
            })
        };
        // Not joined on drop: the writer ends once the socket is shut down and its queue is
        // gone, and `drop` must not wait on a write.
        let writer = match writer {
            Ok(writer) => writer,
            Err(error) => {
                // The reader is already running and would never end: stop it.
                shared.close();
                let _ = reader.join();
                return Err(ClientError::Io(error));
            }
        };
        Ok(Self {
            shared,
            outbound: Some(outbound),
            reader: Some(reader),
            _writer: Some(writer),
        })
    }

    /// Closes the connection and joins its helpers before a test releases its fd fence.
    /// A timeout names the unfinished helper and keeps its handle for a later attempt.
    /// Both helpers are attempted against the same deadline; the first error is reported.
    /// If a writer error leaves the reader unjoined, its diagnostic states that explicitly.
    /// Normal Drop remains bounded and may detach helpers in every build.
    #[cfg(any(test, feature = "testkit"))]
    pub fn close_and_join(&mut self, deadline: Instant) -> std::io::Result<()> {
        self.shared.close();
        self.outbound.take();
        let writer = Helper::join_before(&mut self._writer, deadline);
        let reader = Helper::join_before(&mut self.reader, deadline);
        match writer {
            Err(error) if self.reader.is_some() => Err(std::io::Error::new(
                error.kind(),
                format!("{error}; org-client-reader not joined"),
            )),
            Err(error) => Err(error),
            Ok(()) => reader,
        }
    }
}
