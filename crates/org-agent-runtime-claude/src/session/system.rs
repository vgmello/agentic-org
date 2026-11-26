//! `system` frames: `init` (checked against the spec), status and the other subtypes.

use serde_json::Value;

use org_agent_runtime::{NoticeKind, Output, SessionEvent, SessionId, StartFailure};

use super::classify::{
    is_builtin_plugin, mcp_server_name, permission_mode_matches, plugin_display_name,
    remote_control_active,
};
use super::{ClaudeSession, Phase};

impl ClaudeSession {
    pub(super) fn handle_system(&mut self, val: &Value) -> Vec<Output> {
        let subtype = val.get("subtype").and_then(Value::as_str).unwrap_or("");
        let mut out = Vec::new();

        match subtype {
            "init" => {
                // Verify `init.tools` is a subset of the spec allowlist on every `system/init`.
                if let Some(tools) = val.get("tools").and_then(Value::as_array) {
                    let disallowed: Vec<String> = tools
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|name| !self.is_tool_allowed(name))
                        .map(ToOwned::to_owned)
                        .collect();
                    if !disallowed.is_empty() {
                        let detail = format!(
                            "Claude system/init exposed disallowed tools: {}",
                            disallowed.join(", ")
                        );
                        return self.fail_or_fast_stop(detail);
                    }
                }

                // Verify `init.plugins` holds only built-in plugins (S1, agent-sessions.md:168).
                if let Some(plugins) = val.get("plugins").and_then(Value::as_array) {
                    let non_builtin: Vec<String> = plugins
                        .iter()
                        .filter(|p| !is_builtin_plugin(p))
                        .map(plugin_display_name)
                        .collect();
                    if !non_builtin.is_empty() {
                        let detail = format!(
                            "Claude system/init loaded non-builtin plugins: {}",
                            non_builtin.join(", ")
                        );
                        return self.fail_or_fast_stop(detail);
                    }
                }

                // Verify `init.mcp_servers` lists only servers granted in `spec.mcp_servers` (S1).
                if let Some(servers) = val.get("mcp_servers").and_then(Value::as_array) {
                    let unlisted: Vec<String> = servers
                        .iter()
                        .filter_map(mcp_server_name)
                        .filter(|name| !self.allowed_mcp_servers.contains(name))
                        .collect();
                    if !unlisted.is_empty() {
                        let detail = format!(
                            "Claude system/init exposed unlisted MCP servers: {}",
                            unlisted.join(", ")
                        );
                        return self.fail_or_fast_stop(detail);
                    }
                }

                // Verify remote control is not active on any `system/init` (S1).
                if remote_control_active(val) {
                    return self.fail_or_fast_stop(
                        "Claude system/init reported remote control active".to_owned(),
                    );
                }

                if let Some(reported_mode) = val.get("permissionMode").and_then(Value::as_str) {
                    if reported_mode == "bypassPermissions" {
                        // Banned everywhere: fast-stop instead of emitting ModeChanged (B1).
                        if self.phase == Phase::Opening {
                            self.init_deadline = None;
                            self.phase = Phase::Failed;
                            return vec![Output::Event(SessionEvent::StartFailed {
                                reason: StartFailure::ModeMismatch {
                                    requested: self.expected_mode.reported_name.to_owned(),
                                    reported: reported_mode.to_owned(),
                                },
                            })];
                        }
                        return self.fail_or_fast_stop(
                            "Claude system/init reported banned permission mode `bypassPermissions`"
                                .to_owned(),
                        );
                    }
                    if !reported_mode.is_empty()
                        && !permission_mode_matches(&self.expected_mode, reported_mode)
                    {
                        if self.phase == Phase::Opening {
                            self.init_deadline = None;
                            self.phase = Phase::Failed;
                            return vec![Output::Event(SessionEvent::StartFailed {
                                reason: StartFailure::ModeMismatch {
                                    requested: self.expected_mode.reported_name.to_owned(),
                                    reported: reported_mode.to_owned(),
                                },
                            })];
                        }
                        out.push(Output::Event(SessionEvent::ModeChanged {
                            mode: Some(reported_mode.to_owned()),
                        }));
                    }
                }

                if let Some(raw_sid) = val.get("session_id").and_then(Value::as_str) {
                    match SessionId::try_from(raw_sid) {
                        Ok(sid) => {
                            if self.emitted_session_id.as_ref() != Some(&sid) {
                                self.emitted_session_id = Some(sid.clone());
                                out.push(Output::Event(SessionEvent::SessionStarted {
                                    runtime_session_id: sid,
                                }));
                            }
                        }
                        Err(_) => {
                            out.push(Output::Event(SessionEvent::ProtocolError {
                                detail: "Claude system/init carried an invalid session_id"
                                    .to_owned(),
                            }));
                        }
                    }
                }
                // Unsolicited turn started by Claude itself (e.g. background bash/sub-agent
                // completing after the previous turn's result; ADR-0025 decision 6).
                if self.phase == Phase::Idle {
                    // Flush any idle-time background sub-agent usage before resetting (S3).
                    if let Some(u) = self.flush_open_usage() {
                        out.push(u);
                    }
                    self.phase = Phase::Busy;
                    self.turn_open = true;
                    self.reset_turn_usage();
                    out.push(Output::Event(SessionEvent::TurnStarted {
                        turn_id: None,
                        solicited: false,
                    }));
                    out.push(Output::Event(SessionEvent::Status(self.busy_status())));
                }
            }
            "task_notification" | "task_started" | "task_updated" => {
                let text = val
                    .get("summary")
                    .or_else(|| val.get("message"))
                    .and_then(Value::as_str)
                    .map(|s| format!("{subtype}: {s}"))
                    .unwrap_or_else(|| subtype.to_owned());
                out.push(Output::Event(SessionEvent::Notice {
                    kind: NoticeKind::Warning,
                    text,
                }));
            }
            "commands_changed" | "background_tasks_changed" | "status" | "thinking_tokens" => {}
            other => {
                out.push(Output::Event(SessionEvent::Unknown {
                    type_name: format!("system/{other}"),
                }));
            }
        }
        out
    }
}
