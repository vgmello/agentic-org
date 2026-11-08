//! The dispatch table: wire method name -> handler. Adding a method is one `on::<Call>`
//! registration; the listener and the connection code never change.

use std::collections::HashMap;
use std::sync::Arc;

use org_office::Office;
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::connection::Connection;
use crate::events::EventBus;

/// What a handler may use. Borrowed from the server for the duration of one request.
pub struct Context<'a> {
    pub office: &'a Office,
    pub events: &'a EventBus,
    /// The connection the request came in on (for `subscribe`, and later for the hub).
    pub connection: &'a Arc<Connection>,
}

type RawHandler = Box<dyn Fn(&Context<'_>, &Request) -> Response + Send + Sync>;

#[derive(Default)]
pub struct Router {
    handlers: HashMap<&'static str, RawHandler>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the handler for call `C`. Reading `C::Params` and writing `C::Result` are
    /// done here, so a handler only sees typed values.
    ///
    /// # Panics
    /// If `C::METHOD` is already registered: that is a programming error.
    pub fn on<C: Call>(
        &mut self,
        handler: impl Fn(&Context<'_>, C::Params) -> Result<C::Result, ApiError> + Send + Sync + 'static,
    ) -> &mut Self {
        let raw: RawHandler = Box::new(move |context, request| {
            let params = match request.params::<C>() {
                Ok(params) => params,
                Err(_) => {
                    return Response::error(
                        request.id,
                        ApiError::new(
                            ErrorCode::InvalidRequest,
                            format!("the params of `{}` are not valid", C::METHOD),
                        ),
                    )
                }
            };
            match handler(context, params) {
                Ok(result) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| {
                    Response::error(
                        request.id,
                        ApiError::new(
                            ErrorCode::StorageError,
                            "the server could not encode its answer",
                        ),
                    )
                }),
                Err(error) => Response::error(request.id, error),
            }
        });
        let previous = self.handlers.insert(C::METHOD, raw);
        assert!(
            previous.is_none(),
            "method `{}` registered twice",
            C::METHOD
        );
        self
    }

    /// Answers `request`: with its handler's response, or `unknown_method`.
    pub fn dispatch(&self, context: &Context<'_>, request: &Request) -> Response {
        match self.handlers.get(request.method.as_str()) {
            Some(handler) => handler(context, request),
            None => Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::UnknownMethod,
                    format!("unknown method `{}`", request.method),
                ),
            ),
        }
    }
}
