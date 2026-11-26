//! Wire contracts for `L2` immutable usage & cost ledger, versioned USD model pricing,
//! historical attribution corrections, timezone-aware cost queries, and CSV export
//! (`TASK-53`, `doc-87`, `doc-51`, `FC-084..088`).

mod ledger;
mod subscription;

pub use ledger::{
    CostBreakdownGroup, CostBreakdownRow, CostExportCsv, CostExportResult, CostPeriodPreset,
    CostPriceList, CostPriceListParams, CostPriceListResult, CostPriceSet, CostPriceSetParams,
    CostPriceSetResult, CostQuery, CostQueryParams, CostQueryResult, CostSummaryView,
    CostUsageCorrect, CostUsageCorrectParams, CostUsageCorrectResult, CostUsageRecord,
    CostUsageRecordParams, CostUsageRecordResult, UsageRecordView,
};
pub use subscription::{
    CostSubscriptionUsageCollect, CostSubscriptionUsageCollectParams,
    CostSubscriptionUsageCollectResult, CostSubscriptionUsageCredentialRevoke,
    CostSubscriptionUsageCredentialRevokeParams, CostSubscriptionUsageCredentialRevokeResult,
    CostSubscriptionUsageCredentialSet, CostSubscriptionUsageCredentialSetParams,
    CostSubscriptionUsageCredentialSetResult, CostSubscriptionUsageList,
    CostSubscriptionUsageListParams, CostSubscriptionUsageListResult, SubscriptionModelUsageView,
    SubscriptionUsageAuthSource, SubscriptionUsageCredentialView, SubscriptionUsageSnapshotView,
    SubscriptionUsageStatus, SubscriptionUsageWindowView,
};

use serde::{Deserialize, Serialize};

/// Completeness status of token counts or cost valuation (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CompletenessStatus {
    Complete,
    Partial,
    Unavailable,
    #[serde(other)]
    Unknown,
}

impl CompletenessStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unavailable",
        }
    }
}

/// Basis for a usage record's USD cost valuation (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UsageCostBasis {
    RuntimeEstimate,
    TokenEstimate,
    Unpriced,
    #[serde(other)]
    Unknown,
}

impl UsageCostBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeEstimate => "runtime_estimate",
            Self::TokenEstimate => "token_estimate",
            Self::Unpriced => "unpriced",
            Self::Unknown => "unknown",
        }
    }
}

/// Consuming actor kind for a usage record (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UsageActorKind {
    Agent,
    OneOff,
    #[serde(other)]
    Unknown,
}

impl UsageActorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::OneOff => "one_off",
            Self::Unknown => "one_off",
        }
    }
}

/// Historical work scope of a usage record (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UsageScopeKind {
    Task,
    Project,
    Team,
    Organization,
    #[serde(other)]
    Unknown,
}

impl UsageScopeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Project => "project",
            Self::Team => "team",
            Self::Organization => "organization",
            Self::Unknown => "organization",
        }
    }
}

/// Build vs Run work classification (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum WorkType {
    Build,
    Run,
    #[serde(other)]
    Unknown,
}

impl WorkType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Run => "run",
            Self::Unknown => "build",
        }
    }
}

/// How work attribution was captured (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AttributionBasis {
    WorkFor,
    CurrentTask,
    ExplicitContext,
    Unattributed,
    ManualCorrection,
    #[serde(other)]
    Unknown,
}

impl AttributionBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkFor => "work_for",
            Self::CurrentTask => "current_task",
            Self::ExplicitContext => "explicit_context",
            Self::Unattributed => "unattributed",
            Self::ManualCorrection => "manual_correction",
            Self::Unknown => "unattributed",
        }
    }
}

/// Kind of immutable usage correction (`doc-87`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CorrectionKind {
    Usage,
    Attribution,
    #[serde(other)]
    Unknown,
}

impl CorrectionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Attribution => "attribution",
            Self::Unknown => "attribution",
        }
    }
}

/// Versioned USD model price (micro-USD per 1,000,000 tokens; `doc-87`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPriceView {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub effective_from: String,
    pub currency: String,
    pub input_micro_usd_per_million: Option<u64>,
    pub output_micro_usd_per_million: Option<u64>,
    pub cache_read_micro_usd_per_million: Option<u64>,
    pub cache_write_micro_usd_per_million: Option<u64>,
    pub created_at: String,
    pub created_by_kind: String,
    pub created_by_id: Option<String>,
}

/// Result of exact integer micro-USD token valuation (`doc-87` §Token normalization and valuation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenValuation {
    pub cost_micro_usd: Option<u64>,
    pub cost_basis: UsageCostBasis,
    pub cost_status: CompletenessStatus,
    pub cost_issue: Option<&'static str>,
}

/// Computes exact integer micro-USD valuation from disjoint token counts and a versioned price,
/// summing category charges in `u128` and rounding half-up ONCE per event (`doc-87`).
///
/// A zero-count category needs no rate; a nonzero or unknown category with no rate makes the
/// valuation partial (when some nonzero category was priced) or unpriced.
pub fn compute_token_cost_micro_usd(
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    price: &ModelPriceView,
) -> TokenValuation {
    let pairs = [
        (input_tokens, price.input_micro_usd_per_million),
        (output_tokens, price.output_micro_usd_per_million),
        (cache_read_tokens, price.cache_read_micro_usd_per_million),
        (cache_write_tokens, price.cache_write_micro_usd_per_million),
    ];
    let mut sum_num: u128 = 0;
    let mut any_priced = false;
    let mut any_missing_rate = false;
    let mut any_unknown_tokens = false;
    for (tokens_opt, rate_opt) in pairs {
        match (tokens_opt, rate_opt) {
            (Some(0), _) => {
                any_priced = true;
            }
            (Some(t), Some(r)) => {
                sum_num = sum_num.saturating_add(u128::from(t).saturating_mul(u128::from(r)));
                any_priced = true;
            }
            (Some(_), None) => {
                any_missing_rate = true;
            }
            (None, _) => {
                any_unknown_tokens = true;
            }
        }
    }
    if !any_priced {
        return TokenValuation {
            cost_micro_usd: None,
            cost_basis: UsageCostBasis::Unpriced,
            cost_status: CompletenessStatus::Unavailable,
            cost_issue: Some(if any_missing_rate {
                "missing_price_rate"
            } else {
                "tokens_unavailable"
            }),
        };
    }
    let rounded = (sum_num.saturating_add(500_000)) / 1_000_000;
    let cost_micro_usd = u64::try_from(rounded).unwrap_or(u64::MAX);
    if any_missing_rate || any_unknown_tokens {
        TokenValuation {
            cost_micro_usd: Some(cost_micro_usd),
            cost_basis: UsageCostBasis::TokenEstimate,
            cost_status: CompletenessStatus::Partial,
            cost_issue: Some(if any_missing_rate {
                "partial_price_rate"
            } else {
                "partial_tokens"
            }),
        }
    } else {
        TokenValuation {
            cost_micro_usd: Some(cost_micro_usd),
            cost_basis: UsageCostBasis::TokenEstimate,
            cost_status: CompletenessStatus::Complete,
            cost_issue: None,
        }
    }
}

#[cfg(test)]
mod tests;
