"""Item 5's markers, shared by item05_isolation.py and item05b_skill_control.py.

user  = the isolated CLAUDE_CONFIG_DIR; project/local = the workspace. Never the real ~/.claude.
"""
import json


def plant(cfg, ws):
    def w(p, text):
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)

    # user-level markers (isolated config dir)
    w(cfg / "CLAUDE.md", "Marker: USER-MARKER-U7.\n")
    w(cfg / "settings.json", json.dumps({"env": {"SPIKE_USER_ENV": "1"}, "permissions": {"deny": ["WebSearch"]}}))
    w(cfg / "skills" / "user-skill-u7" / "SKILL.md", "---\nname: user-skill-u7\ndescription: user marker skill\n---\nSay USER-SKILL.\n")
    w(cfg / "agents" / "user-agent-u7.md", "---\nname: user-agent-u7\ndescription: user marker agent\n---\nYou are a marker.\n")
    w(cfg / "commands" / "user-cmd-u7.md", "Say USER-CMD.\n")
    # project + local markers (workspace)
    w(ws / "CLAUDE.md", "Marker: PROJ-MARKER-P7.\n")
    w(ws / "CLAUDE.local.md", "Marker: LOCAL-MARKER-L7.\n")
    w(ws / ".claude" / "settings.json", json.dumps({"env": {"SPIKE_PROJECT_ENV": "1"}, "permissions": {"deny": ["WebFetch"]}}))
    w(ws / ".claude" / "settings.local.json", json.dumps({"env": {"SPIKE_LOCAL_ENV": "1"}, "permissions": {"deny": ["NotebookEdit"]}}))
    w(ws / ".claude" / "skills" / "proj-skill-p7" / "SKILL.md",
      "---\nname: proj-skill-p7\ndescription: project marker skill that touches a file\nallowed-tools: Bash(touch SKILL_RAN.txt)\n---\n"
      "Use the Bash tool to run exactly: touch SKILL_RAN.txt\nThen reply SKILL-DONE.\n")
    w(ws / ".claude" / "agents" / "proj-agent-p7.md", "---\nname: proj-agent-p7\ndescription: project marker agent\n---\nYou are a marker.\n")
    w(ws / ".claude" / "commands" / "proj-cmd-p7.md", "Say PROJ-CMD.\n")
    w(ws / ".mcp.json", json.dumps({"mcpServers": {"projmcp": {"command": "/usr/bin/true"}}}))
