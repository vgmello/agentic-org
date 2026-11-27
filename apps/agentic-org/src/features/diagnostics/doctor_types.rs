//! Shared diagnostic check status and check record types for `agentic-org doctor`.

use org_cli_shell::output::Style;
use serde::Serialize;

use super::redact::redact_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }

    pub(super) fn style(self) -> Style {
        match self {
            Self::Pass => Style::Ok,
            Self::Warn => Style::Warn,
            Self::Fail => Style::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorCheck {
    pub(crate) id: String,
    pub(crate) category: String,
    pub(crate) status: CheckStatus,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remediation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<String>,
}

impl DoctorCheck {
    pub(crate) fn pass(
        id: impl Into<String>,
        category: impl Into<String>,
        message: impl Into<String>,
        path: Option<String>,
    ) -> Self {
        Self {
            id: redact_text(&id.into()).text,
            category: redact_text(&category.into()).text,
            status: CheckStatus::Pass,
            message: redact_text(&message.into()).text,
            remediation: None,
            path: path.map(|p| redact_text(&p).text),
        }
    }

    pub(crate) fn warn(
        id: impl Into<String>,
        category: impl Into<String>,
        message: impl Into<String>,
        remediation: Option<String>,
        path: Option<String>,
    ) -> Self {
        Self {
            id: redact_text(&id.into()).text,
            category: redact_text(&category.into()).text,
            status: CheckStatus::Warn,
            message: redact_text(&message.into()).text,
            remediation: remediation.map(|r| redact_text(&r).text),
            path: path.map(|p| redact_text(&p).text),
        }
    }

    pub(crate) fn fail(
        id: impl Into<String>,
        category: impl Into<String>,
        message: impl Into<String>,
        remediation: Option<String>,
        path: Option<String>,
    ) -> Self {
        Self {
            id: redact_text(&id.into()).text,
            category: redact_text(&category.into()).text,
            status: CheckStatus::Fail,
            message: redact_text(&message.into()).text,
            remediation: remediation.map(|r| redact_text(&r).text),
            path: path.map(|p| redact_text(&p).text),
        }
    }
}
