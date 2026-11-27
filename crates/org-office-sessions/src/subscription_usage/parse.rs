//! Strict, duplicate-key-rejecting typed JSON decoders for provider subscription usage payloads
//! (`TASK-79`). Never clamps out-of-range percentages, never silently drops wrong-type fields,
//! rejects bearer/secret reflection in model labels, and preserves exact provider window durations.

use std::collections::BTreeSet;
use std::fmt;

use org_office::accounting::{
    canonical_provider_namespace, MAX_MODEL_NAME_BYTES, MAX_SUBSCRIPTION_MODELS,
};
use org_proto::costs::{SubscriptionModelUsageView, SubscriptionUsageWindowView};
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

use crate::redact::redact_text;
use crate::subscription_usage::SubscriptionUsageCollectOutcome;

const MAX_RFC3339_EPOCH_SECS: u64 = 253_402_300_799; // 9999-12-31T23:59:59Z
const FIVE_HOURS_SECS: u64 = 5 * 3_600;
const DAY_SECS: u64 = 86_400;
const WEEK_SECS: u64 = 7 * 86_400;

/// Strict JSON validator that rejects any object with duplicate keys anywhere in the document.
struct NoDupJson;

impl<'de> Deserialize<'de> for NoDupJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NoDupVisitor;
        impl<'de> Visitor<'de> for NoDupVisitor {
            type Value = NoDupJson;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a valid JSON value without duplicate object keys")
            }

            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(NoDupJson)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(NoDupJson)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(NoDupJson)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(NoDupJson)
            }
            fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
                Ok(NoDupJson)
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(NoDupJson)
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(NoDupJson)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                while seq.next_element::<NoDupJson>()?.is_some() {}
                Ok(NoDupJson)
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut seen = BTreeSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !seen.insert(key) {
                        return Err(de::Error::custom("duplicate JSON object key"));
                    }
                    let _: NoDupJson = map.next_value()?;
                }
                Ok(NoDupJson)
            }
        }
        deserializer.deserialize_any(NoDupVisitor)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ResetAtRaw {
    Rfc3339(String),
    EpochInt(u64),
    EpochFloat(f64),
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RawWindow {
    starts_at: Option<String>,
    ends_at: Option<String>,
    resets_at: Option<String>,
    reset_at: Option<ResetAtRaw>,
    window_seconds: Option<u64>,
    limit_window_seconds: Option<u64>,
    utilization_bps: Option<u64>,
    utilization: Option<f64>,
    used_percent: Option<f64>,
    requests: Option<u64>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct StructuredPayload {
    #[serde(default)]
    subscription_identity: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    models: Vec<StructuredModelRow>,
}

#[derive(Deserialize)]
struct StructuredModelRow {
    model: String,
    #[serde(default)]
    total: Option<RawWindow>,
    #[serde(default)]
    daily: Option<RawWindow>,
    #[serde(default)]
    weekly: Option<RawWindow>,
    #[serde(default)]
    short_term: Option<RawWindow>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AnthropicOAuthPayload {
    five_hour: Option<RawWindow>,
    seven_day: Option<RawWindow>,
    seven_day_opus: Option<RawWindow>,
    seven_day_sonnet: Option<RawWindow>,
    seven_day_haiku: Option<RawWindow>,
}

#[derive(Deserialize)]
struct OpenAiWhamPayload {
    rate_limit: OpenAiRateLimit,
    #[serde(default)]
    additional_rate_limits: Option<Vec<OpenAiExtraLimit>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct OpenAiRateLimit {
    primary_window: Option<RawWindow>,
    secondary_window: Option<RawWindow>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct OpenAiExtraLimit {
    limit_name: Option<String>,
    model: Option<String>,
    rate_limit: Option<OpenAiRateLimit>,
    primary_window: Option<RawWindow>,
    secondary_window: Option<RawWindow>,
}

pub(crate) fn parse_provider_payload(
    provider: &str,
    body: &[u8],
    token: &str,
    expected_subscription_identity: Option<&str>,
) -> SubscriptionUsageCollectOutcome {
    if serde_json::from_slice::<NoDupJson>(body).is_err() {
        return invalid_response();
    }
    let root_probe = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) if v.is_object() => v,
        _ => return invalid_response(),
    };
    let canonical = canonical_provider_namespace(provider);
    let parsed = if root_probe.get("models").is_some() {
        if !structured_shapes_are_objects(&root_probe) {
            return invalid_response();
        }
        match decode_structured_models(body, expected_subscription_identity) {
            Ok(rows) => Some(rows),
            Err(early) => return early,
        }
    } else {
        match canonical.as_str() {
            "anthropic" => {
                if !anthropic_shapes_are_objects(&root_probe) {
                    return invalid_response();
                }
                decode_anthropic_oauth(body)
            }
            "openai" => {
                if !openai_shapes_are_objects(&root_probe) {
                    return invalid_response();
                }
                decode_openai_wham(body)
            }
            _ => None,
        }
    };
    match parsed {
        Some(models) if validate_decoded_models(&models, token) => {
            SubscriptionUsageCollectOutcome::Collected {
                models,
                message: None,
            }
        }
        _ => invalid_response(),
    }
}

fn is_opt_object(val: Option<&serde_json::Value>) -> bool {
    matches!(
        val,
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Object(_))
    )
}

fn structured_shapes_are_objects(root: &serde_json::Value) -> bool {
    let Some(models) = root.get("models").and_then(|v| v.as_array()) else {
        return false;
    };
    models.iter().all(|row| {
        row.as_object().is_some_and(|obj| {
            is_opt_object(obj.get("total"))
                && is_opt_object(obj.get("daily"))
                && is_opt_object(obj.get("weekly"))
                && is_opt_object(obj.get("short_term"))
        })
    })
}

fn anthropic_shapes_are_objects(root: &serde_json::Value) -> bool {
    let Some(obj) = root.as_object() else {
        return false;
    };
    [
        "five_hour",
        "seven_day",
        "seven_day_opus",
        "seven_day_sonnet",
        "seven_day_haiku",
    ]
    .iter()
    .all(|k| is_opt_object(obj.get(*k)))
}

fn openai_rate_limit_is_objects(rl: &serde_json::Value) -> bool {
    rl.as_object().is_some_and(|o| {
        is_opt_object(o.get("primary_window")) && is_opt_object(o.get("secondary_window"))
    })
}

fn openai_shapes_are_objects(root: &serde_json::Value) -> bool {
    let Some(rl) = root.get("rate_limit") else {
        return false;
    };
    if !openai_rate_limit_is_objects(rl) {
        return false;
    }
    match root.get("additional_rate_limits") {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::Array(arr)) => arr.iter().all(|item| {
            item.as_object().is_some_and(|o| {
                o.get("rate_limit")
                    .is_none_or(|r| r.is_null() || openai_rate_limit_is_objects(r))
                    && is_opt_object(o.get("primary_window"))
                    && is_opt_object(o.get("secondary_window"))
            })
        }),
        _ => false,
    }
}

fn invalid_response() -> SubscriptionUsageCollectOutcome {
    SubscriptionUsageCollectOutcome::Unavailable {
        issue_code: "INVALID_PROVIDER_RESPONSE".into(),
        message: "Provider usage payload was malformed or failed strict validation.".into(),
    }
}

fn decode_structured_models(
    body: &[u8],
    expected_identity: Option<&str>,
) -> Result<Vec<SubscriptionModelUsageView>, SubscriptionUsageCollectOutcome> {
    let payload: StructuredPayload =
        serde_json::from_slice(body).map_err(|_| invalid_response())?;
    verify_payload_identity(
        expected_identity,
        payload.subscription_identity.as_deref(),
        payload.account_id.as_deref(),
    )?;
    if payload.models.is_empty() || payload.models.len() > MAX_SUBSCRIPTION_MODELS {
        return Err(invalid_response());
    }
    let mut rows = Vec::with_capacity(payload.models.len());
    for item in payload.models {
        rows.push(decode_structured_row(item).ok_or_else(invalid_response)?);
    }
    rows.sort_by(|a, b| a.model.cmp(&b.model));
    Ok(rows)
}

fn verify_payload_identity(
    expected_identity: Option<&str>,
    reported_sub_id: Option<&str>,
    reported_account_id: Option<&str>,
) -> Result<(), SubscriptionUsageCollectOutcome> {
    let sub_id = reported_sub_id.map(str::trim).filter(|s| !s.is_empty());
    let acct_id = reported_account_id.map(str::trim).filter(|s| !s.is_empty());
    if let (Some(a), Some(b)) = (sub_id, acct_id) {
        if !a.eq_ignore_ascii_case(b) {
            return Err(invalid_response());
        }
    }
    let expected = expected_identity.map(str::trim).filter(|s| !s.is_empty());
    let actual = sub_id.or(acct_id);
    if let (Some(exp), Some(act)) = (expected, actual) {
        if !exp.eq_ignore_ascii_case(act) {
            return Err(SubscriptionUsageCollectOutcome::AuthRequired {
                issue_code: "ACCOUNT_IDENTITY_MISMATCH".into(),
                message: "Provider usage response account identity does not match this connection's subscription_identity.".into(),
            });
        }
    }
    Ok(())
}

fn decode_structured_row(item: StructuredModelRow) -> Option<SubscriptionModelUsageView> {
    let model = item.model.trim().to_owned();
    let total = match item.total {
        Some(w) => convert_window(w, None)?,
        None => SubscriptionUsageWindowView::default(),
    };
    let daily = match item.daily {
        Some(w) => {
            let win = convert_window(w, Some(DAY_SECS))?;
            (win.window_seconds == Some(DAY_SECS)).then_some(win)?
        }
        None => SubscriptionUsageWindowView::default(),
    };
    let weekly = match item.weekly {
        Some(w) => {
            let win = convert_window(w, Some(WEEK_SECS))?;
            (win.window_seconds == Some(WEEK_SECS)).then_some(win)?
        }
        None => SubscriptionUsageWindowView::default(),
    };
    let short_term = match item.short_term {
        Some(w) => {
            let win = convert_window(w, None)?;
            if win.window_seconds.is_some_and(|s| s >= DAY_SECS) {
                return None;
            }
            Some(win)
        }
        None => None,
    };
    Some(SubscriptionModelUsageView {
        model,
        total,
        daily,
        weekly,
        short_term,
    })
}

fn decode_anthropic_oauth(body: &[u8]) -> Option<Vec<SubscriptionModelUsageView>> {
    let payload: AnthropicOAuthPayload = serde_json::from_slice(body).ok()?;
    let mut rows = Vec::new();
    if payload.five_hour.is_some() || payload.seven_day.is_some() {
        let short_term = match payload.five_hour {
            Some(w) => Some(convert_exact_duration_window(w, FIVE_HOURS_SECS)?),
            None => None,
        };
        let weekly = match payload.seven_day {
            Some(w) => convert_exact_duration_window(w, WEEK_SECS)?,
            None => SubscriptionUsageWindowView::default(),
        };
        rows.push(SubscriptionModelUsageView {
            model: "claude-subscription".into(),
            total: SubscriptionUsageWindowView::default(),
            daily: SubscriptionUsageWindowView::default(),
            weekly,
            short_term,
        });
    }
    for (name, bucket) in [
        ("claude-haiku", payload.seven_day_haiku),
        ("claude-opus", payload.seven_day_opus),
        ("claude-sonnet", payload.seven_day_sonnet),
    ] {
        if let Some(w) = bucket {
            rows.push(SubscriptionModelUsageView {
                model: name.into(),
                total: SubscriptionUsageWindowView::default(),
                daily: SubscriptionUsageWindowView::default(),
                weekly: convert_exact_duration_window(w, WEEK_SECS)?,
                short_term: None,
            });
        }
    }
    if rows.is_empty() {
        return None;
    }
    rows.sort_by(|a, b| a.model.cmp(&b.model));
    Some(rows)
}

fn convert_exact_duration_window(
    raw: RawWindow,
    expected_secs: u64,
) -> Option<SubscriptionUsageWindowView> {
    let win = convert_window(raw, Some(expected_secs))?;
    (win.window_seconds == Some(expected_secs)).then_some(win)
}

fn decode_openai_wham(body: &[u8]) -> Option<Vec<SubscriptionModelUsageView>> {
    let payload: OpenAiWhamPayload = serde_json::from_slice(body).ok()?;
    let mut rows = vec![build_openai_row(
        "codex-subscription".into(),
        payload.rate_limit,
    )?];
    if let Some(extras) = payload.additional_rate_limits {
        if extras.len() >= MAX_SUBSCRIPTION_MODELS {
            return None;
        }
        for item in extras {
            let name = item
                .limit_name
                .or(item.model)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())?;
            let rl = item.rate_limit.unwrap_or(OpenAiRateLimit {
                primary_window: item.primary_window,
                secondary_window: item.secondary_window,
            });
            rows.push(build_openai_row(name, rl)?);
        }
    }
    rows.sort_by(|a, b| a.model.cmp(&b.model));
    Some(rows)
}

fn build_openai_row(model: String, rl: OpenAiRateLimit) -> Option<SubscriptionModelUsageView> {
    let mut daily = SubscriptionUsageWindowView::default();
    let mut weekly = SubscriptionUsageWindowView::default();
    let mut short_term: Option<SubscriptionUsageWindowView> = None;
    let mut any_window = false;
    for raw in [rl.primary_window, rl.secondary_window]
        .into_iter()
        .flatten()
    {
        let converted = convert_window(raw, None)?;
        any_window = true;
        match converted.window_seconds {
            Some(DAY_SECS) => {
                if window_has_metrics(&daily) {
                    return None;
                }
                daily = converted;
            }
            Some(WEEK_SECS) => {
                if window_has_metrics(&weekly) {
                    return None;
                }
                weekly = converted;
            }
            Some(secs) if secs > 0 && secs < DAY_SECS => {
                if short_term.is_some() {
                    return None;
                }
                short_term = Some(converted);
            }
            _ => return None,
        }
    }
    if !any_window {
        return None;
    }
    Some(SubscriptionModelUsageView {
        model,
        total: SubscriptionUsageWindowView::default(),
        daily,
        weekly,
        short_term,
    })
}

fn convert_window(
    raw: RawWindow,
    default_window_secs: Option<u64>,
) -> Option<SubscriptionUsageWindowView> {
    let starts_at = match raw.starts_at {
        Some(s) => Some(org_proto::canonicalize_rfc3339_utc(&s)?),
        None => None,
    };
    let ends_at = match raw.ends_at {
        Some(s) => Some(org_proto::canonicalize_rfc3339_utc(&s)?),
        None => None,
    };
    if let (Some(ref s), Some(ref e)) = (&starts_at, &ends_at) {
        let s_ms = org_proto::parse_rfc3339_ms(s)?;
        let e_ms = org_proto::parse_rfc3339_ms(e)?;
        if s_ms >= e_ms {
            return None;
        }
    }
    let resets_at = match (raw.resets_at, raw.reset_at) {
        (Some(s), None) => Some(org_proto::canonicalize_rfc3339_utc(&s)?),
        (None, Some(r)) => Some(parse_reset_at(r)?),
        (None, None) => None,
        (Some(_), Some(_)) => return None,
    };
    let window_seconds = match (raw.window_seconds, raw.limit_window_seconds) {
        (Some(0), _) | (_, Some(0)) => return None,
        (Some(a), Some(b)) if a != b => return None,
        (Some(a), _) | (_, Some(a)) => Some(a),
        (None, None) => default_window_secs,
    };
    let utilization_bps =
        parse_strict_utilization(raw.utilization_bps, raw.utilization, raw.used_percent)?;
    let view = SubscriptionUsageWindowView {
        starts_at,
        ends_at,
        resets_at,
        window_seconds,
        utilization_bps,
        requests: raw.requests,
        input_tokens: raw.input_tokens,
        output_tokens: raw.output_tokens,
        cache_read_tokens: raw.cache_read_tokens,
        cache_write_tokens: raw.cache_write_tokens,
        total_tokens: raw.total_tokens,
    };
    window_has_metrics(&view).then_some(view)
}

fn parse_strict_utilization(
    bps: Option<u64>,
    util: Option<f64>,
    used_pct: Option<f64>,
) -> Option<Option<u32>> {
    match (bps, util, used_pct) {
        (Some(n), None, None) => (n <= 10_000).then_some(Some(n as u32)),
        (None, Some(p), None) | (None, None, Some(p)) => {
            if !p.is_finite() || !(0.0..=100.0).contains(&p) {
                return None;
            }
            Some(Some((p * 100.0).round() as u32))
        }
        (None, None, None) => Some(None),
        _ => None,
    }
}

fn parse_reset_at(raw: ResetAtRaw) -> Option<String> {
    match raw {
        ResetAtRaw::Rfc3339(s) => org_proto::canonicalize_rfc3339_utc(&s),
        ResetAtRaw::EpochInt(secs) => epoch_secs_to_rfc3339(secs),
        ResetAtRaw::EpochFloat(f) => {
            if !f.is_finite() || f < 0.0 || f > MAX_RFC3339_EPOCH_SECS as f64 {
                return None;
            }
            epoch_secs_to_rfc3339(f.trunc() as u64)
        }
    }
}

fn epoch_secs_to_rfc3339(secs: u64) -> Option<String> {
    if secs > MAX_RFC3339_EPOCH_SECS {
        return None;
    }
    let formatted = crate::credentials::format_rfc3339_ms(secs.saturating_mul(1_000));
    org_proto::parse_rfc3339_ms(&formatted).map(|_| formatted)
}

fn window_has_metrics(w: &SubscriptionUsageWindowView) -> bool {
    w.utilization_bps.is_some()
        || w.requests.is_some()
        || w.input_tokens.is_some()
        || w.output_tokens.is_some()
        || w.cache_read_tokens.is_some()
        || w.cache_write_tokens.is_some()
        || w.total_tokens.is_some()
}

fn validate_decoded_models(models: &[SubscriptionModelUsageView], token: &str) -> bool {
    if models.is_empty() || models.len() > MAX_SUBSCRIPTION_MODELS {
        return false;
    }
    let mut seen = BTreeSet::new();
    for m in models {
        let name = m.model.trim();
        if name.is_empty()
            || name.len() > MAX_MODEL_NAME_BYTES
            || name.bytes().any(|b| b < 0x20 || b == 0x7f)
            || !seen.insert(name)
            || reflects_secret(name, token)
        {
            return false;
        }
        let has_any = window_has_metrics(&m.total)
            || window_has_metrics(&m.daily)
            || window_has_metrics(&m.weekly)
            || m.short_term.as_ref().is_some_and(window_has_metrics);
        if !has_any {
            return false;
        }
    }
    true
}

fn reflects_secret(text: &str, token: &str) -> bool {
    if !token.is_empty() && text.contains(token) {
        return true;
    }
    redact_text(text, &[token]) != text
}
