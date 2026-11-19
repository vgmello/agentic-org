// The session log's header names the run it belongs to (agent-run-modes-ux §1 Runs row): when it started, who started
// it, how long it ran (or "running … " while open) and its outcome, in the Runs tab's words.
import { durationFact, outcomeText, startedByText, whenText } from '../../present';
import type { Turn } from '../../types';

export function runFacts(t: Turn, now = Date.now()): string[] {
	return [whenText(t.startedAt), startedByText(t.startedBy), durationFact(t, now), outcomeText(t)];
}
