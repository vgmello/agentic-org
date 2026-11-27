//! Building `host.sock` responses: results, refusals and secret-bearing lines.

use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::control_listener;
use crate::control_listener::Connection;

pub(crate) fn secret_response(connection: &Connection, id: u64, result: &impl serde::Serialize) {
    #[derive(serde::Serialize)]
    struct SecretResponse<'a, T: ?Sized> {
        id: u64,
        result: &'a T,
    }
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|n| *n < org_proto::MAX_FRAME_BYTES)
                .ok_or_else(|| std::io::Error::other("secret response limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let value = SecretResponse { id, result };
    let mut count = Count(0);
    if serde_json::to_writer(&mut count, &value).is_ok() {
        // Measure first so encoding cannot leave a secret in a grown buffer.
        let mut bytes = Vec::with_capacity(count.0 + 1);
        if serde_json::to_writer(&mut bytes, &value).is_ok() {
            bytes.push(b'\n');
            connection.send_secret_line(control_listener::SecretLine::new(bytes));
            return;
        }
        crate::secrets::zero(&mut bytes);
    }
    connection.send_response(Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "secret result unavailable"),
    ));
}

pub(super) fn not_authorized(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::NotAuthorized,
            "this request is not authorized on host.sock",
        ),
    )
}

pub(super) fn params_dont_fit(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::InvalidRequest,
            "the params don't fit this method",
        ),
    )
}

/// Runs a typed handler: params that don't decode are `invalid_request`.
pub(super) fn answer<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<C::Result, ApiError>,
) -> Response {
    let params = match request.params::<C>() {
        Ok(params) => params,
        Err(_) => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    "the params don't fit this method",
                ),
            );
        }
    };
    match handler(params) {
        Ok(result) => ok::<C>(request.id, &result),
        Err(error) => Response::error(request.id, error),
    }
}

/// A successful answer to request `id` (or `storage_error` if it can't be encoded).
pub(crate) fn ok<C: Call>(id: u64, result: &C::Result) -> Response {
    Response::ok::<C>(id, result).unwrap_or_else(|_| {
        Response::error(
            id,
            ApiError::new(ErrorCode::StorageError, "the answer couldn't be encoded"),
        )
    })
}
