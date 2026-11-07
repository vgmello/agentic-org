/** The session's goal, as the agent set it with CreateGoal. */
export type Goal = {
  /** What must be true when the work is done, checkable from the conversation. */
  condition: string
  /** When the agent set it (ISO 8601). */
  setAt: string
  /** How many checks have found it unmet so far. */
  checks: number
  /** Unmet checks since the person last wrote; at the cap the goal pauses. */
  blocksInARow: number
  /** The evaluator's last word on what is still missing. */
  lastReason?: string
}

declare module 'claude-code' {
  interface PluginState {
    goals: { goal: Shaped<Goal | null> }
  }
}
