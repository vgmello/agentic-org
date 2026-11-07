// goals: a CreateGoal tool that works like Claude Code's /goal, set by the agent.
//
// The agent calls CreateGoal with a completion condition. Each time the
// session is about to stop, a Stop hook asks a separate evaluator whether the
// conversation shows the condition met. If not, the stop is blocked and the
// agent is told what is still missing, so it keeps working. A met goal clears
// itself. After MAX_BLOCKS unmet checks in a row the goal pauses until the
// person writes again, so a goal that can't be met never traps the session.

import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { Goal } from '../types'

/** Longest condition the agent may set, as /goal's own limit. */
const MAX_CONDITION = 500
/** Unmet checks in a row before the goal pauses until the person writes again.
 * SPIKE S1: overridable with GOALS_MAX_BLOCKS (read at session start). */
let MAX_BLOCKS = 8
/** SPIKE S1: where the "goal paused" signal goes (the presence receiver). */
let PRESENCE_SOCKET: string | undefined = undefined

/** SPIKE S1: tell the presence receiver the goal paused. Delivered by a curl child
 * bounded by --max-time and timeoutMs, not $.http.fetch: the session drains in-flight
 * fetches before it exits (~30 s each against a hung receiver; see the S1 README). */
function emitPaused($: EngineInterface, current: Goal): void {
  const socket = PRESENCE_SOCKET
  if (socket === undefined) return
  const send = async () => {
    const [session_id, agent] = await Promise.all([$.session.id(), $.env.get('AC_AGENT')])
    // No last_reason: the evaluator's text can quote conversation content.
    const body = JSON.stringify({
      v: 1,
      source: 'goals',
      hook: 'GoalPaused',
      session_id,
      agent: agent || null,
      blocks_in_a_row: current.blocksInARow,
      max_blocks: MAX_BLOCKS,
      t_hook: Date.now(),
      transport: 'curl', // the peer is this curl child: the receiver takes the session pid from its parent
    })
    await $.process.run(
      // -q first: ignore ~/.curlrc; --noproxy '*': never route the socket POST through a proxy.
      ['/usr/bin/curl', '-q', '--noproxy', '*', '-s', '-o', '/dev/null', '--max-time', '0.3', '--unix-socket', socket,
        '-H', 'content-type: application/json', '--data-binary', '@-', 'http://presence/v1/events'],
      { stdin: body, timeoutMs: 1000 },
    )
  }
  void send().catch(() => undefined)
}
/** Model for the fallback evaluator, used when the session can't be forked. */
const FALLBACK_MODEL = 'haiku'
/** Characters of recent conversation the fallback evaluator reads. */
const FALLBACK_CHARS = 40_000

const CREATE = 'mcp__goals__CreateGoal'
const CLEAR = 'mcp__goals__ClearGoal'

// Change the shape tag when Goal's fields change: it declines what an
// earlier version stored, which outlives a reload.
const goal = atom({ plugin: 'goals', key: 'goal' } as const, null, { shape: 'goal-1' })

const CREATE_DESCRIPTION = [
  'Set your own goal for this session: a completion condition that a separate evaluator checks each time you are about to stop.',
  'While the condition is unmet you are asked to keep working, and you are told what is still missing; once the evaluator confirms it from the conversation, the goal clears.',
  'It works like the /goal command, except that you set it.',
  'Write the condition so it can be verified from the conversation alone. Name the deliverables and the checks, not the effort: "app/fees.py and tests/test_fees.py exist, and `python3 -m unittest` exits 0", not "work on fees".',
  `At most ${MAX_CONDITION} characters. Setting a goal replaces the current one. Main session only: a subagent can't hold a goal.`,
].join(' ')

const CLEAR_DESCRIPTION = [
  'Clear your current session goal, so the session can stop without meeting it.',
  'Use it when the person asks you to stop, when the goal turns out to be wrong or impossible to meet, or when you need a decision from the person to go on. Say why in `reason`.',
].join(' ')

type Verdict =
  | { kind: 'met'; reason: string }
  | { kind: 'unmet'; reason: string }
  | { kind: 'unknown'; reason: string }

function short(text: string, max = 60): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat
}

function showStatus($: EngineInterface, current: Goal | null): void {
  if (current === null) $.ui.status(undefined)
  else if (current.blocksInARow >= MAX_BLOCKS) $.ui.status(`goal paused: ${short(current.condition)}`)
  else $.ui.status(`goal: ${short(current.condition)}`)
}

function question(condition: string, lastMessage: string | undefined): string {
  return [
    'Pause the work and answer one question about this conversation. Do not continue the task.',
    `A goal was set for this session:\n<goal>\n${condition}\n</goal>`,
    lastMessage
      ? `The assistant's last message, which the conversation above may not include yet:\n<last_message>\n${lastMessage}\n</last_message>`
      : '',
    'Is the goal met? Judge only by evidence in the conversation: files written, commands run and their output, results reported. Plans, promises and claims without evidence do not count.',
    'Reply with exactly MET or NOT MET on the first line, then one short sentence: the evidence, or what is still missing.',
  ]
    .filter(Boolean)
    .join('\n\n')
}

export function parseVerdict(text: string): Verdict {
  const lines = text.trim().split('\n')
  const first = (lines[0] ?? '').toUpperCase().replace(/[^A-Z ]/g, '').trim()
  const reason = lines.slice(1).join(' ').trim() || 'no reason given'
  if (first.startsWith('NOT MET')) return { kind: 'unmet', reason }
  if (first.startsWith('MET')) return { kind: 'met', reason }
  return { kind: 'unknown', reason: `unclear verdict "${short(lines[0] ?? '', 40)}"` }
}

/** The recent conversation as plain text, for the fallback evaluator. */
async function recentConversation($: EngineInterface): Promise<string> {
  const rows = await $.session.messages()
  const lines: string[] = []
  for (const row of rows) {
    if (row.text) lines.push(`${row.role.toUpperCase()}: ${row.text}`)
    for (const use of row.toolUses) {
      const outcome = use.text === undefined ? '' : ` -> ${use.isError ? 'ERROR ' : ''}${short(use.text, 2000)}`
      lines.push(`TOOL ${use.tool} ${short(JSON.stringify(use.input), 300)}${outcome}`)
    }
  }
  return lines.join('\n').slice(-FALLBACK_CHARS)
}

/**
 * Asks a separate evaluator whether the goal is met. A fork of the session
 * reads the whole transcript from the prompt cache; when there is nothing to
 * fork, a one-off completion reads the recent conversation instead.
 */
async function evaluate($: EngineInterface, condition: string, lastMessage: string | undefined): Promise<Verdict> {
  const ask = question(condition, lastMessage)
  const forked = await $.model.fork({ prompt: ask, effort: 'low' })
  if (forked.isAnswered) return parseVerdict(forked.text)

  const completed = await $.model.complete({
    model: FALLBACK_MODEL,
    system: 'You check whether a goal set for a coding session has been met, from the conversation you are shown.',
    prompt: `<conversation>\n${await recentConversation($)}\n</conversation>\n\n${ask}`,
  })
  if (completed.isAnswered) return parseVerdict(completed.text)
  return { kind: 'unknown', reason: `the evaluator could not run (${forked.reason}, then ${completed.reason})` }
}

function blockReason(current: Goal, reason: string): string {
  return [
    `Your goal is not met yet: ${current.condition}`,
    `Evaluator: ${reason}`,
    'Keep working toward it, and show the evidence in the conversation (for example the test output), because the evaluator sees only the conversation.',
    "If it can't be met (it's blocked, it needs the person's decision, or the goal itself is wrong), say so and call ClearGoal with the reason before you stop.",
  ].join('\n')
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const override = Number.parseInt((await $.env.get('GOALS_MAX_BLOCKS')) ?? '', 10)
    if (Number.isFinite(override) && override > 0) MAX_BLOCKS = override
    PRESENCE_SOCKET = (await $.env.get('AC_PRESENCE_SOCKET')) ?? undefined
    await $.tool.register({
      name: 'CreateGoal',
      description: CREATE_DESCRIPTION,
      inputSchema: {
        type: 'object',
        properties: {
          condition: {
            type: 'string',
            minLength: 1,
            maxLength: MAX_CONDITION,
            description: 'What must be true when the work is done, checkable from the conversation.',
          },
        },
        required: ['condition'],
        additionalProperties: false,
      },
      isDeferred: false,
    })
    await $.tool.register({
      name: 'ClearGoal',
      description: CLEAR_DESCRIPTION,
      inputSchema: {
        type: 'object',
        properties: {
          reason: { type: 'string', description: 'Why the goal is being cleared.' },
        },
        required: ['reason'],
        additionalProperties: false,
      },
      isDeferred: false,
    })
    await $.command.register({
      name: 'agent-goal',
      description: 'Show the goal the agent set for this session, or clear it: /agent-goal [clear]',
    })
    showStatus($, await read($, goal))

    return next(e)
  })

  on('tool.call', { tool: CREATE }, async ($, e) => {
    if (e.agentId !== undefined) {
      return {
        deny: "CreateGoal works only in the main session; a subagent can't hold a goal. Treat the condition as your definition of done and report against it.",
      }
    }
    const condition = typeof e.condition === 'string' ? e.condition.trim() : ''
    if (condition === '') return { deny: 'Give the goal as `condition`: what must be true when the work is done.' }
    if (condition.length > MAX_CONDITION) {
      return { deny: `Keep the condition to ${MAX_CONDITION} characters (it has ${condition.length}).` }
    }

    const previous = await read($, goal)
    const created: Goal = { condition, setAt: new Date().toISOString(), checks: 0, blocksInARow: 0 }
    await update($, goal, () => created)
    showStatus($, created)

    return {
      result: [
        previous === null ? `Goal set: ${condition}` : `Goal replaced. Was: ${previous.condition}\nNow: ${condition}`,
        'Each time you are about to stop, a separate evaluator checks it against the conversation, and you are asked to keep working while it is unmet. Show the evidence in the conversation, because the evaluator sees only that.',
      ].join('\n\n'),
    }
  }).catch(() => ({ deny: 'goals: CreateGoal failed; no goal was set.' }))

  on('tool.call', { tool: CLEAR }, async ($, e) => {
    if (e.agentId !== undefined) return { deny: 'ClearGoal works only in the main session.' }
    const previous = await read($, goal)
    if (previous === null) return { result: 'No goal is set.' }

    await update($, goal, () => null)
    showStatus($, null)
    const reason = typeof e.reason === 'string' && e.reason.trim() !== '' ? e.reason.trim() : 'no reason given'
    $.ui.toast(`Goal cleared: ${short(reason, 80)}`)

    return { result: `Goal cleared: ${previous.condition}\nReason: ${reason}` }
  }).catch(() => ({ deny: 'goals: ClearGoal failed.' }))

  on('command.run', { command: 'agent-goal' }, async ($, e) => {
    const current = await read($, goal)
    if (e.args.trim() === 'clear') {
      if (current === null) return { text: 'No goal is set.' }
      await update($, goal, () => null)
      showStatus($, null)
      return { text: `Goal cleared: ${current.condition}` }
    }
    if (current === null) return { text: 'No goal is set. The agent sets one with its CreateGoal tool.' }

    const state =
      current.blocksInARow >= MAX_BLOCKS
        ? `paused after ${current.blocksInARow} unmet checks in a row; send a message to resume`
        : current.checks === 0
          ? 'not yet checked'
          : `${current.checks} unmet ${current.checks === 1 ? 'check' : 'checks'}`
    const last = current.lastReason === undefined ? '' : `\nLast check: ${current.lastReason}`
    return { text: `Goal: ${current.condition} (${state})${last}` }
  })

  // The person writing again resumes a paused goal.
  on('prompt.submit', async ($, e, next) => {
    const current = await read($, goal)
    if (current !== null && current.blocksInARow > 0) {
      const resumed = { ...current, blocksInARow: 0 }
      await update($, goal, () => resumed)
      showStatus($, resumed)
    }
    return next(e)
  }).catch(($, e, next) => next(e))

  on('classic.Stop', async ($, e, next) => {
    const current = await read($, goal)
    if (current === null || current.blocksInARow >= MAX_BLOCKS) return next(e)

    const verdict = await evaluate($, current.condition, e.last_assistant_message)
    if (verdict.kind === 'unknown') {
      // Never trap the session on a check that couldn't run.
      $.ui.log(`goals: couldn't check the goal (${verdict.reason}), so the session may stop.`)
      return next(e)
    }
    if (verdict.kind === 'met') {
      await update($, goal, () => null)
      showStatus($, null)
      $.ui.toast(`Goal met: ${short(current.condition, 80)}`)
      return next(e)
    }

    const checked: Goal = {
      ...current,
      checks: current.checks + 1,
      blocksInARow: current.blocksInARow + 1,
      lastReason: verdict.reason,
    }
    await update($, goal, () => checked)
    showStatus($, checked)
    if (checked.blocksInARow >= MAX_BLOCKS) {
      emitPaused($, checked)
      $.ui.toast(`Goal paused after ${MAX_BLOCKS} unmet checks; send a message to resume.`)
      return next(e)
    }

    const below = await next(e)
    const ours = blockReason(checked, verdict.reason)
    return { ...below, block: below.block === undefined ? ours : `${below.block}\n\n${ours}` }
  }).catch(($, e, next) => next(e))
}
