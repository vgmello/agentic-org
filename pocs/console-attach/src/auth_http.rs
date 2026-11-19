use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::agent::PermissionPreset;
use crate::auth_login::{AuthManager, AuthProvider, StartLoginOptions};

#[derive(Debug, Clone)]
pub struct AuthHttpConfig {
    pub port: u16,
    /// When `Some`, isolates CLI credentials into `<base>/<provider>` so real user credentials stay untouched.
    pub isolated_config_root: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct StartBody {
    provider: AuthProvider,
}

#[derive(Debug, Deserialize)]
struct SubmitBody {
    login_id: String,
    code: String,
}

#[derive(Debug, Deserialize)]
struct CancelBody {
    login_id: String,
}

/// Runs a localhost-only HTTP API + SSO-like demo page for Claude & Codex login.
pub fn serve_auth_ui(config: AuthHttpConfig) -> io::Result<()> {
    let addr = format!("127.0.0.1:{}", config.port);
    let listener = TcpListener::bind(&addr)?;
    let manager = Arc::new(AuthManager::new());
    let config = Arc::new(config);

    eprintln!("console-attach auth-ui listening on http://{addr}");
    if let Some(root) = &config.isolated_config_root {
        eprintln!(
            "  credential storage: ISOLATED ({}) — your real ~/.claude and ~/.codex are untouched",
            root.display()
        );
    } else {
        eprintln!("  credential storage: DEFAULT user CLI config (~/.claude / ~/.codex)");
    }

    for stream_res in listener.incoming() {
        let Ok(stream) = stream_res else {
            continue;
        };
        let mgr = Arc::clone(&manager);
        let cfg = Arc::clone(&config);
        let _ = thread::Builder::new()
            .name("auth-http-conn".to_string())
            .spawn(move || {
                let _ = handle_http_connection(stream, &mgr, &cfg);
            });
    }
    Ok(())
}

fn handle_http_connection(
    mut stream: TcpStream,
    manager: &AuthManager,
    config: &AuthHttpConfig,
) -> io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut req_line = String::new();
    if reader.read_line(&mut req_line)? == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = req_line.split_whitespace().collect();
    if parts.len() < 2 {
        return write_json(&mut stream, 400, &json!({ "error": "malformed request" }));
    }
    let method = parts[0];
    let target = parts[1];

    let mut content_length: usize = 0;
    loop {
        let mut hline = String::new();
        let n = reader.read_line(&mut hline)?;
        if n == 0 || hline == "\r\n" || hline == "\n" {
            break;
        }
        if let Some((k, v)) = hline.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }

    if content_length > 64 * 1024 {
        return write_json(&mut stream, 413, &json!({ "error": "body too large" }));
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (target, None),
    };

    match (method, path) {
        ("GET", "/") => write_html(&mut stream, 200, DEMO_UI_HTML),
        ("GET", "/api/presets") => write_json(&mut stream, 200, &PermissionPreset::all_info()),
        ("POST", "/api/auth/start") => {
            let parsed: StartBody = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(err) => {
                    return write_json(
                        &mut stream,
                        400,
                        &json!({ "error": format!("invalid JSON: {err}") }),
                    );
                }
            };
            let mut opts = StartLoginOptions::for_provider(parsed.provider);
            if let Some(root) = &config.isolated_config_root {
                let sub = match parsed.provider {
                    AuthProvider::Claude => "claude",
                    AuthProvider::ClaudeToken => "claude-token",
                    AuthProvider::Codex => "codex",
                };
                opts.config_dir = Some(root.join(sub));
            }
            match manager.start_login(opts) {
                Ok(snap) => write_json(&mut stream, 200, &snap),
                Err(err) => write_json(&mut stream, 500, &json!({ "error": err.to_string() })),
            }
        }
        ("POST", "/api/auth/submit") => {
            let parsed: SubmitBody = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(err) => {
                    return write_json(
                        &mut stream,
                        400,
                        &json!({ "error": format!("invalid JSON: {err}") }),
                    );
                }
            };
            match manager.submit_code(&parsed.login_id, &parsed.code) {
                Ok(snap) => write_json(&mut stream, 200, &snap),
                Err(err) => write_json(&mut stream, 400, &json!({ "error": err.to_string() })),
            }
        }
        ("GET", "/api/auth/status") => {
            let login_id = query.and_then(|q| {
                q.split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .find(|(k, _)| *k == "id")
                    .map(|(_, v)| v.to_string())
            });
            let Some(id) = login_id else {
                return write_json(
                    &mut stream,
                    400,
                    &json!({ "error": "missing ?id=<login_id>" }),
                );
            };
            match manager.status(&id) {
                Ok(snap) => write_json(&mut stream, 200, &snap),
                Err(err) => write_json(&mut stream, 404, &json!({ "error": err.to_string() })),
            }
        }
        ("POST", "/api/auth/cancel") => {
            let parsed: CancelBody = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(err) => {
                    return write_json(
                        &mut stream,
                        400,
                        &json!({ "error": format!("invalid JSON: {err}") }),
                    );
                }
            };
            match manager.cancel(&parsed.login_id) {
                Ok(snap) => write_json(&mut stream, 200, &snap),
                Err(err) => write_json(&mut stream, 404, &json!({ "error": err.to_string() })),
            }
        }
        _ => write_json(&mut stream, 404, &json!({ "error": "not found" })),
    }
}

fn write_json<T: serde::Serialize>(
    stream: &mut TcpStream,
    status: u16,
    payload: &T,
) -> io::Result<()> {
    let body =
        serde_json::to_vec(payload).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let reason = status_reason(status);
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}

fn write_html(stream: &mut TcpStream, status: u16, html: &str) -> io::Result<()> {
    let body = html.as_bytes();
    let reason = status_reason(status);
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn status_reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Internal Server Error",
    }
}

const DEMO_UI_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <title>Agentic CLI SSO-Like Login POC</title>
  <style>
    :root { color-scheme: dark; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
    body { max-width: 720px; margin: 40px auto; padding: 0 20px; background: #0f1115; color: #e6eaf2; }
    h1 { font-size: 1.5rem; margin-bottom: 0.4rem; }
    p.sub { color: #9aa4b2; margin-top: 0; }
    .row { display: flex; gap: 12px; margin: 20px 0; flex-wrap: wrap; }
    button {
      padding: 10px 18px; border-radius: 8px; border: 1px solid #2b3240;
      background: #1d2432; color: #fff; font-weight: 600; cursor: pointer;
    }
    button:hover { background: #293347; }
    button.primary { background: #4f46e5; border-color: #6366f1; }
    button.primary:hover { background: #4338ca; }
    button.danger { background: #7f1d1d; border-color: #991b1b; }
    .card {
      background: #161a22; border: 1px solid #252b37; border-radius: 12px;
      padding: 18px; margin-top: 16px; display: none;
    }
    .card.visible { display: block; }
    .badge {
      display: inline-block; padding: 3px 10px; border-radius: 999px;
      font-size: 0.8rem; font-weight: 600; background: #263042; color: #cbd5e1;
    }
    .badge.ok { background: #14532d; color: #86efac; }
    .badge.err { background: #7f1d1d; color: #fecaca; }
    .codebox {
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 1.6rem; letter-spacing: 0.14em; padding: 12px 16px;
      background: #0d1016; border: 1px solid #2d3648; border-radius: 8px;
      display: inline-block; margin: 10px 0;
    }
    input[type="text"] {
      width: 100%; box-sizing: border-box; padding: 10px 12px; border-radius: 8px;
      border: 1px solid #2e3748; background: #0d1016; color: #fff;
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace; margin: 10px 0;
    }
    a.oauth-link { color: #93c5fd; word-break: break-all; font-size: 0.85rem; }
    pre {
      background: #0b0d12; border: 1px solid #202632; border-radius: 8px;
      padding: 12px; overflow-x: auto; font-size: 0.8rem; color: #a5b4fc; max-height: 220px;
    }
  </style>
</head>
<body>
  <h1>CLI SSO-Like Login POC (Rust PTY Backend)</h1>
  <p class="sub">Starts <code>claude auth login --claudeai</code> or <code>codex login --device-auth</code> headlessly in a PTY, extracts the OAuth URL / device code, and completes login without a local browser on the server.</p>

  <div class="row">
    <button class="primary" onclick="startLogin('claude_token')">Generate 1-Year Claude Token (Remote Offices)</button>
    <button onclick="startLogin('claude')">Sign in with Claude (Local)</button>
    <button class="primary" onclick="startLogin('codex')">Sign in with Codex</button>
  </div>

  <div id="panel" class="card">
    <div style="display:flex;justify-content:space-between;align-items:center;">
      <strong id="title">Login</strong>
      <span id="stateBadge" class="badge">starting</span>
    </div>
    <p id="message" class="sub" style="margin:10px 0;"></p>

    <div id="urlBlock" style="display:none;margin:12px 0;">
      <div class="row" style="margin:8px 0;">
        <button id="openUrlBtn" class="primary">Open Sign-In Page</button>
        <button class="danger" onclick="cancelLogin()">Cancel</button>
      </div>
      <a id="urlLink" class="oauth-link" target="_blank" rel="noopener noreferrer"></a>
    </div>

    <div id="deviceBlock" style="display:none;margin:12px 0;">
      <div>Enter this one-time code on the OpenAI page:</div>
      <div class="codebox" id="deviceCode"></div>
      <button onclick="copyDeviceCode()">Copy Code</button>
    </div>

    <div id="pasteBlock" style="display:none;margin:12px 0;">
      <label for="codeInput">Paste the authorization code returned by Claude:</label>
      <input id="codeInput" type="text" placeholder="Paste code here..." autocomplete="off" />
      <button class="primary" onclick="submitCode()">Submit Code</button>
    </div>

    <div id="tokenBlock" style="display:none;margin:12px 0;">
      <div style="color:#86efac;font-weight:600;margin-bottom:6px;">Generated 1-Year Claude OAuth Token (store securely for remote offices):</div>
      <input id="oauthTokenOut" type="text" readonly />
      <button class="primary" onclick="copyOAuthToken()">Copy Token</button>
    </div>

    <details style="margin-top:16px;">
      <summary style="cursor:pointer;color:#9aa4b2;">Raw intercepted PTY output</summary>
      <pre id="ptyOut"></pre>
    </details>
  </div>

  <details class="card visible" style="margin-top:20px;">
    <summary style="cursor:pointer;font-weight:600;">Advanced Settings — Sandbox &amp; Permission Mode (Secure by Default)</summary>
    <p class="sub" style="margin:10px 0 12px;">
      Sessions run with a workspace OS sandbox and interactive approval routing by default. Override only if you understand the risk.
    </p>
    <div id="presetList" style="display:grid;gap:10px;"></div>
    <label style="display:flex;align-items:center;gap:8px;margin-top:14px;cursor:pointer;">
      <input id="sandboxToggle" type="checkbox" checked onchange="renderPresetSummary()" />
      <span>Enable OS workspace sandbox (macOS Seatbelt / Linux bubblewrap)</span>
    </label>
    <pre id="presetSummary" style="margin-top:12px;"></pre>
  </details>

  <script>
    let currentLoginId = null;
    let pollTimer = null;
    let openedUrlFor = null;
    let presets = [];
    let selectedPresetId = 'safe_ask';

    async function loadPresets() {
      try {
        const res = await fetch('/api/presets');
        if (!res.ok) return;
        presets = await res.json();
        const list = document.getElementById('presetList');
        list.innerHTML = '';
        for (const p of presets) {
          const el = document.createElement('label');
          el.style.cssText = 'display:block;padding:10px 12px;border-radius:8px;border:1px solid ' + (p.dangerous ? '#7f1d1d' : '#252b37') + ';background:#0f131b;cursor:pointer;';
          el.innerHTML =
            '<div style="display:flex;align-items:center;gap:8px;">' +
            '<input type="radio" name="preset" value="' + p.id + '"' + (p.id === selectedPresetId ? ' checked' : '') + ' />' +
            '<strong>' + p.label + '</strong>' +
            (p.dangerous ? ' <span class="badge err">High Risk</span>' : '') +
            '</div>' +
            '<div style="color:#9aa4b2;font-size:0.85rem;margin-top:4px;margin-left:22px;">' + p.description + '</div>';
          el.querySelector('input').onchange = () => {
            selectedPresetId = p.id;
            document.getElementById('sandboxToggle').checked = p.sandbox_default;
            renderPresetSummary();
          };
          list.appendChild(el);
        }
        renderPresetSummary();
      } catch (_) {}
    }

    function renderPresetSummary() {
      const p = presets.find(x => x.id === selectedPresetId);
      if (!p) return;
      const sandbox = document.getElementById('sandboxToggle').checked;
      document.getElementById('presetSummary').textContent =
        'Selected preset : ' + p.id + ' (sandbox=' + (sandbox ? 'on' : 'off') + ')\n' +
        'Claude CLI      : --preset ' + p.id + ' --sandbox ' + (sandbox ? 'on' : 'off') + '  (permission-mode=' + p.claude_permission_mode + ')\n' +
        'Codex CLI       : ' + p.codex_flags.join(' ');
    }
    loadPresets();

    async function startLogin(provider) {
      stopPolling();
      openedUrlFor = null;
      const panel = document.getElementById('panel');
      panel.classList.add('visible');
      document.getElementById('title').textContent =
        provider === 'claude_token' ? 'Generate 1-Year Claude OAuth Token (Remote Offices)'
        : provider === 'claude' ? 'Claude Subscription Sign-In'
        : 'Codex Subscription Sign-In';
      document.getElementById('stateBadge').textContent = 'starting...';
      document.getElementById('stateBadge').className = 'badge';
      document.getElementById('message').textContent = 'Launching headless CLI inside Rust PTY...';
      document.getElementById('urlBlock').style.display = 'none';
      document.getElementById('deviceBlock').style.display = 'none';
      document.getElementById('pasteBlock').style.display = 'none';
      document.getElementById('tokenBlock').style.display = 'none';
      document.getElementById('codeInput').value = '';
      document.getElementById('ptyOut').textContent = '';

      const res = await fetch('/api/auth/start', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ provider })
      });
      const data = await res.json();
      if (!res.ok) {
        document.getElementById('stateBadge').textContent = 'error';
        document.getElementById('stateBadge').className = 'badge err';
        document.getElementById('message').textContent = data.error || 'Failed to start login';
        return;
      }
      currentLoginId = data.login_id;
      renderSnapshot(data);
      startPolling();
    }

    function renderSnapshot(s) {
      const badge = document.getElementById('stateBadge');
      badge.textContent = s.state;
      badge.className = 'badge' + (s.state === 'authenticated' ? ' ok' : (s.state === 'failed' || s.state === 'cancelled' ? ' err' : ''));
      document.getElementById('message').textContent = s.message || '';
      document.getElementById('ptyOut').textContent = s.output_preview || '';

      if (s.url) {
        document.getElementById('urlBlock').style.display = 'block';
        const link = document.getElementById('urlLink');
        link.href = s.url;
        link.textContent = s.url;
        document.getElementById('openUrlBtn').onclick = () => window.open(s.url, '_blank', 'noopener,noreferrer');
        if (openedUrlFor !== s.login_id) {
          openedUrlFor = s.login_id;
          window.open(s.url, '_blank', 'noopener,noreferrer');
        }
      }

      if (s.flow === 'paste_code' && (s.state === 'awaiting_code' || s.state === 'submitting')) {
        document.getElementById('pasteBlock').style.display = 'block';
      } else {
        document.getElementById('pasteBlock').style.display = 'none';
      }

      if (s.flow === 'device_code' && s.user_code && s.state === 'awaiting_device_auth') {
        document.getElementById('deviceBlock').style.display = 'block';
        document.getElementById('deviceCode').textContent = s.user_code;
      } else if (s.state !== 'awaiting_device_auth') {
        document.getElementById('deviceBlock').style.display = 'none';
      }

      if (s.oauth_token) {
        document.getElementById('tokenBlock').style.display = 'block';
        document.getElementById('oauthTokenOut').value = s.oauth_token;
      }

      if (s.state === 'authenticated' || s.state === 'failed' || s.state === 'cancelled') {
        stopPolling();
      }
    }

    function copyOAuthToken() {
      const text = document.getElementById('oauthTokenOut').value;
      if (text && navigator.clipboard) navigator.clipboard.writeText(text);
    }

    async function submitCode() {
      if (!currentLoginId) return;
      const code = document.getElementById('codeInput').value.trim();
      if (!code) return;
      const res = await fetch('/api/auth/submit', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ login_id: currentLoginId, code })
      });
      const data = await res.json();
      if (!res.ok) {
        document.getElementById('message').textContent = data.error || 'Failed submitting code';
        return;
      }
      renderSnapshot(data);
    }

    async function cancelLogin() {
      if (!currentLoginId) return;
      const res = await fetch('/api/auth/cancel', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ login_id: currentLoginId })
      });
      if (res.ok) renderSnapshot(await res.json());
    }

    function copyDeviceCode() {
      const text = document.getElementById('deviceCode').textContent;
      if (text && navigator.clipboard) navigator.clipboard.writeText(text);
    }

    function startPolling() {
      stopPolling();
      pollTimer = setInterval(async () => {
        if (!currentLoginId) return;
        const res = await fetch('/api/auth/status?id=' + encodeURIComponent(currentLoginId));
        if (res.ok) renderSnapshot(await res.json());
      }, 1000);
    }

    function stopPolling() {
      if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
    }
  </script>
</body>
</html>
"#;
