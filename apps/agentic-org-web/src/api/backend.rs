//! The `ApiBackend` implementation: routing, preparing, dispatching and executing every `/api/v1`
//! call, and the live-event stream.

use super::{
    account_routes, attachment_routes, bad_input, chat_routes, copilot_routes, cost_routes,
    create_outcome, doc_routes, external_auth_routes, failure, git_routes, hiring_routes,
    instance_routes, instance_routes::CallerAuth, instance_view, invalid, mutation, not_found,
    note_routes, office_routes, open_routes, org_routes, organization_view, process_routes,
    provider_routes, result, review_routes, search_routes, session_routes, task_routes,
    tess_routes, unavailable, view, Call, CliApprovalDecideBody, Credentials, EmptyBody,
    RegisterBody, Route, SessionApi, SignInBody,
};
use crate::body::{stream::StreamedBody, BusinessBody};
use crate::host::Failure;
use crate::path::RequestPath;
use crate::service::{ApiBackend, ApiReply, AsyncDispatch, PreparedCall, WebReply};
use crate::sse;
use hyper::{http::request::Parts, Method, StatusCode};
use org_host_control::browser::{
    BrowserSessionEnd, BrowserSessionEndParams, InstanceView, LinkRegister, LinkRegisterParams,
    SessionSignIn, SessionSignInParams,
};
use org_host_control::organization::{self as host_org, BrowserAuth};
use org_proto::web::{
    self as gateway,
    organizations::{self, CreatingOrganization, SelectedCreateRequest, SelectedOrganizationList},
    CreateState, MutationOutcome, OperationId, OperationKind, OperationLookup, Scope,
    StreamRequest, Version, WebErrorCode,
};
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl SessionApi {
    pub(super) async fn execute_events_stream(
        &self,
        caller: CallerAuth,
        request: StreamRequest,
    ) -> WebReply {
        let session = match self.validate_async(caller).await {
            Ok(session) => session,
            Err(error) => return WebReply::Json(failure(error)),
        };
        let list = match self
            .host
            .request_async::<host_org::List>(&host_org::ListParams {
                auth: self.auth(&caller),
            })
            .await
        {
            Ok(list) => list,
            Err(error) => return WebReply::Json(failure(error)),
        };
        let hq_by_org: std::collections::BTreeMap<String, String> = list
            .organizations
            .iter()
            .filter_map(|org| {
                let office_id = org
                    .offices
                    .iter()
                    .find(|o| o.role == org_proto::OfficeRole::Hq)
                    .or_else(|| org.offices.first())
                    .map(|o| o.id.clone())?;
                Some((org.id.clone(), office_id))
            })
            .collect();
        let linked: BTreeSet<String> = list.organizations.into_iter().map(|o| o.id).collect();
        if !sse::authorize_subscriptions(&request, self.host.instance(), &linked) {
            return WebReply::Json(ApiReply::error(
                StatusCode::FORBIDDEN,
                WebErrorCode::NotAuthorized,
                "Subscription scope is not authorized.",
            ));
        }
        let targets = collect_poll_targets(&request, &hq_by_org, self.auth(&caller));
        WebReply::Stream(sse::open_stream_on_bus(
            self.events.as_ref(),
            self.host.instance().to_owned(),
            session.session_id,
            request,
            Some(Arc::clone(&self.offices)),
            targets,
        ))
    }
}

pub(super) fn collect_poll_targets(
    request: &StreamRequest,
    hq_by_org: &std::collections::BTreeMap<String, String>,
    auth: BrowserAuth,
) -> Vec<sse::OfficePollTarget> {
    let mut by_org: std::collections::BTreeMap<String, Option<u64>> =
        std::collections::BTreeMap::new();
    for sub in &request.subscriptions {
        if let Scope::Organization { org_id, .. } = &sub.scope {
            let parsed = sub
                .cursor
                .as_ref()
                .and_then(|c| sse::parse_org_cursor_seq(c, org_id));
            let entry = by_org.entry(org_id.clone()).or_insert(None);
            if entry.is_none() && parsed.is_some() {
                *entry = parsed;
            }
        }
    }
    by_org
        .into_iter()
        .filter_map(|(org_id, initial_org_seq)| {
            let office_id = hq_by_org.get(&org_id)?.clone();
            Some(sse::OfficePollTarget {
                org_id,
                office_id,
                auth: auth.clone(),
                initial_org_seq,
            })
        })
        .collect()
}

impl ApiBackend for SessionApi {
    type Route = Route;
    type Call = Call;
    fn route(&self, request: &Parts, path: &RequestPath) -> Result<Route, ApiReply> {
        let query = request.uri.query().is_some();
        // HEAD is GET without the body (`into_http` drops it), as `Allow` advertises.
        let get = matches!(request.method, Method::GET | Method::HEAD) && !query;
        let post = request.method == Method::POST && !query;
        let patch = request.method == Method::PATCH && !query;
        let segments: Vec<&str> = path.segments().iter().map(String::as_str).collect();
        Ok(match segments.as_slice() {
            ["api", "v1", "version"] if get => Route::Version,
            ["api", "v1", "access"] if get => Route::Access,
            ["api", "v1", "session", "sign-in"] if post => Route::SignIn(Self::peer(request)?),
            ["api", "v1", "links", "register"] if post => Route::Register(Self::peer(request)?),
            ["api", "v1", "links", "inspect"] if post => {
                Route::Account(account_routes::AccountRoute::LinkInspect)
            }
            ["api", "v1", "links", "register", "sign-in"] if post => Route::Account(
                account_routes::AccountRoute::LinkRegisterAttach(Self::peer(request)?),
            ),
            ["api", "v1", "links", "set-password"] if post => Route::Account(
                account_routes::AccountRoute::LinkSetPassword(Self::peer(request)?),
            ),
            ["api", "v1", "account", "password"] if post => Route::Account(
                account_routes::AccountRoute::ChangePassword(self.caller_auth(request)?),
            ),
            ["api", "v1", "account", "preferences"] if get => Route::Account(
                account_routes::AccountRoute::PreferencesGet(self.caller_auth(request)?),
            ),
            ["api", "v1", "account", "preferences"] if post || patch => Route::Account(
                account_routes::AccountRoute::PreferencesSet(self.caller_auth(request)?),
            ),
            ["api", "v1", "instance", "admin", "handover"] if post => Route::Account(
                account_routes::AccountRoute::AdminHandover(self.caller_auth(request)?),
            ),
            ["api", "v1", "instance", "auth", "enable"] if post => {
                let pair = if self.is_auth_enabled() {
                    Some(self.pair(request)?)
                } else {
                    None
                };
                Route::EnableAuth(Self::peer(request)?, pair)
            }
            ["api", "v1", "instance", "tls"] if post => {
                Route::EnableTls(self.caller_auth(request)?)
            }
            ["api", "v1", "session"] if get => Route::Session(self.caller_auth(request)?),
            ["api", "v1", "session", "sign-out"] if post => {
                Route::SignOut(self.pair(request).map_err(|r| self.clear(r))?)
            }
            ["api", "v1", "organizations"] if get => {
                Route::Organizations(self.caller_auth(request)?)
            }
            ["api", "v1", "organizations"] if post => {
                Route::CreateOrganization(self.caller_auth(request)?)
            }
            ["api", "v1", "organizations", org, tail @ ..] => {
                return org_routes::route_organization(self, request, org, tail, get, post, query);
            }
            ["api", "v1", "events", "stream"] if post => {
                Route::EventsStream(self.caller_auth(request)?)
            }
            ["api", "v1", "cli-approvals", "decide"] if post => {
                Route::CliApprovalDecide(self.caller_auth(request)?)
            }
            ["api", "v1", "operations", "host", id]
                if matches!(request.method, Method::GET | Method::HEAD) =>
            {
                let caller = self.caller_auth(request)?;
                let id = organizations::host_operation_id(id).map_err(bad_input)?;
                self.host_lookup_scope(request.uri.query())?;
                Route::Operation(caller, id)
            }
            ["api", "v1", "operations", "org", id]
                if matches!(request.method, Method::GET | Method::HEAD) =>
            {
                let caller = self.caller_auth(request)?;
                let id = organizations::host_operation_id(id).map_err(bad_input)?;
                let org_id = self.org_lookup_scope(request.uri.query())?;
                Route::OrgOperation(caller, org_id, id)
            }
            ["api", "v1", "instance"] if get => Route::Instance(self.caller_auth(request)?),
            ["api", "v1", "version"]
            | ["api", "v1", "access"]
            | ["api", "v1", "instance"]
            | ["api", "v1", "instance", "auth", "enable"]
            | ["api", "v1", "instance", "tls"]
            | ["api", "v1", "session", "sign-in"]
            | ["api", "v1", "links", "register"]
            | ["api", "v1", "links", "inspect"]
            | ["api", "v1", "links", "register", "sign-in"]
            | ["api", "v1", "links", "set-password"]
            | ["api", "v1", "account", "password"]
            | ["api", "v1", "account", "preferences"]
            | ["api", "v1", "instance", "admin", "handover"]
            | ["api", "v1", "session"]
            | ["api", "v1", "session", "sign-out"]
            | ["api", "v1", "organizations"]
            | ["api", "v1", "events", "stream"]
            | ["api", "v1", "cli-approvals", "decide"]
            | ["api", "v1", "operations", "host", _]
            | ["api", "v1", "operations", "org", _] => return Err(invalid()),
            _ => return Err(not_found()),
        })
    }
    fn prepare(&self, route: Route, body: BusinessBody) -> Result<PreparedCall<Call>, ApiReply> {
        let bytes = body.as_bytes();
        let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
        let call = match route {
            Route::Version => empty().map(|()| Call::Version)?,
            Route::Access => empty().map(|()| Call::Access)?,
            Route::EnableAuth(peer, pair) => {
                return instance_routes::prepare_auth_enable(peer, pair, bytes);
            }
            Route::EnableTls(caller) => return instance_routes::prepare_tls_enable(caller, bytes),
            Route::Session(pair) => empty().map(|()| Call::Session(pair))?,
            Route::Organizations(pair) => empty().map(|()| Call::Organizations(pair))?,
            Route::OrgSnapshot(pair, org) => empty().map(|()| Call::OrgSnapshot(pair, org))?,
            Route::OrgProjects(pair, org, include_archived) => {
                empty().map(|()| Call::OrgProjects(pair, org, include_archived))?
            }
            Route::OrgProjectInspect(pair, org, project) => {
                empty().map(|()| Call::OrgProjectInspect(pair, org, project))?
            }
            Route::OrgProjectCreate(pair, org) => {
                return org_routes::prepare_project_create(self, pair, org, bytes);
            }
            Route::OrgOffices(pair, org) => empty().map(|()| Call::OrgOffices(pair, org))?,
            Route::OfficeRestart(pair, org, office) => {
                return org_routes::prepare_office_restart(self, pair, org, office, bytes);
            }
            Route::OrgMutation(pair, org, op) => {
                let scope = Scope::Organization {
                    instance_id: self.host.instance().to_owned(),
                    org_id: org.clone(),
                };
                let command = op.decode(bytes, &scope).map_err(bad_input)?;
                let id = command.operation_id().clone();
                return Ok(PreparedCall::mutation(
                    Call::OrgMutation(pair, org, command),
                    id,
                    bytes.len(),
                ));
            }
            Route::OrgOperation(pair, org, id) => {
                empty().map(|()| Call::OrgOperation(pair, org, id))?
            }
            Route::EventsStream(pair) => {
                let request: StreamRequest =
                    gateway::decode_request(bytes).map_err(|_| invalid())?;
                if request.subscriptions.is_empty() || request.subscriptions.len() > 32 {
                    return Err(invalid());
                }
                Call::EventsStream(pair, request)
            }
            Route::CliApprovalDecide(pair) => {
                let CliApprovalDecideBody { link, approve } =
                    serde_json::from_slice(bytes).map_err(|_| invalid())?;
                Call::CliApprovalDecide(pair, link, approve)
            }
            Route::Instance(pair) => empty().map(|()| Call::Instance(pair))?,
            Route::Operation(pair, id) => empty().map(|()| Call::Operation(pair, id))?,
            Route::CreateOrganization(pair) => {
                // The operation id's format is decided first and named, then the envelope,
                // its scope and the payload's rules: all before any host call.
                let raw: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|_| invalid())?;
                // A missing id, or one that is not a string, is named the same way.
                let id = raw
                    .get("operation_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let id = organizations::host_operation_id(id).map_err(bad_input)?;
                let request: SelectedCreateRequest =
                    gateway::decode_request(bytes).map_err(|_| invalid())?;
                let instance = Scope::Instance {
                    instance_id: self.host.instance().to_owned(),
                };
                request
                    .scope
                    .check_target(&instance)
                    .map_err(|_| invalid())?;
                let plan = request.params.plan().map_err(bad_input)?;
                return Ok(PreparedCall::mutation(
                    Call::CreateOrganization(pair, id.clone(), plan),
                    id,
                    bytes.len(),
                ));
            }
            Route::SignOut(pair) => {
                serde_json::from_slice::<EmptyBody>(bytes).map_err(|_| invalid())?;
                Call::SignOut(pair)
            }
            Route::SignIn(address) => {
                let SignInBody { login, password } =
                    serde_json::from_slice(bytes).map_err(|_| invalid())?;
                Call::SignIn(address, Credentials { login, password })
            }
            Route::Register(address) => {
                let RegisterBody {
                    link,
                    login,
                    password,
                } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
                Call::Register(address, link, Credentials { login, password })
            }
            Route::Account(route) => return account_routes::prepare(route, bytes),
            Route::OpenOrg(route) => return open_routes::prepare(route, bytes),
            Route::Hiring(route) => return hiring_routes::prepare(route, bytes),
            Route::Sessions(route) => return session_routes::prepare(route, bytes),
            Route::Chat(route) => return chat_routes::prepare(route, bytes),
            Route::Tasks(route) => return task_routes::prepare(route, bytes),
            Route::Notes(route) => return note_routes::prepare(route, bytes),
            Route::Git(route) => return git_routes::prepare(route, bytes),
            Route::Process(route) => return process_routes::prepare(route, bytes),
            Route::Docs(route) => return doc_routes::prepare(route, bytes),
            Route::Search(route) => return search_routes::prepare(route, bytes),
            Route::Copilot(route) => return copilot_routes::prepare(route, bytes),
            Route::Cost(route) => return cost_routes::prepare(route, bytes),
            Route::Provider(route) => return provider_routes::prepare(route, bytes),
            Route::OfficeFederation(route) => return office_routes::prepare(route, bytes),
            Route::Attachments(route) => return attachment_routes::prepare(route, bytes),
            Route::Tess(route) => return tess_routes::prepare(route, bytes),
            Route::ExternalAuth(route) => return external_auth_routes::prepare(route, bytes),
            Route::Reviews(route) => return review_routes::prepare(route, bytes),
        };
        Ok(PreparedCall::read(call, bytes.len()))
    }
    fn dispatch_async(self: Arc<Self>, call: Call) -> AsyncDispatch<Call> {
        match call {
            Call::OrgSnapshot(pair, org_id) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(org_routes::execute_org_snapshot(&self, pair, &org_id).await)
                }),
                streaming: false,
            },
            Call::OrgProjects(pair, org_id, include_archived) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_projects_list(&self, pair, &org_id, include_archived)
                            .await,
                    )
                }),
                streaming: false,
            },
            Call::OrgProjectInspect(pair, org_id, project_ref) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_project_inspect(&self, pair, &org_id, &project_ref)
                            .await,
                    )
                }),
                streaming: false,
            },
            Call::OrgProjectCreate(pair, org_id, op_id, params) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_project_create(&self, pair, &org_id, op_id, params)
                            .await,
                    )
                }),
                streaming: false,
            },
            Call::OrgOffices(pair, org_id) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(org_routes::execute_org_offices(&self, pair, &org_id).await)
                }),
                streaming: false,
            },
            Call::OfficeRestart(pair, org_id, office_id, op_id) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_office_restart(&self, pair, &org_id, &office_id, op_id)
                            .await,
                    )
                }),
                streaming: false,
            },
            Call::OrgMutation(pair, org_id, command) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_org_mutation(&self, pair, &org_id, command).await,
                    )
                }),
                streaming: false,
            },
            Call::OrgOperation(pair, org_id, op_id) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_org_operation(&self, pair, &org_id, op_id).await,
                    )
                }),
                streaming: false,
            },
            Call::EventsStream(pair, request) => AsyncDispatch::Async {
                future: Box::pin(async move { self.execute_events_stream(pair, request).await }),
                streaming: true,
            },
            Call::CliApprovalDecide(pair, link, approve) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_cli_approval_decide(&self, pair, link, approve).await,
                    )
                }),
                streaming: false,
            },
            Call::Hiring(call) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(hiring_routes::execute(&self, call).await)
                }),
                streaming: false,
            },
            Call::Sessions(call) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(session_routes::execute(&self, call).await)
                }),
                streaming: false,
            },
            Call::Chat(call) => AsyncDispatch::Async {
                future: Box::pin(
                    async move { WebReply::Json(chat_routes::execute(&self, call).await) },
                ),
                streaming: false,
            },
            Call::Tasks(call) => AsyncDispatch::Async {
                future: Box::pin(
                    async move { WebReply::Json(task_routes::execute(&self, call).await) },
                ),
                streaming: false,
            },
            Call::Notes(call) => AsyncDispatch::Async {
                future: Box::pin(
                    async move { WebReply::Json(note_routes::execute(&self, call).await) },
                ),
                streaming: false,
            },
            Call::Git(call) => AsyncDispatch::Async {
                future: Box::pin(
                    async move { WebReply::Json(git_routes::execute(&self, call).await) },
                ),
                streaming: false,
            },
            Call::Process(call) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(process_routes::execute(&self, call).await)
                }),
                streaming: false,
            },
            Call::Docs(call) => AsyncDispatch::Async {
                future: Box::pin(
                    async move { WebReply::Json(doc_routes::execute(&self, call).await) },
                ),
                streaming: false,
            },
            Call::Search(call) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(search_routes::execute(&self, call).await)
                }),
                streaming: false,
            },
            Call::Copilot(call) => match call.timeout() {
                Some(timeout) => AsyncDispatch::AsyncBounded {
                    future: Box::pin(async move {
                        WebReply::Json(copilot_routes::execute(&self, call).await)
                    }),
                    timeout,
                },
                None => AsyncDispatch::Async {
                    future: Box::pin(async move {
                        WebReply::Json(copilot_routes::execute(&self, call).await)
                    }),
                    streaming: false,
                },
            },
            Call::Cost(call) => AsyncDispatch::Async {
                future: Box::pin(
                    async move { WebReply::Json(cost_routes::execute(&self, call).await) },
                ),
                streaming: false,
            },
            Call::Provider(call) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(provider_routes::execute(&self, call).await)
                }),
                streaming: false,
            },
            Call::OfficeFederation(call) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(office_routes::execute(&self, call).await)
                }),
                streaming: false,
            },
            Call::Attachments(call) => AsyncDispatch::Async {
                future: Box::pin(attachment_routes::execute(self, call)),
                streaming: false,
            },
            Call::Tess(call) => AsyncDispatch::Async {
                streaming: tess_routes::is_streaming(&call),
                future: Box::pin(tess_routes::execute(self, call)),
            },
            Call::ExternalAuth(call) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(external_auth_routes::execute(&self, call).await)
                }),
                streaming: false,
            },
            Call::Reviews(call) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(review_routes::execute(&self, call).await)
                }),
                streaming: false,
            },
            other => AsyncDispatch::Sync(other),
        }
    }
    fn streamed_body(&self, route: &Route) -> Option<u64> {
        match route {
            Route::Attachments(route) => attachment_routes::streamed_body(route),
            _ => None,
        }
    }
    fn prepare_streamed(
        &self,
        route: Route,
        body: StreamedBody,
    ) -> Result<PreparedCall<Call>, ApiReply> {
        match route {
            Route::Attachments(route) => attachment_routes::prepare_streamed(route, body),
            _ => Err(not_found()),
        }
    }
    fn execute(&self, call: Call) -> ApiReply {
        match call {
            Call::Version => result(Version::default()),
            Call::Access => instance_routes::execute_access(self),
            Call::EnableAuth(client_address, pair, credentials) => {
                instance_routes::execute_auth_enable(self, client_address, pair, credentials)
            }
            Call::EnableTls(caller, input) => {
                instance_routes::execute_tls_enable(self, caller, input)
            }
            Call::Account(call) => account_routes::execute(self, call),
            Call::OpenOrg(call) => open_routes::execute(self, call),
            Call::Hiring(_)
            | Call::Sessions(_)
            | Call::Chat(_)
            | Call::Tasks(_)
            | Call::Notes(_)
            | Call::Git(_)
            | Call::Search(_)
            | Call::Copilot(_)
            | Call::Process(_)
            | Call::Docs(_)
            | Call::Cost(_)
            | Call::Provider(_)
            | Call::OfficeFederation(_)
            | Call::Attachments(_)
            | Call::Tess(_)
            | Call::ExternalAuth(_)
            | Call::Reviews(_)
            | Call::OrgSnapshot(_, _)
            | Call::OrgProjects(_, _, _)
            | Call::OrgProjectInspect(_, _, _)
            | Call::OrgProjectCreate(_, _, _, _)
            | Call::OrgOffices(_, _)
            | Call::OfficeRestart(_, _, _, _)
            | Call::OrgMutation(_, _, _)
            | Call::OrgOperation(_, _, _)
            | Call::EventsStream(_, _)
            | Call::CliApprovalDecide(_, _, _) => {
                unreachable!("office RPCs and SSE streams dispatch asynchronously")
            }
            Call::Session(pair) => match self
                .validate(pair)
                .and_then(|s| view(self.host.instance(), s))
            {
                Ok(view) => result(view),
                Err(e) => failure(e),
            },
            Call::Instance(pair) => match self
                .validate(pair)
                .and_then(|_| self.host.request::<InstanceView>(&Default::default()))
                .and_then(|v| instance_view(self.host.instance(), self.listen_scheme, v))
            {
                Ok(view) => result(view),
                Err(e) => failure(e),
            },
            // Always the host's answer: what an account can open is never read from a cache.
            Call::Organizations(pair) => {
                let listed = self
                    .validate(pair)
                    .and_then(|_| {
                        self.host.request::<host_org::List>(&host_org::ListParams {
                            auth: self.auth(&pair),
                        })
                    })
                    .and_then(|list| {
                        Ok(SelectedOrganizationList {
                            organizations: list
                                .organizations
                                .into_iter()
                                .map(organization_view)
                                .collect::<Result<_, _>>()?,
                            creating: list
                                .creating
                                .into_iter()
                                .map(|c| {
                                    Ok(CreatingOrganization {
                                        operation_id: OperationId::parse(c.operation_id)
                                            .map_err(|_| Failure::Unavailable)?,
                                        name: c.name,
                                        slug: c.slug,
                                    })
                                })
                                .collect::<Result<_, Failure>>()?,
                        })
                    });
                match listed {
                    Ok(list) => result(list),
                    Err(Failure::Refused(_)) => unavailable(),
                    Err(e) => failure(e),
                }
            }
            Call::CreateOrganization(pair, id, plan) => {
                if let Err(e) = self.validate(pair) {
                    return failure(e);
                }
                // Let the host decide administrator authority authoritatively (L-05: no
                // duplicated BFF admin short-circuit).
                let reply = self
                    .host
                    .request::<host_org::Create>(&host_org::CreateParams {
                        auth: self.auth(&pair),
                        operation_id: id.as_str().to_owned(),
                        plan,
                    });
                // The host answers `expired` only for an id it has never seen (one it
                // knows answers its stored state at any age), so no create has this id:
                // a plain error, never an outcome of the operation.
                if matches!(&reply, Err(Failure::Refused(error))
                    if error.code == org_proto::ErrorCode::Expired)
                {
                    return ApiReply::error(
                        StatusCode::GONE,
                        WebErrorCode::Expired,
                        "This request expired. Start again.",
                    );
                }
                // No stated answer: the create may or may not have been admitted. The
                // same id asks again.
                mutation(
                    create_outcome(&id, reply)
                        .unwrap_or(MutationOutcome::Unknown { operation_id: id }),
                    true,
                )
            }
            Call::Operation(pair, id) => execute_operation(self, pair, id),
            Call::SignOut(pair) => {
                // The host deletes the record and pushes `session.ended` before this
                // reply; the event sink drops the cache entry on the reader thread.
                let ended = self.validate(CallerAuth::Pair(pair)).and_then(|session| {
                    self.host
                        .request::<BrowserSessionEnd>(&BrowserSessionEndParams {
                            session_id: session.session_id,
                        })
                });
                self.clear(match ended {
                    Ok(_) => result(serde_json::json!({})),
                    Err(e) => failure(e),
                })
            }
            Call::SignIn(client_address, Credentials { login, password }) => {
                self.establish(|pair| {
                    self.host.request_then::<SessionSignIn>(
                        &SessionSignInParams {
                            login,
                            password,
                            client_address,
                            cookie_digest: pair.cookie_digest,
                            proof_digest: pair.proof_digest,
                            origin: self.origin.clone(),
                        },
                        self.caching(pair),
                    )
                })
            }
            Call::Register(client_address, link, Credentials { login, password }) => {
                let auth_flag = Arc::clone(&self.auth_enabled);
                self.establish(|pair| {
                    let reply = self.host.request_then::<LinkRegister>(
                        &LinkRegisterParams {
                            link,
                            login,
                            password,
                            client_address,
                            cookie_digest: pair.cookie_digest,
                            proof_digest: pair.proof_digest,
                            origin: self.origin.clone(),
                        },
                        self.caching(pair),
                    );
                    if matches!(&reply, Ok(session) if session.admin) {
                        auth_flag.store(true, Ordering::Release);
                    }
                    reply
                })
            }
        }
    }
}

fn execute_operation(api: &SessionApi, pair: CallerAuth, id: OperationId) -> ApiReply {
    let reply = api.validate(pair).and_then(|_| {
        api.host
            .request::<host_org::OperationLookup>(&host_org::OperationLookupParams {
                auth: api.auth(&pair),
                operation_id: id.as_str().to_owned(),
            })
    });
    let outcome = match reply {
        Err(e @ (Failure::Denied | Failure::NotFound | Failure::Unavailable)) => return failure(e),
        // A refused lookup is the lookup's own error, never a stored refusal
        // of the create: `expired` is 410, anything else is unavailable. (The
        // host looks a known operation up at any age and answers `not_found`
        // for any other, so it sends no `expired` here today.)
        Err(Failure::Refused(error)) => {
            return if error.code == org_proto::ErrorCode::Expired {
                ApiReply::error(
                    StatusCode::GONE,
                    WebErrorCode::Expired,
                    "This operation is too old to look up.",
                )
            } else {
                unavailable()
            };
        }
        stated => create_outcome(&id, stated),
    };
    let state = match &outcome {
        Ok(MutationOutcome::Accepted { .. }) => Some(CreateState::Reserved),
        Ok(MutationOutcome::Completed { .. }) => Some(CreateState::Linked),
        Ok(MutationOutcome::Rejected { .. }) => None,
        _ => return unavailable(),
    };
    match outcome {
        Ok(outcome) => result(OperationLookup::Outcome {
            kind: OperationKind::Host,
            state,
            outcome,
        }),
        Err(e) => failure(e),
    }
}
