//! Production session routes compose with selected business adaptation. Native
//! authority is the admitted HostControl, never a test capability flag.
use super::{BrowserSessions, Established, SecretBody, Ticket};
use crate::{
    body::BusinessBody,
    path::RequestPath,
    selected::{SelectedApi, SessionAuthority},
    service::{ApiBackend, ApiReply, PreparedCall},
};
use hyper::{http::request::Parts, Method, StatusCode};
use org_proto::gateway::{GatewayErrorCode, SessionEstablished};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
pub struct SessionApi {
    sessions: Arc<BrowserSessions>,
    selected: SelectedApi<Arc<BrowserSessions>>,
}
pub enum Route {
    Version,
    SignIn,
    Get(Ticket),
    Refresh(Ticket),
    SignOut(Ticket),
    Selected(crate::selected::Route<Ticket>),
}
pub enum Command {
    Version,
    SignIn {
        login: String,
        password: org_host_control::Password,
    },
    Get(Ticket),
    Refresh(Ticket),
    SignOut(Ticket),
    Selected(crate::selected::Call<Ticket>),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    login: String,
    password: org_host_control::Password,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
fn invalid() -> ApiReply {
    ApiReply::error(
        StatusCode::BAD_REQUEST,
        GatewayErrorCode::InvalidRequest,
        "Invalid session request.",
    )
}
fn object<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, ApiReply> {
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return Err(invalid());
    }
    serde_json::from_slice(bytes).map_err(|_| invalid())
}
impl SessionApi {
    pub fn new(sessions: Arc<BrowserSessions>) -> Self {
        Self {
            selected: SelectedApi::new(Arc::clone(&sessions)),
            sessions,
        }
    }
    fn established(&self, result: Established) -> Result<ApiReply, ApiReply> {
        #[derive(Serialize)]
        struct Reply<T> {
            result: T,
        }
        let result_value = SessionEstablished::new(result.view, result.proof);
        ApiReply::secret_json(
            StatusCode::OK,
            &Reply {
                result: result_value,
            },
        )?
        .cookie(&result.cookie)
    }
}
impl ApiBackend for SessionApi {
    type Route = Route;
    type Call = Command;
    fn route(&self, request: &Parts, path: &RequestPath) -> Result<Route, ApiReply> {
        let parts = path.segments();
        if parts == ["api", "v1", "version"] {
            if request.method != Method::GET || request.uri.query().is_some() {
                return Err(invalid());
            }
            return Ok(Route::Version);
        }
        if matches!(parts,[api,v,session,..] if api=="api"&&v=="v1"&&session=="session") {
            if request.uri.query().is_some() {
                return Err(invalid());
            }
            if parts == ["api", "v1", "session", "sign-in"] && request.method == Method::POST {
                return Ok(Route::SignIn);
            }
            let ticket = self.sessions.authenticate(request)?;
            return match (
                parts.len(),
                parts.last().map(String::as_str),
                &request.method,
            ) {
                (3, _, &Method::GET) => Ok(Route::Get(ticket)),
                (4, Some("refresh"), &Method::POST) => Ok(Route::Refresh(ticket)),
                (4, Some("sign-out"), &Method::POST) => Ok(Route::SignOut(ticket)),
                _ => Err(invalid()),
            };
        }
        self.selected.route(request, path).map(Route::Selected)
    }
    fn credentials(&self, route: &Route) -> bool {
        matches!(route, Route::SignIn)
    }
    fn prepare_credentials(
        &self,
        route: Route,
        body: SecretBody,
    ) -> Result<PreparedCall<Command>, ApiReply> {
        if !matches!(route, Route::SignIn) {
            return Err(invalid());
        }
        let Credentials { login, password } = object(body.as_bytes())?;
        if login.len() > 4096 || password.expose().len() > 4096 {
            return Err(invalid());
        }
        Ok(PreparedCall::read(
            Command::SignIn { login, password },
            body.as_bytes().len(),
        ))
    }
    fn prepare(&self, route: Route, body: BusinessBody) -> Result<PreparedCall<Command>, ApiReply> {
        if let Route::Selected(route) = route {
            return self
                .selected
                .prepare(route, body)
                .map(|prepared| prepared.map(Command::Selected));
        }
        let bytes = body.as_bytes().len();
        let call = match route {
            Route::Version if bytes == 0 => Command::Version,
            Route::Get(ticket) if bytes == 0 => Command::Get(ticket),
            Route::Refresh(ticket) => {
                object::<Empty>(body.as_bytes())?;
                Command::Refresh(ticket)
            }
            Route::SignOut(ticket) => {
                object::<Empty>(body.as_bytes())?;
                Command::SignOut(ticket)
            }
            _ => return Err(invalid()),
        };
        Ok(PreparedCall::read(call, bytes))
    }
    fn execute(&self, call: Command) -> ApiReply {
        match call {
            Command::Selected(call) => self.selected.execute(call),
            Command::Version => ApiReply::json(
                StatusCode::OK,
                serde_json::json!({"result":{"api_version":1,"contract_revision":3}}),
            ),
            Command::SignIn { login, password } => self
                .sessions
                .sign_in(login, password)
                .and_then(|result| self.established(result))
                .unwrap_or_else(|reply| reply),
            Command::Get(ticket) => match self.sessions.get(ticket) {
                Ok(view) => ApiReply::json(StatusCode::OK, serde_json::json!({"result":view})),
                Err(reply) => reply,
            },
            Command::Refresh(ticket) => self
                .sessions
                .refresh(ticket)
                .and_then(|result| self.established(result))
                .unwrap_or_else(|reply| reply),
            Command::SignOut(ticket) => self
                .sessions
                .sign_out(ticket)
                .and_then(|()| {
                    ApiReply::json(StatusCode::OK, serde_json::json!({"result":{}}))
                        .cookie(&self.sessions.clear_cookie())
                })
                .unwrap_or_else(|reply| reply),
        }
    }
}
