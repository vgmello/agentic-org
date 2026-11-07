// presence (spike S1): forwards normalized Claude Code hook events to the
// status receiver over a Unix socket, fire-and-forget with a ~200 ms bound.
//
// Never forwards tool inputs or outputs: only the tool NAME, notification
// type/text and lifecycle reasons. Stop and PermissionRequest are reported
// after the hooks beneath settle (`await next(e)`), so the receiver sees
// whether goals blocked the stop or another hook/auto mode decided.
//
// Identity: AC_AGENT if set (it does NOT reach `claude --bg` sessions unless
// passed with --settings env), else bound ONCE from the main thread's
// SessionStart (`agent_type` = --agent, then `session_title` = --name).
// Later events reuse it: PreToolUse carries no identity at all, and inside a
// subagent `agent_type` is the subagent's type.
//
// The pid is not sent: the receiver takes it from the socket (LOCAL_PEERPID).
//
// THROWAWAY SPIKE CODE.

import type { Register } from 'claude-code'

const SCHEMA = 1
const TIMEOUT_MS = 200
const DEFAULT_SOCKET = '/tmp/ac-spike-s1/run/events.sock'
// Separate from the socket's directory, so a session creating it can't block the receiver.
const DEFAULT_STATE_DIR = '/tmp/ac-spike-s1/state'
const HUNG_BACKOFF_MS = 5_000
/** Lifecycle events that are always attempted, even inside a hung backoff (bounded anyway). */
const ALWAYS = new Set(['Stop', 'SessionEnd'])
/** One per module load: seq restarts on a reload, and the receiver resets its window on a new epoch. */
const EPOCH = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
/** curl that reads no ~/.curlrc (-q must come first) and uses no proxy. */
const CURL = ['/usr/bin/curl', '-q', '--noproxy', '*', '-s', '-o', '/dev/null', '--max-time', '0.3']

// Module state (one copy per hooks-module load, i.e. per session process).
let seq = 0
let boundAgent: string | null = null
let env: { acAgent: string | null; socket: string; transport: string; stateDir: string } | null = null

async function readEnv($: any): Promise<NonNullable<typeof env>> {
  if (env !== null) return env
  const acAgent = (await $.env.get('AC_AGENT')) || null
  const socket = (await $.env.get('AC_PRESENCE_SOCKET')) || DEFAULT_SOCKET
  // SPIKE item 12: how to deliver. 'curl' (default: a child bounded by --max-time and timeoutMs),
  // 'fetch' ($.http.fetch, skipped while one is hung) or 'fetch-raw' (the original, unbounded).
  const transport = (await $.env.get('AC_PRESENCE_TRANSPORT')) || 'curl'
  const stateDir = (await $.env.get('AC_PRESENCE_STATE_DIR')) || DEFAULT_STATE_DIR
  env = { acAgent, socket, transport, stateDir }
  return env
}

// Hung-receiver guard for the fetch transport. $.http.fetch has no timeout or
// abort, and the session drains in-flight fetches before SessionEnd/exit (~30 s
// each against a hung receiver). So: at most one POST in flight; one that has not
// settled within TIMEOUT_MS marks the receiver hung, and POSTs are skipped for
// HUNG_BACKOFF_MS after it finally settles (the state file still gets every event).
let inFlight = false
let hungUntil = 0
let skipped = 0

let chain: Promise<unknown> = Promise.resolve()
/** tool_use_id -> the loop (subagent) that made the call, from tool.call: at runtime the classic
 * PreToolUse envelope carries neither agent_id nor agentId (verified, spike S1 run5). */
const loopByCall = new Map<string, string>()

function deliver($: any, e: NonNullable<typeof env>, body: Record<string, unknown>): void {
  const always = ALWAYS.has(String(body.hook))
  if (e.transport !== 'fetch-raw' && Date.now() < hungUntil && !always) {
    skipped++
    return
  }
  const text = JSON.stringify({ ...body, transport: e.transport, skipped_before: skipped })
  if (e.transport === 'curl') {
    // One child at a time, in order (separate children race each other otherwise),
    // each bounded by curl --max-time and process.run's timeoutMs, so session
    // shutdown never waits more than ~0.3 s on a hung receiver. A timeout
    // (curl exit 28) marks the receiver hung: skip POSTs for HUNG_BACKOFF_MS, except
    // Stop and SessionEnd, which are always attempted.
    chain = chain
      .then(async () => {
        if (Date.now() < hungUntil && !always) {
          skipped++
          return
        }
        const r = await $.process
          .run([...CURL, '--unix-socket', e.socket,
            '-H', 'content-type: application/json', '--data-binary', '@-', 'http://presence/v1/events'],
          { stdin: text, timeoutMs: 1000 })
          .catch(() => null)
        if (r === null || r.exitCode === 28) hungUntil = Date.now() + HUNG_BACKOFF_MS
      })
      .catch(() => undefined)
    return
  }
  if (e.transport === 'fetch' && inFlight) {
    skipped++
    return
  }
  inFlight = true
  const started = Date.now()
  const send = $.http
    .fetch('http://presence/v1/events', { method: 'POST', headers: { 'content-type': 'application/json' }, body: text, socketPath: e.socket })
    .catch(() => undefined)
    .finally(() => {
      inFlight = false
      if (Date.now() - started > TIMEOUT_MS) hungUntil = Date.now() + HUNG_BACKOFF_MS
    })
  void Promise.race([send, $.clock.sleep(TIMEOUT_MS).catch(() => undefined)]).catch(() => undefined)
}

// Latest normalized event per session, so orgd can rebuild state after a restart
// and still learn the status while the socket is hung. $.fs has write but no rename,
// so this is a whole-file write, not an atomic replace (see README).
// The directory is made 0700 once per load (mkdir -m, then chmod in case a session made it
// first); $.fs.write can't set a file mode, so files get the umask's (0644): the 0700
// directory is what keeps them private.
let writes: Promise<unknown> = Promise.resolve()
let dirReady: Promise<unknown> | null = null
function writeState($: any, e: NonNullable<typeof env>, body: Record<string, unknown>): void {
  const name = String(body.session_id ?? 'unknown').replace(/[^A-Za-z0-9-]/g, '_')
  const text = JSON.stringify({ ...body, written_at: Date.now(), posts_skipped: skipped }) + '\n'
  dirReady ??= $.process
    .run(['/bin/mkdir', '-p', '-m', '700', e.stateDir], { timeoutMs: 1000 })
    .then(() => $.process.run(['/bin/chmod', '700', e.stateDir], { timeoutMs: 1000 }))
    .catch(() => undefined)
  writes = writes.then(() => dirReady).then(() => $.fs.write(`${e.stateDir}/${name}.json`, text)).catch(() => undefined)
}

function emit($: any, e: NonNullable<typeof env>, body: Record<string, unknown>): void {
  writeState($, e, body)
  deliver($, e, body)
}

/** Event-specific, input-free fields. */
function fields(name: string, e: any): Record<string, unknown> {
  switch (name) {
    case 'PreToolUse':
      return { tool: e.tool, tool_use_id: e.tool_use_id }
    case 'PostToolUse':
    case 'PostToolUseFailure':
    case 'PermissionDenied':
      return { tool: e.tool_name, tool_use_id: e.tool_use_id }
    case 'PermissionRequest':
      return { tool: e.tool_name }
    case 'Notification':
      return { notification_type: e.notification_type, message: e.message, title: e.title }
    case 'SessionStart':
      return { source: e.source, model: e.model, session_title: e.session_title }
    case 'SessionEnd':
      return { reason: e.reason }
    case 'Stop':
      return {
        stop_hook_active: e.stop_hook_active,
        background_tasks: Array.isArray(e.background_tasks) ? e.background_tasks.length : null,
        background_task_kinds: Array.isArray(e.background_tasks) ? e.background_tasks.map((t: any) => t.type ?? t.kind ?? null) : null,
      }
    case 'StopFailure':
      return { error: e.error, error_details: typeof e.error_details === 'string' ? e.error_details.slice(0, 200) : undefined }
    case 'SubagentStart':
    case 'SubagentStop':
      return { subagent_id: e.agent_id, subagent_type: e.agent_type }
    case 'Elicitation':
      return { mcp_server: e.mcp_server_name, message: e.message, mode: e.mode }
    default:
      return {}
  }
}

/** Events the receiver needs. Everything else passes straight through. */
const FORWARD = new Set([
  'SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'PostToolUseFailure',
  'PermissionRequest', 'PermissionDenied', 'Notification', 'Elicitation', 'Stop', 'StopFailure',
  'SessionEnd', 'SubagentStart', 'SubagentStop',
])
/** Reported after the chain beneath settles. */
const AFTER = new Set(['Stop', 'PermissionRequest'])

export const register: Register = on => {
  // Mod-registered tools (goals' CreateGoal) raise no classic PreToolUse, only
  // tool.call: forward it as a sequenced "ToolCall" heartbeat.
  on('tool.call', async ($: any, e: any, next: any) => {
    const ev = await readEnv($)
    if (e.agentId !== undefined && e.tool_use_id) loopByCall.set(e.tool_use_id, e.agentId)
    deliver($, ev, {
      v: SCHEMA, agent: boundAgent, session_id: await $.session.id(), hook: 'ToolCall', seq: ++seq, epoch: EPOCH,
      tool: e.tool, tool_use_id: e.tool_use_id, loop_agent_id: e.agentId ?? null, in_subagent: e.agentId !== undefined,
      t_hook: Date.now(), t_send: Date.now(),
    })
    return next(e)
  }).catch(($: any, e: any, next: any) => next(e))

  on('classic.*', async ($: any, e: any, next: any) => {
    const tHook = Date.now()
    const name = String(next.event).replace(/^classic\./, '')
    if (!FORWARD.has(name)) return next(e)

    const ev = await readEnv($)
    const acAgent = ev.acAgent
    // The classic input says `agent_id` inside a subagent; the PreToolUse envelope carries `agentId`.
    const loopAgentId: string | null = e.agent_id ?? e.agentId ?? (e.tool_use_id ? loopByCall.get(e.tool_use_id) : undefined) ?? null
    if ((name === 'PostToolUse' || name === 'PostToolUseFailure') && e.tool_use_id) loopByCall.delete(e.tool_use_id)
    const isSub = loopAgentId !== null && name !== 'SubagentStart' && name !== 'SubagentStop'
    // Subagent* events carry the SUBAGENT's agent_type (e.g. general-purpose): never bind from them.
    const isSubagentEvent = name === 'SubagentStart' || name === 'SubagentStop'
    if (boundAgent === null && !isSubagentEvent && (name === 'SessionStart' || (!isSub && e.agent_type))) {
      // Bind once, from the main thread's SessionStart; after a module reload (no new
      // SessionStart) from the first main-thread event that names the agent.
      boundAgent = acAgent ?? e.agent_type ?? e.session_title ?? null
    }
    const sessionId: string = e.session_id ?? (await $.session.id())
    const base: Record<string, unknown> = {
      v: SCHEMA,
      agent: boundAgent,
      agent_source: acAgent !== null ? 'AC_AGENT' : boundAgent !== null ? 'SessionStart' : 'none',
      session_id: sessionId,
      hook: name,
      payload_agent_type: e.agent_type ?? null,
      epoch: EPOCH,
      loop_agent_id: isSub ? loopAgentId : null,
      in_subagent: isSub,
      permission_mode: e.permission_mode ?? null,
      t_hook: tHook,
      ...fields(name, e),
    }

    if (!AFTER.has(name)) {
      base.seq = ++seq
      base.t_send = Date.now()
      emit($, ev, base)
      return next(e)
    }

    const below = await next(e)
    base.seq = ++seq
    base.t_after_next = Date.now()
    base.chain_below = Array.isArray(next.trace) ? next.trace.map((t: any) => `${t.index}:${t.plugin}:${t.tier}:${t.outcome?.kind ?? JSON.stringify(t.outcome)}`) : null
    if (name === 'Stop') base.blocked = below?.block !== undefined
    if (name === 'PermissionRequest') {
      base.decided = below?.decision !== undefined
      base.decision = below?.decision?.behavior ?? null
    }
    base.t_send = Date.now()
    emit($, ev, base)
    return below
  }).catch(($: any, e: any, next: any) => next(e))
}
