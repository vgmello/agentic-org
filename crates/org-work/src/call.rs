//! The work writes this build handles (phase 1), and who makes them.

use org_proto::work::{
    ChecklistAdd, ChecklistAddParams, ChecklistCheck, ChecklistCheckParams, ChecklistEdit,
    ChecklistEditParams, ChecklistMakeTask, ChecklistMakeTaskParams, ChecklistMove,
    ChecklistMoveParams, ChecklistRemove, ChecklistRemoveParams, ChecklistUnlinkTask,
    ChecklistUnlinkTaskParams, TaskAssign, TaskAssignParams, TaskCreate, TaskCreateParams,
    TaskSetStatus, TaskSetStatusParams, TaskUpdate, TaskUpdateParams, WorkComment,
    WorkCommentParams, WorkDelete, WorkDeleteParams, WorkLink, WorkLinkParams, WorkUndo,
    WorkUndoParams, WorkUnlink, WorkUnlinkParams, WorkWatch, WorkWatchParams,
};
use org_proto::{ApiError, Call, ErrorCode, Request};

/// The bound member a call acts as (§2.2). The wire never carries an actor: the office makes
/// this from the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The member the connection is bound as.
    pub member_id: String,
    /// The connection is the app's (it may make app-only calls).
    pub via_app: bool,
}

impl Caller {
    /// An agent's connection, bound as the agent.
    pub fn agent(member_id: impl Into<String>) -> Self {
        Self {
            member_id: member_id.into(),
            via_app: false,
        }
    }

    /// The app's connection, bound as the owner or a person.
    pub fn app(member_id: impl Into<String>) -> Self {
        Self {
            member_id: member_id.into(),
            via_app: true,
        }
    }
}

macro_rules! work_calls {
    ($($variant:ident($call:ident, $params:ident)),* $(,)?) => {
        /// A work write, read from a request.
        #[derive(Debug, Clone, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum WorkCall {
            $($variant($params),)*
        }

        impl WorkCall {
            /// Reads `request` as a work write: `Ok(None)` if its method isn't one this build
            /// handles, `Err(invalid_request)` if it is but its params aren't valid.
            pub fn parse(request: &Request) -> Result<Option<Self>, ApiError> {
                $(if request.is::<$call>() {
                    return request
                        .params::<$call>()
                        .map(|p| Some(Self::$variant(p)))
                        .map_err(|_| ApiError::new(
                            ErrorCode::InvalidRequest,
                            format!("the params of `{}` are not valid", request.method),
                        ));
                })*
                Ok(None)
            }

            /// The method name.
            pub fn method(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => $call::METHOD,)*
                }
            }

            /// The client-made id that makes a retry get the same answer.
            pub fn change_id(&self) -> &str {
                match self {
                    $(Self::$variant(p) => &p.change_id,)*
                }
            }

            /// The params, re-encoded as JSON (for the params hash and the size check).
            pub(crate) fn params_json(&self) -> serde_json::Value {
                let encoded = match self {
                    $(Self::$variant(p) => serde_json::to_value(p),)*
                };
                encoded.expect("work params always encode as JSON")
            }
        }
    };
}

work_calls!(
    TaskCreate(TaskCreate, TaskCreateParams),
    TaskUpdate(TaskUpdate, TaskUpdateParams),
    TaskAssign(TaskAssign, TaskAssignParams),
    TaskSetStatus(TaskSetStatus, TaskSetStatusParams),
    Comment(WorkComment, WorkCommentParams),
    Watch(WorkWatch, WorkWatchParams),
    Link(WorkLink, WorkLinkParams),
    Unlink(WorkUnlink, WorkUnlinkParams),
    Undo(WorkUndo, WorkUndoParams),
    Delete(WorkDelete, WorkDeleteParams),
    ChecklistAdd(ChecklistAdd, ChecklistAddParams),
    ChecklistEdit(ChecklistEdit, ChecklistEditParams),
    ChecklistCheck(ChecklistCheck, ChecklistCheckParams),
    ChecklistMove(ChecklistMove, ChecklistMoveParams),
    ChecklistRemove(ChecklistRemove, ChecklistRemoveParams),
    ChecklistMakeTask(ChecklistMakeTask, ChecklistMakeTaskParams),
    ChecklistUnlinkTask(ChecklistUnlinkTask, ChecklistUnlinkTaskParams),
);

impl WorkCall {
    /// App-only calls (§2.2): refused on any other connection with `not_authorized`, before
    /// anything else. In phase 1: `work.delete`.
    pub fn app_only(&self) -> bool {
        matches!(self, Self::Delete(_))
    }
}
