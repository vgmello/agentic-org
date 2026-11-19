// Words for the Processes screens: a level or a kind is always a word with an icon, never a color alone (PROC-18).
import type { EntryKind, Problem } from './types';

export const KIND_WORD: Record<EntryKind, string> = { inherited: '', override: 'Override', addition: 'Addition' };
export const SOURCE_WORD = { organization: 'Organization', team: 'Team', project: 'Project', agent: 'Agent' } as const;

/** "Team Web", "Project Atlas", "Organization", "Agent Noor Haddad": the level's word, then where, for a badge. */
export const placeLabel = (level: keyof typeof SOURCE_WORD, scopeName?: string): string => (level === 'organization' || !scopeName ? SOURCE_WORD[level] : `${SOURCE_WORD[level]} ${scopeName}`);

/** HH:MM on a 24-hour clock (never "24:05"). */
export const hhmm = (iso: string): string => new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });

/** An invalid field follows the process name and colon in the screen's problem list. */
export const problemWords = (problem: Problem): string => (problem.kind === 'invalid' || problem.kind === 'mismatch' ? problem.text.charAt(0).toLowerCase() + problem.text.slice(1) : problem.text);

/** Include a date when a saved handbook copy is from a different local calendar day. */
export function copyTime(iso: string, now = new Date()): string {
	const at = new Date(iso);
	if (at.getFullYear() === now.getFullYear() && at.getMonth() === now.getMonth() && at.getDate() === now.getDate()) return hhmm(iso);
	return at.toLocaleString([], { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
}
