// Words for the Processes screens: a level or a kind is always a word with an icon, never a color alone (PROC-18).
import type { EntryKind } from './types';

export const KIND_WORD: Record<EntryKind, string> = { inherited: '', override: 'Override', addition: 'Addition' };
export const SOURCE_WORD = { organization: 'Organization', team: 'Team', project: 'Project', agent: 'Agent' } as const;

/** "Team Web", "Project Atlas", "Organization", "Agent noor": the level's word, then where, for a badge. */
export const placeLabel = (level: keyof typeof SOURCE_WORD, scopeName?: string): string => (level === 'organization' || !scopeName ? SOURCE_WORD[level] : `${SOURCE_WORD[level]} ${scopeName}`);

/** HH:MM on a 24-hour clock (never "24:05"). */
export const hhmm = (iso: string): string => new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
