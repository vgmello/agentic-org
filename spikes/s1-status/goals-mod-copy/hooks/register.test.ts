import { expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'
import type { On } from 'claude-code'

import { parseVerdict } from './register'

const USAGE = { input_tokens: 1, output_tokens: 1, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }
const CONDITION = 'app/fees.py exists and `python3 -m unittest` exits 0'

type Seen = {
  posts: { argv: readonly string[]; timeoutMs?: number; body: Record<string, unknown> }[]
  registered: string[]
  forks: string[]
  completions: string[]
  statuses: (string | undefined)[]
  toasts: string[]
}

/**
 * Stands for the engine beneath the mod. The evaluator answers from `verdicts`
 * in order; `null` stands for a fork that can't run (nothing to fork yet).
 */
function engine(on: On, verdicts: (string | null)[], fallback: string[] = [], env: Record<string, string> = {}): Seen {
  const seen: Seen = { posts: [], registered: [], forks: [], completions: [], statuses: [], toasts: [] }
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  // SPIKE S1: goals reads GOALS_MAX_BLOCKS / AC_PRESENCE_SOCKET / AC_AGENT, and POSTs "goal paused".
  on('env.get', (_, e) => ({ value: env[e.name] }))
  on('session.id', () => ({ value: 'sess-1' }))
  on('process.run', (_, e) => {
    seen.posts.push({ argv: e.argv, timeoutMs: e.init?.timeoutMs, body: JSON.parse(String(e.init?.stdin ?? '{}')) })
    return { value: { exitCode: 0, stdout: '', stderr: '' } }
  })
  on('tool.register', (_, e) => {
    seen.registered.push(`mcp__goals__${e.name}`)
    return { value: { tool: `mcp__goals__${e.name}` } }
  })
  on('command.register', (_, e) => {
    seen.registered.push(`/${e.name}`)
    return { value: { command: e.name } }
  })
  on('prompt.submit', (_, e) => ({ text: e.text }))
  on('ui.status', (_, e) => {
    seen.statuses.push(e.text)
    return { value: undefined }
  })
  on('ui.toast', (_, e) => {
    seen.toasts.push(e.text)
    return { value: undefined }
  })
  on('ui.log', () => ({ value: undefined }))
  on('session.messages', () => ({
    value: [
      { role: 'user', text: 'Add app/fees.py', toolUses: [] },
      { role: 'assistant', text: 'Writing it.', toolUses: [{ tool_use_id: 't1', tool: 'Bash', input: { command: 'python3 -m unittest' }, text: 'OK' }] },
    ],
  }))
  on('model.fork', (_, e) => {
    seen.forks.push(e.prompt)
    const text = verdicts.shift()
    return {
      value:
        text === null || text === undefined
          ? { isAnswered: false, reason: 'nothing-to-fork', usage: USAGE }
          : { isAnswered: true, text, usage: USAGE },
    }
  })
  on('model.complete', (_, e) => {
    seen.completions.push(e.prompt)
    const text = fallback.shift()
    return {
      value:
        text === undefined
          ? { isAnswered: false, reason: 'empty-reply', usage: USAGE }
          : { isAnswered: true, text, usage: USAGE },
    }
  })
  // Core: no settings Stop hooks, so nothing else blocks.
  on('classic.Stop', () => ({}))
  return seen
}

const TYPED = { origin: { kind: 'composer' }, presentation: { isFullscreen: false, columns: 100 } } as const

/** Runs /agent-goal as the person typing it. */
function agentGoal($: Engine, args = '') {
  return $.command.run({ command: 'agent-goal', args, ...TYPED })
}

async function start($: Engine): Promise<void> {
  await $.session.start({ cwd: '/', surface: 'terminal', isInteractive: true })
}

test('the mod registers CreateGoal, ClearGoal and /agent-goal', async ($, on) => {
  const seen = engine(on, [])
  await start($)
  expect(seen.registered).toEqual(['mcp__goals__CreateGoal', 'mcp__goals__ClearGoal', '/agent-goal'])
})

test('an unmet goal blocks the stop and says what is missing; a met goal clears', async ($, on) => {
  const seen = engine(on, ['NOT MET\nNo test run is shown yet.', 'MET\nThe unittest run printed OK.'])
  await start($)

  const created = await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION })
  expect(created.deny).toBeUndefined()
  expect(String(created.result)).toContain(`Goal set: ${CONDITION}`)
  expect(seen.statuses.at(-1)).toContain('goal: app/fees.py exists')

  const first = await $.classic.Stop({ stop_hook_active: false, last_assistant_message: 'I wrote the file.' })
  expect(first.block).toContain(`Your goal is not met yet: ${CONDITION}`)
  expect(first.block).toContain('Evaluator: No test run is shown yet.')
  expect(seen.forks[0]).toContain(`<goal>\n${CONDITION}\n</goal>`)
  expect(seen.forks[0]).toContain('<last_message>\nI wrote the file.\n</last_message>')

  const second = await $.classic.Stop({ stop_hook_active: true, last_assistant_message: 'Tests pass: OK.' })
  expect(second.block).toBeUndefined()
  expect(seen.toasts.at(-1)).toContain('Goal met')
  expect(seen.statuses.at(-1)).toBeUndefined()

  // Cleared: the next stop asks no evaluator.
  const third = await $.classic.Stop({ stop_hook_active: false })
  expect(third.block).toBeUndefined()
  expect(seen.forks.length).toBe(2)
})

test('with no goal set, the stop goes ahead without a check', async ($, on) => {
  const seen = engine(on, ['NOT MET\nshould not be asked'])
  await start($)
  const result = await $.classic.Stop({ stop_hook_active: false })
  expect(result.block).toBeUndefined()
  expect(seen.forks.length).toBe(0)
})

test('the goal pauses after eight unmet checks in a row, and the person writing again resumes it', async ($, on) => {
  engine(on, Array.from({ length: 9 }, () => 'NOT MET\nstill missing'))
  await start($)
  await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION })

  for (let i = 1; i <= 7; i++) {
    expect((await $.classic.Stop({ stop_hook_active: i > 1 })).block).toContain('not met yet')
  }
  // The eighth unmet check pauses the goal and lets the session stop.
  expect((await $.classic.Stop({ stop_hook_active: true })).block).toBeUndefined()
  expect((await agentGoal($)).text).toContain('paused after 8 unmet checks')

  await $.prompt.submit({ text: 'keep going', origin: TYPED.origin, wait: false })
  expect((await $.classic.Stop({ stop_hook_active: false })).block).toContain('not met yet')
})

test('SPIKE S1: GOALS_MAX_BLOCKS overrides the cap, and pausing POSTs GoalPaused to AC_PRESENCE_SOCKET', async ($, on) => {
  const seen = engine(on, ['NOT MET\nsecret quoted from the chat', 'NOT MET\nstill missing'], [], {
    GOALS_MAX_BLOCKS: '2',
    AC_PRESENCE_SOCKET: '/tmp/x/events.sock',
    AC_AGENT: 'zz-agent',
  })
  await start($)
  await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION })
  expect((await $.classic.Stop({ stop_hook_active: false })).block).toContain('not met yet')
  expect(seen.posts.length).toBe(0)
  // The second unmet check reaches the overridden cap of 2: the stop goes ahead and the pause is signalled.
  expect((await $.classic.Stop({ stop_hook_active: true })).block).toBeUndefined()
  await new Promise(resolve => setTimeout(resolve, 20)) // let the fire-and-forget POST land
  expect(seen.posts.length).toBe(1)
  expect(seen.posts[0]?.argv).toContain('/tmp/x/events.sock')
  expect(seen.posts[0]?.argv).toContain('--max-time')
  expect(seen.posts[0]?.argv.slice(0, 4)).toEqual(['/usr/bin/curl', '-q', '--noproxy', '*'])
  expect(seen.posts[0]?.timeoutMs).toBe(1000)
  expect(seen.posts[0]?.body).toMatchObject({ v: 1, source: 'goals', hook: 'GoalPaused', blocks_in_a_row: 2, max_blocks: 2, session_id: 'sess-1', agent: 'zz-agent', transport: 'curl' })
  expect(JSON.stringify(seen.posts[0]?.body)).not.toContain('secret')
})

test('SPIKE S1: without AC_PRESENCE_SOCKET a paused goal posts nothing', async ($, on) => {
  const seen = engine(on, ['NOT MET\na', 'NOT MET\nb'], [], { GOALS_MAX_BLOCKS: '2' })
  await start($)
  await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION })
  await $.classic.Stop({ stop_hook_active: false })
  expect((await $.classic.Stop({ stop_hook_active: true })).block).toBeUndefined()
  await new Promise(resolve => setTimeout(resolve, 20)) // let the fire-and-forget POST land
  expect(seen.posts.length).toBe(0)
})

test('when the session cannot be forked, a separate completion checks the goal', async ($, on) => {
  const seen = engine(on, [null], ['MET\nThe unittest run printed OK.'])
  await start($)
  await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION })

  const result = await $.classic.Stop({ stop_hook_active: false })
  expect(result.block).toBeUndefined()
  expect(seen.completions.length).toBe(1)
  expect(seen.completions[0]).toContain('TOOL Bash')
  expect(seen.completions[0]).toContain('-> OK')
})

test('a check that cannot run never traps the session', async ($, on) => {
  engine(on, [null], [])
  await start($)
  await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION })
  expect((await $.classic.Stop({ stop_hook_active: false })).block).toBeUndefined()
})

test('CreateGoal refuses subagents, empty and over-long conditions', async ($, on) => {
  engine(on, [])
  await start($)
  const sub = await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION, agentId: 'a1' })
  expect(sub.deny).toContain('main session')
  const empty = await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: '   ' })
  expect(empty.deny).toContain('condition')
  const long = await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: 'x'.repeat(501) })
  expect(long.deny).toContain('500 characters')
})

test('a new goal replaces the old one, and ClearGoal and /agent-goal clear it', async ($, on) => {
  engine(on, [])
  await start($)
  await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: 'first goal' })
  const replaced = await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION })
  expect(String(replaced.result)).toContain('Was: first goal')
  expect((await agentGoal($)).text).toBe(`Goal: ${CONDITION} (not yet checked)`)

  const cleared = await $.tool.call({ tool: 'mcp__goals__ClearGoal', reason: 'needs a product decision' })
  expect(String(cleared.result)).toContain('Reason: needs a product decision')
  expect((await agentGoal($)).text).toContain('No goal is set')

  await $.tool.call({ tool: 'mcp__goals__CreateGoal', condition: CONDITION })
  expect((await agentGoal($, 'clear')).text).toBe(`Goal cleared: ${CONDITION}`)
  expect((await $.classic.Stop({ stop_hook_active: false })).block).toBeUndefined()
})

test('verdicts are read from the first line, tolerating formatting', () => {
  expect(parseVerdict('MET\nTests pass.')).toEqual({ kind: 'met', reason: 'Tests pass.' })
  expect(parseVerdict('**NOT MET**\nNo tests yet.')).toEqual({ kind: 'unmet', reason: 'No tests yet.' })
  expect(parseVerdict('  not met.  ')).toEqual({ kind: 'unmet', reason: 'no reason given' })
  expect(parseVerdict('Maybe?').kind).toBe('unknown')
})
