//! CLI runtime auto-detection diagnostic check for `agentic-org doctor` (`TASK-271`).

use org_proto::runtimes::DetectedCliRuntime;

use super::doctor_types::DoctorCheck;

/// Probes the Top-6 supported local CLI runtimes (`claude`, `codex`, `agy`, `gemini`, `kimi`, `pi`)
/// and records a pass/warn check without spawning any external process.
pub(super) fn check_cli_runtimes(checks: &mut Vec<DoctorCheck>) {
    let detected = org_host_setup::detect_host_runtimes(None);
    checks.push(build_cli_runtimes_check(&detected));
}

pub(super) fn build_cli_runtimes_check(detected: &[DetectedCliRuntime]) -> DoctorCheck {
    let summary = org_host_setup::format_detected_runtimes_summary(detected);
    let installed_count = detected.iter().filter(|d| d.installed).count();
    if installed_count > 0 {
        let missing: Vec<&str> = detected
            .iter()
            .filter(|d| !d.installed)
            .map(|d| d.runtime_id.as_str())
            .collect();
        let message = if missing.is_empty() {
            format!("Detected local CLI runtimes: {summary}.")
        } else {
            format!(
                "Detected local CLI runtimes: {summary} (not installed: {}).",
                missing.join(", ")
            )
        };
        DoctorCheck::pass("runtime.clis", "runtime", message, None)
    } else {
        DoctorCheck::warn(
            "runtime.clis",
            "runtime",
            format!("Local CLI runtimes: {summary}."),
            Some(
                "Install at least one supported CLI (claude, codex, agy, gemini, kimi, or pi-acp) and run `agentic-org provider detect`."
                    .to_owned(),
            ),
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::doctor_types::CheckStatus;
    use super::*;

    fn sample(id: &str, installed: bool) -> DetectedCliRuntime {
        DetectedCliRuntime {
            runtime_id: id.to_owned(),
            display_name: id.to_owned(),
            binary_name: id.to_owned(),
            protocol: "acp".to_owned(),
            provider_type: "google".to_owned(),
            installed,
            binary_path: installed.then(|| format!("/usr/local/bin/{id}")),
        }
    }

    #[test]
    fn doctor_cli_runtime_check_reports_pass_when_some_installed_and_warn_when_none() {
        let partial = vec![
            sample("claude", true),
            sample("codex", false),
            sample("agy", true),
            sample("gemini", true),
            sample("kimi", false),
            sample("pi", false),
        ];
        let pass = build_cli_runtimes_check(&partial);
        assert_eq!(pass.id, "runtime.clis");
        assert_eq!(pass.status, CheckStatus::Pass);
        assert!(pass.message.contains("3/6 installed (claude, agy, gemini)"));
        assert!(pass.message.contains("not installed: codex, kimi, pi"));

        let none = vec![sample("claude", false), sample("gemini", false)];
        let warn = build_cli_runtimes_check(&none);
        assert_eq!(warn.status, CheckStatus::Warn);
        assert!(warn
            .remediation
            .as_deref()
            .unwrap()
            .contains("provider detect"));
    }
}
