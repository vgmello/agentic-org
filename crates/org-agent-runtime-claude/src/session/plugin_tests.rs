use super::classify::is_builtin_plugin;
use serde_json::json;

#[test]
fn accepts_claude_2_1_323_builtin_plugins_and_rejects_disk_spoofs() {
    assert!(is_builtin_plugin(&json!({
        "name": "cc-plugin-agents-md",
        "path": "builtin",
        "source": "cc-plugin-agents-md@builtin"
    })));
    assert!(is_builtin_plugin(&json!({
        "name": "legacy",
        "source": "builtin"
    })));
    assert!(!is_builtin_plugin(&json!({
        "name": "spoofed",
        "path": "/home/user/.claude/plugins/spoofed",
        "source": "spoofed@builtin"
    })));
    assert!(!is_builtin_plugin(&json!({
        "name": "user-plugin",
        "path": "/home/user/.claude/plugins/user-plugin",
        "source": "user-plugin@marketplace"
    })));
    assert!(!is_builtin_plugin(&json!({
        "name": "",
        "path": "builtin",
        "source": "@builtin"
    })));
    assert!(!is_builtin_plugin(&json!({
        "name": "evil",
        "path": "builtin",
        "source": "other@builtin"
    })));
}
