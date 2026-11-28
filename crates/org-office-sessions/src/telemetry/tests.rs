//! Pure unit tests for bounded per-turn telemetry (`TASK-136` AC #1).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use org_agent_runtime::{Usage, UsageScope};

use super::*;

fn make_usage(
    scope: UsageScope,
    model: &str,
    input: u64,
    output: u64,
    cache_read: u64,
    reasoning: u64,
) -> Usage {
    let mut u = Usage::new(scope, scope);
    u.model = Some(model.to_owned());
    u.input = input;
    u.output = output;
    u.cache_read = cache_read;
    u.reasoning = reasoning;
    u
}

#[test]
fn cache_hit_ratio_percentiles_and_averages_handle_edges() {
    assert_eq!(compute_cache_hit_ratio(0, 0), None);
    assert_eq!(compute_cache_hit_ratio(1000, 0), Some(0.0));
    assert_eq!(compute_cache_hit_ratio(0, 500), Some(1.0));
    let ratio = compute_cache_hit_ratio(1000, 3000).unwrap();
    assert!((ratio - 0.75).abs() < 1e-9, "got {ratio}");
    // Large u64 values do not overflow u128 denominator.
    let large = compute_cache_hit_ratio(u64::MAX / 2, u64::MAX / 2).unwrap();
    assert!((large - 0.5).abs() < 1e-9, "got {large}");

    assert_eq!(percentile_u64(&[], 50), None);
    assert_eq!(average_u64(&[]), None);
    let samples = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
    assert_eq!(percentile_u64(&samples, 50), Some(50));
    assert_eq!(percentile_u64(&samples, 95), Some(100));
    assert_eq!(average_u64(&samples), Some(55));
}

#[test]
fn per_turn_telemetry_tracks_latency_tokens_cumulative_deltas_rss_and_denials() {
    let collector = TelemetryCollector::new();
    collector.record_session_started(
        "emp-1",
        Some("claude"),
        Some("claude-sonnet-4-6"),
        Some("medium"),
    );

    // Turn 1: TurnDelta usage + sandbox denial + live RSS sample.
    let t0 = Instant::now();
    collector.record_turn_started("emp-1", None, None, t0);
    collector.record_usage(
        "emp-1",
        "run-1",
        &make_usage(
            UsageScope::TurnDelta,
            "claude-sonnet-4-6",
            1_000,
            250,
            3_000,
            400,
        ),
    );
    collector.record_sandbox_denial("emp-1", 2);
    let live_rss = org_platform::process::process_rss_bytes(std::process::id()).unwrap_or(42_000);
    collector.record_turn_finished(
        "emp-1",
        true,
        Some(live_rss),
        t0 + Duration::from_millis(125),
        1_700_000_000_000,
    );

    // Turn 2: Cumulative usage reports (Codex-style) must subtract previous run watermark.
    let t1 = t0 + Duration::from_millis(500);
    collector.record_turn_started("emp-1", Some("gpt-5.4"), Some("high"), t1);
    collector.record_usage(
        "emp-1",
        "run-cum",
        &make_usage(UsageScope::Cumulative, "gpt-5.4", 500, 100, 500, 50),
    );
    collector.record_usage(
        "emp-1",
        "run-cum",
        &make_usage(UsageScope::Cumulative, "gpt-5.4", 700, 160, 900, 80),
    );
    let higher_rss = live_rss.saturating_add(8_192);
    collector.record_turn_finished(
        "emp-1",
        false,
        Some(higher_rss),
        t1 + Duration::from_millis(300),
        1_700_000_000_800,
    );

    let snap = collector.snapshot_agent("emp-1").expect("agent tracked");
    assert_eq!(snap.total_turn_count, 2);
    assert_eq!(snap.recent_turns.len(), 2);
    assert_eq!(snap.input_tokens, 1_700);
    assert_eq!(snap.output_tokens, 410);
    assert_eq!(snap.cached_read_tokens, 3_900);
    assert_eq!(snap.thought_tokens, 480);
    assert_eq!(snap.sandbox_denial_count, 2);
    assert_eq!(snap.last_rss_bytes, Some(higher_rss));
    assert_eq!(snap.peak_rss_bytes, Some(higher_rss));
    assert_eq!(snap.active_model.as_deref(), Some("gpt-5.4"));
    assert_eq!(snap.active_effort.as_deref(), Some("high"));

    let s1 = &snap.recent_turns[0];
    assert_eq!(s1.wall_latency_ms, 125);
    assert_eq!(s1.input_tokens, 1_000);
    assert_eq!(s1.cached_read_tokens, 3_000);
    assert_eq!(s1.thought_tokens, 400);
    assert!((s1.cache_hit_ratio.unwrap() - 0.75).abs() < 1e-9);
    assert_eq!(s1.sandbox_denial_count, 2);
    assert!(s1.succeeded);

    let s2 = &snap.recent_turns[1];
    assert_eq!(s2.wall_latency_ms, 300);
    assert_eq!(s2.input_tokens, 700);
    assert_eq!(s2.output_tokens, 160);
    assert_eq!(s2.cached_read_tokens, 900);
    assert_eq!(s2.thought_tokens, 80);
    assert!(!s2.succeeded);
}

#[test]
fn pump_telemetry_counts_denials_without_storing_raw_secrets_or_prompts() {
    assert!(!is_sandbox_denial_text(""));
    assert!(!is_sandbox_denial_text("sandbox enabled for workspace"));
    assert!(is_sandbox_denial_text(
        "bwrap: Can't open /etc/shadow: Permission denied"
    ));
    assert!(is_sandbox_denial_text(
        "write outside writable roots denied by policy"
    ));
    assert!(is_sandbox_denial_text(
        "sandbox-exec: operation not permitted"
    ));

    let collector = Arc::new(TelemetryCollector::new());
    let mut rss = HashMap::new();
    rss.insert(777, 12_345_678);
    let pump = PumpTelemetry {
        collector: Arc::clone(&collector),
        sample_rss: Arc::new(move |pid| rss.get(&pid).copied()),
        child_pid: Some(777),
        // Secret-shaped string in metadata must be scrubbed by org-redact.
        active_model: Some("sk-ant-api03-SECRETTOKENVALUE1234567890".into()),
        active_effort: Some("high".into()),
    };

    pump.on_turn_started("emp-sec");
    pump.on_notice(
        "emp-sec",
        "bwrap: Operation not permitted while reading Authorization: Bearer ghp_1234567890abcdefghijklmnopqrstuvwx",
    );
    pump.on_notice("emp-sec", "sandbox enabled: normal status line");
    pump.on_auto_deny(
        "emp-sec",
        "Write",
        Some("tu-sec-1"),
        Some("denied by office policy: prompt=TOP_SECRET_USER_PROMPT_PAYLOAD sk-ant-oat01-ABCDEF1234567890"),
    );
    // Matching ActionDenied + TurnEnded.errors for the same denial must be deduplicated.
    pump.on_action_denied(
        "emp-sec",
        "Write",
        Some("tu-sec-1"),
        Some("denied by office policy: prompt=TOP_SECRET_USER_PROMPT_PAYLOAD sk-ant-oat01-ABCDEF1234567890"),
    );
    pump.on_turn_ended(
        "emp-sec",
        false,
        &[
            "denied by office policy: prompt=TOP_SECRET_USER_PROMPT_PAYLOAD sk-ant-oat01-ABCDEF1234567890".into(),
        ],
    );

    let snap = collector.snapshot_agent("emp-sec").unwrap();
    assert_eq!(snap.sandbox_denial_count, 2);
    assert_eq!(snap.last_rss_bytes, Some(12_345_678));

    let json = serde_json::to_string(&snap.recent_turns).unwrap();
    assert!(!json.contains("SECRETTOKENVALUE"));
    assert!(!json.contains("ghp_1234567890"));
    assert!(!json.contains("TOP_SECRET_USER_PROMPT_PAYLOAD"));
    assert!(!json.contains("sk-ant-oat01-ABCDEF"));
    assert!(
        !snap
            .active_model
            .as_deref()
            .unwrap_or("")
            .contains("SECRETTOKENVALUE"),
        "metadata must pass through org-redact"
    );
}

#[test]
fn denial_deduplication_none_rss_preservation_and_orphaned_turn_reset() {
    let collector = TelemetryCollector::new();
    let t0 = Instant::now();

    // 1. AutoDeny + ActionDenied + matching Notice + TurnEnded.errors count ONCE per tool call.
    collector.record_session_started("emp-dedup", Some("claude"), Some("claude-sonnet-4-6"), None);
    collector.record_turn_started("emp-dedup", None, None, t0);
    collector.record_auto_deny(
        "emp-dedup",
        "Write",
        Some("tu-1"),
        Some("write outside writable roots denied by office policy"),
    );
    collector.record_action_denied(
        "emp-dedup",
        "Write",
        Some("tu-1"),
        Some("write outside writable roots denied by office policy"),
    );
    collector.record_denial_notice(
        "emp-dedup",
        "write outside writable roots denied by office policy",
    );
    // Unkeyed AutoDeny + ActionDenied on the same tool also deduplicates to 1.
    collector.record_auto_deny("emp-dedup", "Bash", None, Some("blocked by sandbox"));
    collector.record_action_denied("emp-dedup", "Bash", None, Some("blocked by sandbox"));
    // TurnEnded errors must NOT double-count once structured denials were already counted.
    collector.record_turn_end_denial_errors(
        "emp-dedup",
        &[
            "write outside writable roots denied by office policy".into(),
            "permission denied".into(),
        ],
    );
    collector.record_turn_finished(
        "emp-dedup",
        false,
        Some(20_000_000),
        t0 + Duration::from_millis(50),
        1,
    );

    // 2. Fallback TurnEnded.errors counts once when no prior AutoDeny/ActionDenied/Notice fired,
    //    AND `rss_bytes = None` is preserved on the TurnTelemetrySample (never copying stale RSS).
    let t1 = t0 + Duration::from_millis(200);
    collector.record_turn_started("emp-dedup", None, None, t1);
    collector.record_turn_end_denial_errors(
        "emp-dedup",
        &[
            "sandbox-exec: operation not permitted".into(),
            "sandbox-exec: operation not permitted".into(),
        ],
    );
    collector.record_turn_finished("emp-dedup", false, None, t1 + Duration::from_millis(40), 2);

    let snap = collector.snapshot_agent("emp-dedup").unwrap();
    assert_eq!(snap.recent_turns.len(), 2);
    assert_eq!(snap.recent_turns[0].sandbox_denial_count, 2);
    assert_eq!(snap.recent_turns[0].rss_bytes, Some(20_000_000));
    assert_eq!(snap.recent_turns[1].sandbox_denial_count, 1);
    assert_eq!(
        snap.recent_turns[1].rss_bytes, None,
        "missing RSS sample must stay None rather than copying stale last_rss_bytes"
    );
    assert_eq!(snap.last_rss_bytes, Some(20_000_000));
    assert_eq!(snap.peak_rss_bytes, Some(20_000_000));
    assert_eq!(snap.sandbox_denial_count, 3);

    // 3. Orphaned active_turn is cleared defensively on `record_session_started` so a new session
    //    cannot inherit stale tokens, denials, or latency from a crashed turn.
    let stale_t = t0 + Duration::from_millis(500);
    collector.record_turn_started(
        "emp-orphan",
        Some("claude-sonnet-4-6"),
        Some("high"),
        stale_t,
    );
    collector.record_usage(
        "emp-orphan",
        "run-dead",
        &make_usage(UsageScope::TurnDelta, "claude-sonnet-4-6", 9_999, 500, 0, 0),
    );
    collector.record_sandbox_denial("emp-orphan", 4);
    // New session starts without prior turn finishing:
    collector.record_session_started(
        "emp-orphan",
        Some("claude"),
        Some("claude-haiku-4-5"),
        Some("low"),
    );
    let fresh_t = stale_t + Duration::from_secs(60);
    collector.record_turn_started("emp-orphan", Some("claude-haiku-4-5"), Some("low"), fresh_t);
    collector.record_usage(
        "emp-orphan",
        "run-new",
        &make_usage(UsageScope::TurnDelta, "claude-haiku-4-5", 100, 20, 50, 0),
    );
    collector.record_turn_finished(
        "emp-orphan",
        true,
        None,
        fresh_t + Duration::from_millis(15),
        3,
    );
    let orphan_snap = collector.snapshot_agent("emp-orphan").unwrap();
    assert_eq!(orphan_snap.recent_turns.len(), 1);
    let fresh_sample = &orphan_snap.recent_turns[0];
    assert_eq!(fresh_sample.wall_latency_ms, 15);
    assert_eq!(fresh_sample.input_tokens, 100);
    assert_eq!(fresh_sample.sandbox_denial_count, 0);
    assert_eq!(fresh_sample.model.as_deref(), Some("claude-haiku-4-5"));
    assert_eq!(fresh_sample.effort.as_deref(), Some("low"));
}

#[test]
fn telemetry_ring_buffers_stay_strictly_bounded() {
    let collector = TelemetryCollector::new();
    let t0 = Instant::now();
    for i in 0..(MAX_RECENT_TURNS_PER_AGENT as u64 + 25) {
        collector.record_turn_started("emp-bound", Some("claude-sonnet-4-6"), Some("low"), t0);
        collector.record_turn_finished(
            "emp-bound",
            true,
            Some(1024),
            t0 + Duration::from_millis(i),
            i,
        );
    }
    let snap = collector.snapshot_agent("emp-bound").unwrap();
    assert_eq!(snap.recent_turns.len(), MAX_RECENT_TURNS_PER_AGENT);
    assert_eq!(
        snap.total_turn_count,
        MAX_RECENT_TURNS_PER_AGENT as u64 + 25
    );
    assert_eq!(
        snap.recent_turns.first().map(|s| s.wall_latency_ms),
        Some(25)
    );

    // Distinct tracked agents must also remain bounded at MAX_TRACKED_AGENTS.
    for idx in 0..(MAX_TRACKED_AGENTS + 30) {
        collector.record_session_started(&format!("agent-{idx:04}"), Some("claude"), None, None);
    }
    let (_, all) = collector.snapshot_all();
    assert!(all.len() <= MAX_TRACKED_AGENTS, "got {}", all.len());
}
