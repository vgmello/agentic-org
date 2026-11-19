// Disposable feasibility evidence, not an adapter or a production readiness test.
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, realpathSync, rmSync, existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import assert from 'node:assert/strict';

const binary = '/Users/vgmello/.codex/packages/standalone/releases/0.169.0-aarch64-apple-darwin/bin/codex';
const fixtureRoot = realpathSync(mkdtempSync('/private/tmp/ao-config-probe-'));
const fixtureHome = join(fixtureRoot, 'home');
const fixtureCodex = join(fixtureHome, '.codex');
const fixtureWorkspace = join(fixtureRoot, 'workspace');
const fixtureScratch = join(fixtureRoot, 'scratch');
for (const path of [fixtureCodex, join(fixtureWorkspace, '.codex'), fixtureScratch]) {
  mkdirSync(path, { recursive: true, mode: 0o700 });
}
const fixtureEnv = {
  PATH: '/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin', HOME: fixtureHome,
  CODEX_HOME: fixtureCodex, TMPDIR: fixtureScratch, SHELL: '/bin/sh', LANG: 'en_US.UTF-8',
  GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null',
};
const gitBinary = execFileSync('/usr/bin/xcrun', ['--find', 'git'], { encoding: 'utf8', timeout: 10000 }).trim();
execFileSync(gitBinary, ['init', '-q', fixtureWorkspace], { env: fixtureEnv, timeout: 10000 });
writeFileSync(join(fixtureWorkspace, '.codex', 'config.toml'), [
  'developer_instructions = "WORKSPACE_CANARY"',
  '[mcp_servers.project_canary]',
  'command = "/usr/bin/false"',
  '',
].join('\n'), { mode: 0o600 });

async function inspect(name, trust, overrides, homeCanary = true, rootProbe = false) {
  writeFileSync(join(fixtureCodex, 'config.toml'), [
    ...(homeCanary ? ['[mcp_servers.home_canary]', 'command = "/usr/bin/false"'] : []),
    `[projects.${JSON.stringify(fixtureWorkspace)}]`,
    `trust_level = ${JSON.stringify(trust)}`,
    '',
  ].join('\n'), { mode: 0o600 });
  const child = spawn(binary, ['app-server', '--stdio', '-c', 'features.hooks=false', ...overrides], {
    env: fixtureEnv, cwd: fixtureWorkspace, stdio: ['pipe', 'pipe', 'pipe'],
  });
  const pending = new Map();
  let sequence = 0;
  let stderrBytes = 0;
  let stderrTail = '';
  child.stderr.on('data', bytes => { stderrBytes += bytes.length; stderrTail = (stderrTail + bytes.toString()).slice(-8192); });
  const fail = error => {
    for (const waiter of pending.values()) { clearTimeout(waiter.timer); waiter.reject(error); }
    pending.clear();
  };
  child.on('error', fail);
  child.stdin.on('error', fail);
  child.on('exit', (code, signal) => fail(new Error(`app-server ended: ${code ?? signal}; ${stderrTail.trim()}`)));
  const lines = createInterface({ input: child.stdout });
  lines.on('line', line => {
    let frame;
    try { frame = JSON.parse(line); } catch { fail(new Error('non-JSON stdout')); return; }
    if (!('id' in frame)) return;
    if (frame.method) {
      child.stdin.write(JSON.stringify({ id: frame.id, error: { code: -32601, message: 'No action approved in fixture' } }) + '\n');
      return;
    }
    const waiter = pending.get(frame.id);
    if (!waiter) return;
    clearTimeout(waiter.timer);
    pending.delete(frame.id);
    if (frame.error) waiter.reject(new Error(frame.error.message));
    else waiter.resolve(frame.result);
  });
  const request = (method, params) => {
    const id = ++sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 15000);
      pending.set(id, { resolve, reject, timer });
      child.stdin.write(JSON.stringify({ id, method, params }) + '\n');
    });
  };
  try {
    await request('initialize', { clientInfo: { name: 'ao-config-fixture', version: '0.1.0' }, capabilities: { experimentalApi: true } });
    child.stdin.write(JSON.stringify({ method: 'initialized', params: {} }) + '\n');
    const result = await request('config/read', { cwd: fixtureWorkspace, includeLayers: false });
    const config = result.config;
    if (!config || typeof config !== 'object') throw new Error('Missing effective config');
    const servers = config.mcp_servers ?? config.mcpServers;
    const rootWrites = [];
    if (rootProbe) {
      for (const [label, destination] of [
        ['workspace', join(fixtureWorkspace, 'root-check.txt')],
        ['scratch', join(fixtureScratch, 'root-check.txt')],
        ['outside-explicit-roots', join(fixtureRoot, 'outside-roots.txt')],
      ]) {
        rmSync(destination, { force: true });
        const executed = await request('command/exec', {
          command: ['/bin/sh', '-c', 'printf synthetic-fixture > "$1"', 'fixture', destination],
          cwd: fixtureWorkspace, permissionProfile: 'ao_exact_probe',
          timeoutMs: 5000, outputBytesCap: 4096,
        });
        const created = existsSync(destination);
        rootWrites.push({ label, exitCode: executed.exitCode, created,
          contentMatches: created && readFileSync(destination, 'utf8') === 'synthetic-fixture',
          stdout: executed.stdout, stderr: executed.stderr });
      }
    }
    return {
      name, trust, overrides, configKeys: Object.keys(config),
      mcpServers: servers,
      developerInstructions: config.developer_instructions ?? config.developerInstructions ?? null,
      hooks: config.features?.hooks ?? null,
      ...(rootProbe ? { rootWrites } : {}),
      stderrBytes,
    };
  } finally {
    const exited = new Promise(resolve => {
      if (child.exitCode !== null || child.signalCode !== null) { resolve(); return; }
      child.once('exit', resolve);
    });
    child.stdin.end();
    const termTimer = setTimeout(() => child.kill('SIGTERM'), 2000);
    const killTimer = setTimeout(() => child.kill('SIGKILL'), 4000);
    await exited;
    clearTimeout(termTimer); clearTimeout(killTimer); lines.close();
  }
}

const results = [];
try {
  results.push(await inspect('trusted-baseline', 'trusted', []));
  results.push(await inspect('trusted-empty-mcp-override', 'trusted', ['-c', 'mcp_servers={}']));
  results.push(await inspect('untrusted-empty-mcp-override', 'untrusted', ['-c', 'mcp_servers={}']));
  results.push(await inspect('trusted-explicit-disable', 'trusted', [
    '-c', 'mcp_servers.home_canary.enabled=false',
    '-c', 'mcp_servers.project_canary.enabled=false',
  ]));
  results.push(await inspect('trusted-cli-untrusted-override', 'trusted', [
    '-c', `projects={${JSON.stringify(fixtureWorkspace)}={trust_level="untrusted"}}`,
    '-c', 'mcp_servers={}',
  ]));
  results.push(await inspect('controlled-empty-home-cli-untrusted', 'trusted', [
    '-c', `projects={${JSON.stringify(fixtureWorkspace)}={trust_level="untrusted"}}`,
    '-c', 'mcp_servers={}',
  ], false));
  for (const extendsWorkspace of [true, false]) {
    const filesystem = [
      ...(!extendsWorkspace ? ['"/"="read"'] : []),
      `${JSON.stringify(fixtureWorkspace)}="write"`,
      `${JSON.stringify(fixtureScratch)}="write"`,
    ].join(',');
    const overrides = [
      '-c', `projects={${JSON.stringify(fixtureWorkspace)}={trust_level="untrusted"}}`,
      '-c', `permissions={ao_exact_probe={${extendsWorkspace ? 'extends=":workspace",' : ''}filesystem={${filesystem}}}}`,
      '-c', 'default_permissions="ao_exact_probe"',
    ];
    const name = extendsWorkspace ? 'workspace-profile-explicit-roots' : 'custom-profile-explicit-roots';
    try { results.push(await inspect(name, 'trusted', overrides, false, true)); }
    catch (error) { results.push({ name, error: error.message }); }
  }
  const byName = Object.fromEntries(results.map(r => [r.name, r]));
  for (const name of ['trusted-baseline', 'trusted-empty-mcp-override']) {
    assert.equal(byName[name].mcpServers?.home_canary?.enabled, true);
    assert.equal(byName[name].mcpServers?.project_canary?.enabled, true);
    assert.equal(byName[name].developerInstructions, 'WORKSPACE_CANARY');
  }
  for (const name of ['untrusted-empty-mcp-override', 'trusted-cli-untrusted-override']) {
    assert.equal(byName[name].mcpServers?.home_canary?.enabled, true);
    assert.equal(byName[name].mcpServers?.project_canary, undefined);
    assert.equal(byName[name].developerInstructions, null);
  }
  assert.equal(byName['trusted-explicit-disable'].mcpServers?.home_canary?.enabled, false);
  assert.equal(byName['trusted-explicit-disable'].mcpServers?.project_canary?.enabled, false);
  assert.deepEqual(Object.keys(byName['controlled-empty-home-cli-untrusted'].mcpServers ?? {}), []);
  assert.equal(byName['controlled-empty-home-cli-untrusted'].developerInstructions, null);
  for (const result of results.filter(r => !r.error)) assert.equal(result.hooks, false);
  for (const name of ['workspace-profile-explicit-roots', 'custom-profile-explicit-roots']) {
    assert.equal(byName[name].error, undefined);
    for (const check of byName[name].rootWrites.filter(r => r.label !== 'outside-explicit-roots')) {
      assert.equal(check.exitCode, 0); assert.equal(check.created, true); assert.equal(check.contentMatches, true);
    }
  }
  const inheritedOutside = byName['workspace-profile-explicit-roots'].rootWrites.find(r => r.label === 'outside-explicit-roots');
  assert.equal(inheritedOutside.exitCode, 0); assert.equal(inheritedOutside.created, true);
  const customOutside = byName['custom-profile-explicit-roots'].rootWrites.find(r => r.label === 'outside-explicit-roots');
  assert.notEqual(customOutside.exitCode, 0); assert.equal(customOutside.created, false);
  const artifact = {
    version: execFileSync(binary, ['--version'], { env: fixtureEnv, encoding: 'utf8', timeout: 10000 }).trim(),
    scope: 'Synthetic home/workspace effective config only; no turn/start, auth, model, native tool or startup-side-effect claim.',
    findings: [
      'mcp_servers={} merges and preserves configured servers; it does not clear either home or trusted workspace MCP definitions.',
      'Explicit known-name enabled=false suppresses those entries in effective configuration, requiring a complete inventory.',
      'CLI untrusted-project override suppresses this project layer, while home MCP remains.',
      'Controlled empty home plus CLI untrusted-project override leaves no synthetic MCP entries in effective configuration.',
      'Standalone commands with a :workspace-derived profile can write to a synthetic sibling under /private/tmp beyond the explicit roots.',
      'A custom profile without :workspace, using / read plus exact workspace/scratch write rules and explicit default_permissions, admits those two roots and refuses creation at the same sibling path.',
      'These observations do not prove absence of native startup effects or model-tool access.',
    ],
    results,
  };
  const sanitized = JSON.stringify(artifact, null, 2).replaceAll(fixtureRoot, '<TMP>');
  writeFileSync(new URL('./config-isolation-0.169.0.json', import.meta.url), sanitized + '\n', { mode: 0o600 });
  console.log(sanitized);
} finally {
  rmSync(fixtureRoot, { recursive: true, force: true });
}
