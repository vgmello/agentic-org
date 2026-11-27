//! The office side of the Chief of Staff's `caller ∩ tess` gate (`TASK-80.1`): who her live run
//! acts for (its run binding, in RAM), that member's live authority, the hire, conversation
//! and message-file (`TASK-240`) reads the gate narrows by, and the drafts she made for each
//! member while the office runs.
//!
//! Every read fails closed. Her employee ID never changes (she is never offboarded), so once
//! read it is remembered; until it is, a failed read refuses every bound member rather than
//! letting anyone through ungated.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

use org_office::Office;
use org_proto::chat::{ChatConversationsParams, ConversationSummary};
use org_proto::{ApiError, ErrorCode};

use crate::serve::{ChiefOfStaffPort, Delegation};

/// How many of her drafts the office remembers the member of; the oldest is forgotten first,
/// after which only the owner may confirm or withdraw that draft.
const REMEMBERED_DRAFTS: usize = 1_024;

/// The production [`ChiefOfStaffPort`].
pub(crate) struct ChiefOfStaffGate {
    office: Arc<Office>,
    sessions: Arc<org_office_sessions::SessionManager>,
    known: OnceLock<String>,
    drafts: Mutex<VecDeque<(String, String)>>,
}

impl ChiefOfStaffGate {
    pub(crate) fn new(
        office: Arc<Office>,
        sessions: Arc<org_office_sessions::SessionManager>,
    ) -> Self {
        Self {
            office,
            sessions,
            known: OnceLock::new(),
            drafts: Mutex::new(VecDeque::new()),
        }
    }

    /// Her employee ID, read once; `Ok(None)` while the organization has no Chief of Staff.
    fn chief_of_staff_id(&self) -> Result<Option<&str>, ApiError> {
        if let Some(id) = self.known.get() {
            return Ok(Some(id.as_str()));
        }
        match self.office.chief_of_staff() {
            Ok(Some(tess)) => Ok(Some(
                self.known.get_or_init(|| tess.id.to_string()).as_str(),
            )),
            Ok(None) => Ok(None),
            Err(error) => {
                crate::logged("chief_of_staff.lookup", &error);
                Err(unavailable())
            }
        }
    }
}

impl ChiefOfStaffPort for ChiefOfStaffGate {
    fn delegation(&self, member: &str) -> Result<Option<Delegation>, ApiError> {
        if self.chief_of_staff_id()? != Some(member) {
            return Ok(None);
        }
        let caller_id = self.sessions.bindings().acting_for_caller_id(member);
        let caller = match caller_id.as_deref() {
            Some(caller) => self.office.caller_authority(caller).map_err(|error| {
                crate::logged("chief_of_staff.caller_authority", &error);
                unavailable()
            })?,
            None => org_office::CallerAuthority::default(),
        };
        Ok(Some(Delegation { caller_id, caller }))
    }

    fn caller_may_hire_into(&self, caller: &str, team_id: &str) -> Result<bool, ApiError> {
        self.office
            .caller_may_hire_into(caller, team_id)
            .map_err(|error| {
                crate::logged("chief_of_staff.may_hire_into", &error);
                unavailable()
            })
    }

    fn hire_team(&self, request_id: &str) -> Result<Option<String>, ApiError> {
        self.office
            .delegated_hire_scope(request_id)
            .map(|scope| scope.map(|scope| scope.home_team_id))
            .map_err(|error| {
                crate::logged("chief_of_staff.hire_team", &error);
                unavailable()
            })
    }

    fn visible_conversations(&self, member: &str) -> Result<Vec<ConversationSummary>, ApiError> {
        let params = ChatConversationsParams {
            include_archived: true,
        };
        match self.office.list_conversations(member, params) {
            Ok(Ok(listed)) => Ok(listed.conversations),
            // A member who may not list conversations sees none.
            Ok(Err(_)) => Ok(Vec::new()),
            Err(error) => {
                crate::logged("chief_of_staff.visible_conversations", &error);
                Err(unavailable())
            }
        }
    }

    fn message_conversation(&self, message_id: &str) -> Result<Option<String>, ApiError> {
        self.office
            .message_conversation_id(message_id)
            .map_err(|error| {
                crate::logged("chief_of_staff.message_conversation", &error);
                unavailable()
            })
    }

    fn attachment_conversation(&self, attachment_id: &str) -> Result<Option<String>, ApiError> {
        self.office
            .attachment_conversation_id(attachment_id)
            .map_err(|error| {
                crate::logged("chief_of_staff.attachment_conversation", &error);
                unavailable()
            })
    }

    fn resolve_member(&self, reference: &str) -> Result<Option<String>, ApiError> {
        self.office.resolve_member_id(reference).map_err(|error| {
            crate::logged("chief_of_staff.resolve_member", &error);
            unavailable()
        })
    }

    fn is_chief_of_staff(&self, member: &str) -> bool {
        self.chief_of_staff_id()
            .map_or(true, |tess| tess == Some(member))
    }

    fn record_draft(&self, request_id: &str, caller: &str) {
        let mut drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
        // The member a draft was first made for keeps it: a replayed or repeated draft of the
        // same request for another member never takes it over.
        if drafts.iter().any(|(id, _)| id == request_id) {
            return;
        }
        if drafts.len() >= REMEMBERED_DRAFTS {
            drafts.pop_front();
        }
        drafts.push_back((request_id.to_owned(), caller.to_owned()));
    }

    fn draft_caller(&self, request_id: &str) -> Option<String> {
        let drafts = self.drafts.lock().unwrap_or_else(|p| p.into_inner());
        drafts
            .iter()
            .find(|(id, _)| id == request_id)
            .map(|(_, caller)| caller.clone())
    }
}

fn unavailable() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "the office could not check what the Chief of Staff may do; try again",
    )
}
