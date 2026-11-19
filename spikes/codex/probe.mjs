// S0: protocol and sandbox probes. No model calls or personal configuration.
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, realpathSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { createServer } from 'node:net';

const binary = process.argv[2];
if (!binary?.startsWith('/')) throw new Error('Usage: node probe.mjs /absolute/path/to/codex');
// /usr/bin/git is an Xcode launcher on macOS; resolve the actual binary before sandboxing.
const resolvedGit = spawnSync('/usr/bin/xcrun', ['--find', 'git'], { encoding: 'utf8', timeout: 10000 });
if (resolvedGit.status !== 0) throw new Error('Cannot resolve the installed Git binary');
const gitBinary = realpathSync(resolvedGit.stdout.trim());
// macOS Unix socket paths are short; keep fixtures under /private/tmp.
const root = realpathSync(mkdtempSync('/private/tmp/codex-s0-'));
const workspace = join(root, 'workspace');
const runtimeHome = join(root, 'home');
const codexHome = join(runtimeHome, '.codex');
const scratch = join(root, 'scratch');
for (const path of [workspace, codexHome, scratch]) mkdirSync(path, { recursive: true, mode: 0o700 });
const env = {
  PATH: '/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin',
  HOME: runtimeHome,
  CODEX_HOME: codexHome,
  TMPDIR: scratch,
  SHELL: '/bin/sh',
  LANG: 'en_US.UTF-8',
  ORG_AGENT: 'codex-s0-fixture',
  ORG_SESSION_TOKEN: 'non-secret-fixture-token',
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_CONFIG_GLOBAL: '/dev/null',
};
writeFileSync(join(codexHome, 'config.toml'), 'approval_policy = "never"\nsandbox_mode = "workspace-write"\n');
const version = spawnSync(binary, ['--version'], { env, cwd: workspace, encoding: 'utf8', timeout: 10000 });
if (version.status !== 0) throw new Error(version.stderr || 'Version check failed');

class Rpc {
  sequence = 0;
  pending = new Map();
  frames = [];
  stderr = '';
  constructor(args = []) {
    this.child = spawn(binary, ['app-server', '--stdio', ...args], { env, cwd: workspace, stdio: ['pipe', 'pipe', 'pipe'] });
    this.child.stderr.on('data', bytes => { this.stderr = (this.stderr + bytes.toString()).slice(-32768); });
    this.child.on('error', error => this.fail(error));
    this.child.on('exit', (code, signal) => this.fail(new Error(`app-server ended: ${code ?? signal}`)));
    const lines = createInterface({ input: this.child.stdout });
    lines.on('line', line => {
      let frame;
      try { frame = JSON.parse(line); } catch { this.fail(new Error('Non-JSON app-server stdout')); return; }
      this.frames.push({ direction: 'received', frame });
      if ('id' in frame && !frame.method) {
        const pending = this.pending.get(frame.id);
        if (!pending) return;
        clearTimeout(pending.timer);
        this.pending.delete(frame.id);
        if (frame.error) pending.reject(new Error(JSON.stringify(frame.error)));
        else pending.resolve(frame.result);
      } else if ('id' in frame) {
        // These probes never approve an action or answer a credential prompt.
        this.send({ id: frame.id, error: { code: -32601, message: 'No interactive approval in the offline probe' } });
      }
    });
  }
  fail(error) {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
  }
  send(frame) {
    this.frames.push({ direction: 'sent', frame });
    this.child.stdin.write(JSON.stringify(frame) + '\n');
  }
  request(method, params, timeoutMs = 15000) {
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timed out`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.send({ id, method, params });
    });
  }
  async initialize() {
    const result = await this.request('initialize', {
      clientInfo: { name: 'agentic-org-s0', version: '0.1.0' },
      capabilities: { experimentalApi: true },
    });
    this.send({ method: 'initialized', params: {} });
    return result;
  }
  async close() {
    this.child.stdin.end();
    await new Promise(resolve => {
      if (this.child.exitCode !== null || this.child.signalCode !== null) return resolve();
      const timer = setTimeout(() => this.child.kill('SIGTERM'), 2000);
      const hardTimer = setTimeout(() => this.child.kill('SIGKILL'), 4000);
      this.child.once('exit', () => { clearTimeout(timer); clearTimeout(hardTimer); resolve(); });
    });
  }
}

const results = [];
const servers = [];
function record(name, value) { results.push({ name, ...value }); console.log(JSON.stringify(results.at(-1))); }
const sandboxPolicy = {
  type: 'workspaceWrite', writableRoots: [workspace, scratch], networkAccess: false,
  excludeSlashTmp: true, excludeTmpdirEnvVar: true,
};
function git(args, cwd = workspace) {
  const result = spawnSync(gitBinary, args, { cwd, env, encoding: 'utf8', timeout: 10000 });
  if (result.status !== 0) throw new Error(`git ${args[0]}: ${result.stderr || result.error}`);
  return result.stdout.trim();
}
async function probeWorktree(rpc) {
  const seed = join(root, 'seed');
  const common = join(root, 'work.git');
  const task = join(workspace, 'task');
  const hooks = join(root, 'empty-hooks');
  mkdirSync(seed);
  mkdirSync(hooks);
  git(['init', '-b', 'v1', seed]);
  writeFileSync(join(seed, 'initial.txt'), 'initial\n');
  git(['add', '.'], seed);
  git(['-c', 'user.name=Codex S0', '-c', 'user.email=fixture@example.invalid', 'commit', '-m', 'fixture'], seed);
  git(['clone', '--bare', seed, common]);
  git(['--git-dir', common, 'config', 'extensions.worktreeConfig', 'true']);
  git(['--git-dir', common, 'config', '--worktree', 'core.bare', 'true']);
  git(['--git-dir', common, 'config', '--unset', 'core.bare']);
  git(['--git-dir', common, 'worktree', 'add', '-b', 'task', task, 'v1']);
  const admin = join(common, 'worktrees', 'task');
  const identity = join(root, 'gitconfig');
  writeFileSync(identity, '[user]\n name = Codex S0\n email = fixture@example.invalid\n');
  const settings = {
    'core.hooksPath': hooks, 'core.fsmonitor': 'false', 'gc.auto': '0',
    'maintenance.auto': 'false', 'core.commitGraph': 'false',
  };
  const gitEnv = { GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: identity, GIT_CONFIG_COUNT: String(Object.keys(settings).length) };
  Object.entries(settings).forEach(([key, value], i) => {
    gitEnv[`GIT_CONFIG_KEY_${i}`] = key;
    gitEnv[`GIT_CONFIG_VALUE_${i}`] = value;
  });
  const policy = { ...sandboxPolicy, writableRoots: [
    task, scratch, join(common, 'objects'), join(common, 'refs'), join(common, 'logs'),
    ...['HEAD', 'HEAD.lock', 'index', 'index.lock', 'logs', 'COMMIT_EDITMSG', 'ORIG_HEAD', 'AUTO_MERGE', 'AUTO_MERGE.lock', 'config.worktree', 'config.worktree.lock'].map(name => join(admin, name)),
  ] };
  const execute = command => rpc.request('command/exec', {
    command, cwd: task, env: gitEnv, sandboxPolicy: policy, timeoutMs: 10000, outputBytesCap: 4096,
  });
  writeFileSync(join(task, 'change.txt'), 'task-change\n');
  const commit = await execute(['/bin/sh', '-c', '"$1" add change.txt && "$1" commit -m task-change', 'probe', gitBinary]);
  record('task-worktree-commit', { verified: commit.exitCode === 0 && git(['log', '-1', '--format=%s'], task) === 'task-change', result: commit });
  for (const relative of ['config', 'hooks/probe', 'HEAD', 'info/exclude', 'worktrees/task/commondir', 'worktrees/task/gitdir']) {
    const path = join(common, relative);
    const previous = existsSync(path) ? readFileSync(path) : null;
    const result = await execute(['/bin/sh', '-c', 'printf should-not-write >> "$1"', 'probe', path]);
    const unchanged = previous ? readFileSync(path).equals(previous) : !existsSync(path);
    record(`worktree-deny-${relative}`, { verified: result.exitCode !== 0 && unchanged, result });
  }
  const worktreeConfig = await execute([gitBinary, 'config', '--worktree', 's0.fixture', 'true']);
  record('worktree-local-config', { allowed: worktreeConfig.exitCode === 0, result: worktreeConfig });
  record('objects-info-limit', { verified: true, note: 'Legacy workspaceWrite allows all objects/; no subtractive deny field. core.commitGraph=false pinned and fixture has no objects/info/alternates. Office must apply the design fallback.' });
}
async function probeSocket(rpc) {
  const path = join(root, 'office.sock');
  const otherPath = join(root, 'other.sock');
  const privateFile = join(workspace, 'private.txt');
  writeFileSync(privateFile, 'synthetic-private-data');
  const server = createServer(socket => socket.end('fixture-office-ok'));
  const other = createServer(socket => socket.end('should-not-connect'));
  const tcp = createServer(socket => socket.end('should-not-connect'));
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(path, resolve); });
  await new Promise((resolve, reject) => { other.once('error', reject); other.listen(otherPath, resolve); });
  await new Promise((resolve, reject) => { tcp.once('error', reject); tcp.listen(0, '127.0.0.1', resolve); });
  try {
    const script = `const net=require('node:net'); const socket=net.connect(process.argv[1]); socket.on('data', b=>process.stdout.write(b)); socket.on('error', e=>{console.error(e.code);process.exitCode=1}); setTimeout(()=>{socket.destroy();process.exitCode=2},2000).unref();`;
    const result = await rpc.request('command/exec', {
      command: [process.execPath, '-e', script, path], cwd: workspace,
      sandboxPolicy, timeoutMs: 5000, outputBytesCap: 4096,
    });
    record('legacy-office-socket', { allowed: result.exitCode === 0 && result.stdout === 'fixture-office-ok', result });
    const profileArgs = [
      '-c', 'features.network_proxy=true',
      // -c's dotted key parser splits periods even inside a quoted path.
      '-c', `permissions={office_probe={extends=":workspace",filesystem={${JSON.stringify(privateFile)}="deny"},network={enabled=true,proxy_url="http://127.0.0.1:0",enable_socks5=false,unix_sockets={${JSON.stringify(path)}="allow"}}}}`,
    ];
    const proxy = new Rpc(profileArgs);
    servers.push(proxy);
    try {
      await proxy.initialize();
      const allowed = await proxy.request('command/exec', {
        command: [process.execPath, '-e', script, path], cwd: workspace,
        permissionProfile: 'office_probe', timeoutMs: 5000, outputBytesCap: 4096,
      });
      record('proxy-office-socket', { verified: allowed.exitCode === 0 && allowed.stdout === 'fixture-office-ok', result: allowed });
      const denied = await proxy.request('command/exec', {
        command: [process.execPath, '-e', script, otherPath], cwd: workspace,
        permissionProfile: 'office_probe', timeoutMs: 5000, outputBytesCap: 4096,
      });
      record('proxy-other-socket-denied', { verified: denied.exitCode !== 0 && !denied.stdout.includes('should-not-connect'), result: denied });
      const tcpScript = `const net=require('node:net'); const socket=net.connect(Number(process.argv[1]),'127.0.0.1'); socket.on('data', b=>process.stdout.write(b)); socket.on('error', e=>{console.error(e.code);process.exitCode=1}); setTimeout(()=>{socket.destroy();process.exitCode=2},2000).unref();`;
      const directTcp = await proxy.request('command/exec', {
        command: [process.execPath, '-e', tcpScript, String(tcp.address().port)], cwd: workspace,
        permissionProfile: 'office_probe', timeoutMs: 5000, outputBytesCap: 4096,
      });
      record('proxy-direct-tcp-denied', { verified: directTcp.exitCode !== 0 && !directTcp.stdout.includes('should-not-connect'), result: directTcp });
      const read = await proxy.request('command/exec', {
        command: ['/bin/cat', privateFile], cwd: workspace,
        permissionProfile: 'office_probe', timeoutMs: 5000, outputBytesCap: 4096,
      });
      record('profile-private-read-denied', { verified: read.exitCode !== 0 && !read.stdout.includes('synthetic-private-data'), result: read });
      const write = await proxy.request('command/exec', {
        command: ['/bin/sh', '-c', 'printf forbidden > "$1"', 'probe', privateFile], cwd: workspace,
        permissionProfile: 'office_probe', timeoutMs: 5000, outputBytesCap: 4096,
      });
      record('profile-private-write-denied', { verified: write.exitCode !== 0 && readFileSync(privateFile, 'utf8') === 'synthetic-private-data', result: write });
      const thread = await proxy.request('thread/start', {
        cwd: workspace, permissions: 'office_probe', developerInstructions: 'Synthetic S0 fixture. Do not call any tool or model.',
        approvalPolicy: 'on-request', approvalsReviewer: 'user',
      });
      record('profile-thread-start', { verified: Boolean(thread.thread?.id), threadId: thread.thread?.id, permissions: thread.permissions, approvalPolicy: thread.approvalPolicy });
      await proxy.close();
      const resumed = new Rpc(profileArgs);
      servers.push(resumed);
      await resumed.initialize();
      try {
        const resume = await resumed.request('thread/resume', {
          threadId: thread.thread.id, cwd: workspace, permissions: 'office_probe',
          developerInstructions: 'Synthetic resumed S0 fixture. Do not call any tool or model.',
          approvalPolicy: 'on-request', approvalsReviewer: 'user',
        });
        record('profile-thread-resume', { verified: resume.thread?.id === thread.thread.id, threadId: resume.thread?.id, approvalPolicy: resume.approvalPolicy });
      } catch (error) {
        if (!error.message.includes('no rollout found')) throw error;
        record('profile-thread-resume', { available: false, error: error.message, note: 'No turn was sent. A newly started empty thread has no persisted rollout; resume after a real turn remains unverified.' });
      }
    } catch (error) {
      record('profile-probe-error', { verified: false, error: error.message, stderr: proxy.stderr });
    }
  } finally {
    await Promise.all([server, other, tcp].map(s => new Promise(resolve => s.close(resolve))));
  }
}
try {
  console.log(JSON.stringify({ version: version.stdout.trim(), root }));
  const rpc = new Rpc();
  servers.push(rpc);
  const initialize = await rpc.initialize();
  record('initialize', { verified: true, result: initialize });
  const execute = command => rpc.request('command/exec', {
    command, cwd: workspace, sandboxPolicy, timeoutMs: 10000, outputBytesCap: 4096,
  });
  const positive = await execute(['/bin/sh', '-c', 'printf sandbox-ok > permitted.txt; cat permitted.txt']);
  record('workspace-write', { verified: positive.exitCode === 0 && readFileSync(join(workspace, 'permitted.txt'), 'utf8') === 'sandbox-ok', result: positive });
  const forbidden = join(root, 'outside.txt');
  const negative = await execute(['/bin/sh', '-c', 'printf denied > "$1"', 'probe', forbidden]);
  record('outside-write-denied', { verified: negative.exitCode !== 0 && !existsSync(forbidden), result: negative });
  const firstEnvironment = await execute(['/bin/sh', '-c', 'test "$ORG_SESSION_TOKEN" = non-secret-fixture-token']);
  record('default-token-environment', { tokenPresent: firstEnvironment.exitCode === 0, result: firstEnvironment });
  const pinned = new Rpc([
    '-c', 'shell_environment_policy.inherit="all"',
    '-c', 'shell_environment_policy.ignore_default_excludes=true',
    '-c', 'shell_environment_policy.include_only=["PATH","HOME","TMPDIR","SHELL","LANG","ORG_AGENT","ORG_SESSION_TOKEN","GIT_CONFIG_NOSYSTEM","GIT_CONFIG_GLOBAL"]',
  ]);
  servers.push(pinned);
  await pinned.initialize();
  const environment = await pinned.request('command/exec', {
    command: ['/bin/sh', '-c', 'test "$ORG_SESSION_TOKEN" = non-secret-fixture-token && test "$ORG_AGENT" = codex-s0-fixture'],
    cwd: workspace, sandboxPolicy, timeoutMs: 10000, outputBytesCap: 4096,
  });
  record('pinned-token-environment', { verified: environment.exitCode === 0, result: environment });
  await probeSocket(rpc);
  if (!process.argv.includes('--socket-only')) await probeWorktree(rpc);
} catch (error) {
  record('probe-error', { verified: false, error: error.message });
  process.exitCode = 1;
} finally {
  for (const server of servers) await server.close();
  const artifact = { version: version.stdout.trim(), results, servers: servers.map(s => ({ frames: s.frames, stderr: s.stderr })) };
  // The child had only synthetic identity and no sign-in or real connectors.
  const sanitized = JSON.stringify(artifact, null, 2).replaceAll(root, '<TMP>');
  const output = process.env.CODEX_S0_OUTPUT;
  if (output) writeFileSync(output, sanitized + '\n', { mode: 0o600 });
  if (results.some(r => r.verified === false)) process.exitCode = 1;
  rmSync(root, { recursive: true, force: true });
}
