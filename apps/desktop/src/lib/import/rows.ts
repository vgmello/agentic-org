// "Bring in an existing team": from what the scan found to the rows the owner reviews (spec our-org-in-the-app §4).
// Pure: nothing here reads a file or calls the office. The scan's two sources are merged by name, one row per agent.

import { trimWhiteSpace } from '$lib/name';
import type { AgentRuntime, Member } from '$lib/proto';
import type { HrAgent, Scan } from './types';

/** What to do with a row: bring it in running, bring it in paused, or leave it out. */
export type Choice = 'active' | 'paused' | 'skip';

export interface Row {
	/** The messaging name in lower case: the same agent is the same row whatever its capitals. */
	key: string;
	/** What it is shown as. */
	name: string;
	/** The handle agents address it by; HQ keeps it unique. */
	messagingName: string;
	source: 'both' | 'hr' | 'messaging';
	/** Where HR keeps its record, when it has one. */
	hr?: HrAgent;
	role?: string;
	/** The role was suggested from the title (shown as "Suggested"); it still counts as filled. */
	roleSuggested?: boolean;
	title?: string;
	focus?: string;
	runtime?: AgentRuntime;
	model?: string;
	home?: string;
	/** The row of its team: set from HR's department, changed by the owner. */
	teamKey?: string;
	/** Who it reports to: another row's key, or `owner`. */
	managerKey: string;
	/** The owner chose the manager by hand: the default (the top team's lead) no longer follows the team. */
	managerSet?: boolean;
	choice: Choice;
	/** The member already in the organization with this messaging name ("Already in <org>"): an agent, or a person whose name it is. */
	existingMemberId?: string;
	existingKind?: 'agent' | 'person';
	/** That agent was offboarded: its name stays reserved, and it is no longer part of the team. */
	existingOffboarded?: boolean;
	/** An earlier import made this agent and the owner has since removed it: it stays removed unless the owner asks for it again. */
	removedEarlier?: boolean;
}

export interface Review {
	rows: Row[];
	/** HR agents that were offboarded in HR: a record, not brought in. */
	offboarded: HrAgent[];
}

export const OWNER = 'owner';

/** Words a messaging name must never be turned into a row for: reserved handles. */
const NOT_AGENTS = new Set(['owner', 'office', 'hr']);

const lower = (s: string) => s.toLowerCase();

/** "software-engineer" -> "software engineer". */
export const roleWords = (slug: string | undefined): string | undefined => (slug ? slug.replace(/[-_]+/g, ' ').trim() || undefined : undefined);

/**
 * One row per agent, merged across the sources by name. An HR record and a messaging name are the same agent when HR's
 * id is the inbox name. `exclude` are names that are not agents here (the owner's own inbox); `members` are the
 * organization's members, to mark an agent that is already in.
 */
export function buildRows(scan: Scan, { exclude = [], members = [] }: { exclude?: string[]; members?: Member[] } = {}): Review {
	const skip = new Set([...NOT_AGENTS, ...exclude.map(lower)]);
	const hr = scan.hr?.error ? [] : (scan.hr?.agents ?? []);
	const names = scan.messaging?.error ? [] : (scan.messaging?.names ?? []);
	const offboarded = hr.filter((a) => lower(a.status ?? '') === 'offboarded');
	// The same id in other capitals (or in two projects) is the same agent: the first record is kept. A record whose id is a
	// reserved handle or the owner's is not an agent either.
	const records = new Map<string, HrAgent>();
	for (const a of hr) if (!offboarded.includes(a) && !skip.has(lower(a.id)) && !records.has(lower(a.id))) records.set(lower(a.id), a);
	// An agent offboarded in HR stays out even when its inbox is still there.
	const gone = new Set(offboarded.map((a) => lower(a.id)));
	// The same name in other capitals is the same handle (HQ compares them ignoring case): the first one is kept.
	const handles = new Map<string, string>();
	for (const name of names) if (!skip.has(lower(name)) && !gone.has(lower(name)) && !handles.has(lower(name))) handles.set(lower(name), name);
	// HQ keeps one handle namespace: an agent's messaging name, and a person's name (the owner's apart).
	const existing = new Map<string, { id: string; kind: 'agent' | 'person'; offboarded: boolean }>();
	for (const m of members) {
		if (m.kind === 'agent' && m.agent) existing.set(lower(m.agent.messaging_name), { id: m.id, kind: 'agent', offboarded: m.agent.status === 'offboarded' });
		else if (m.kind === 'person' && !m.owner) existing.set(lower(m.name), { id: m.id, kind: 'person', offboarded: false });
	}

	const keys = new Set([...records.keys(), ...handles.keys()]);
	const rows: Row[] = [...keys].sort().map((key) => {
		const record = records.get(key);
		const handle = handles.get(key);
		const messagingName = handle ?? record!.id;
		return {
			key,
			name: cleanName(record?.name) || messagingName,
			messagingName,
			source: record && handle ? 'both' : record ? 'hr' : 'messaging',
			...(record ? { hr: record } : {}),
			...(record ? fromHr(record) : {}),
			managerKey: OWNER,
			choice: 'active',
			...(existing.has(key) ? { existingMemberId: existing.get(key)!.id, existingKind: existing.get(key)!.kind, ...(existing.get(key)!.offboarded ? { existingOffboarded: true } : {}) } : {})
		};
	});
	// Reporting lines from HR: to another row's agent, or to the owner.
	const byHrId = new Map(rows.filter((r) => r.hr).map((r) => [lower(r.hr!.id), r.key]));
	for (const row of rows) {
		const boss = row.hr?.reports_to;
		const bossKey = boss ? byHrId.get(lower(boss)) : undefined;
		if (bossKey && bossKey !== row.key) row.managerKey = bossKey;
	}
	breakCycles(rows);
	return { rows, offboarded };
}

/** HR can say A reports to B and B to A; HQ refuses a cycle. The row that closes it reports to the owner instead. */
export function breakCycles(rows: Row[]): void {
	const byKey = new Map(rows.map((r) => [r.key, r]));
	for (const row of rows) {
		const seen = new Set<string>([row.key]);
		for (let boss = byKey.get(row.managerKey); boss; boss = byKey.get(boss.managerKey)) {
			// Only a row that is in the cycle itself is cut; one that merely reports into it keeps its line.
			if (boss.key === row.key) {
				row.managerKey = OWNER;
				break;
			}
			if (seen.has(boss.key)) break;
			seen.add(boss.key);
		}
	}
}

/** The agent record keeps at most this many characters in a role, title, focus or model (org-proto `AGENT_TEXT_MAX_CHARS`). */
const TEXT_MAX = 200;
/** Free text from HR is cut to the limit rather than refused: it is a description, not an identifier. */
const clip = (text: string): string => {
	const chars = [...text];
	return chars.length > TEXT_MAX ? `${chars.slice(0, TEXT_MAX - 1).join('').trimEnd()}…` : text;
};

// eslint-disable-next-line no-control-regex
const CONTROLS = /[\u0000-\u001f\u007f-\u009f\u2028\u2029]+/g;
/** HR text without control characters (HQ refuses them) and trimmed the way HQ trims. */
export const plain = (text: string | undefined): string => trimWhiteSpace((text ?? '').replace(CONTROLS, ' '));
/** A display name HQ will take: plain, and not longer than an organization name (64 characters). */
const cleanName = (name: string | undefined): string => [...plain(name)].slice(0, 64).join('');

/** What an HR record says about the agent. HR agents are Claude Code subagents (the toolkit registers them as such). */
function fromHr(record: HrAgent): Partial<Row> {
	return {
		...(roleWords(plain(record.role)) ? { role: roleWords(plain(record.role)) } : {}),
		...(plain(record.title) ? { title: plain(record.title) } : {}),
		...(plain(record.focus) ? { focus: clip(plain(record.focus)) } : {}),
		runtime: 'claude_code',
		// "inherit" is HR saying it sets no model.
		...(plain(record.model) && lower(plain(record.model)) !== 'inherit' ? { model: clip(plain(record.model)) } : {}),
		...(record.home_path ? { home: record.home_path } : {}),
		...(record.department ? { teamKey: record.department } : {})
	};
}

/**
 * Why `name` can't be an agent's messaging name, in plain words, or nothing when it can (org-proto: 1 to 64 ASCII letters,
 * digits, `-`, `_` and `.`, starting with a letter or digit; not a reserved word; not shaped like a member id).
 */
export function handleProblem(name: string): string | undefined {
	if (name.length > 64) return 'Messaging names are at most 64 characters.';
	if (NOT_AGENTS.has(lower(name))) return `${name} is reserved.`;
	if (/^[A-Za-z0-9]{26}$/.test(name)) return 'This looks like a member id. Choose another name.';
	if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name)) return 'Messaging names use letters, digits, - _ and ., and start with a letter or digit.';
	return undefined;
}

/**
 * Whether `name` can be an agent's messaging name, as org-proto has it: 1 to 64 ASCII letters, digits, `-`, `_` and `.`,
 * starting with a letter or digit, and not shaped like an address (26 letters and digits, a member id's shape).
 */
export function isValidHandle(name: string): boolean {
	return handleProblem(name) === undefined;
}

/** What a row still needs from the owner before it can be brought in. */
export function missingOf(row: Row): ('role' | 'runtime' | 'name')[] {
	const missing: ('role' | 'runtime' | 'name')[] = [];
	if (!isValidHandle(row.messagingName)) missing.push('name');
	// Both are required, and at most TEXT_MAX long (HQ refuses more).
	if (!row.role?.trim() || !row.title?.trim() || row.role.trim().length > TEXT_MAX || row.title.trim().length > TEXT_MAX) missing.push('role');
	if (row.runtime === undefined || row.runtime === 'unknown') missing.push('runtime');
	return missing;
}

/** Whether a row takes part in the import: chosen, complete, and not a person's name. It may already be in (an agent from an earlier try). */
export const takesPart = (row: Row): boolean => row.choice !== 'skip' && row.existingKind !== 'person' && !row.existingOffboarded && !row.removedEarlier && missingOf(row).length === 0;

/** Whether a row will be brought in as it is: chosen, not already in, and nothing missing. */
export const isReady = (row: Row): boolean => row.choice !== 'skip' && !row.existingMemberId && !row.removedEarlier && missingOf(row).length === 0;

/** Whether the row still has to be filled in to be brought in. */
export const needsDetails = (row: Row): boolean => row.choice !== 'skip' && !row.existingMemberId && !row.removedEarlier && missingOf(row).length > 0;

/** An HR-only agent has no messaging inbox: say so and let the owner pick (Active · Paused · Skip). */
export const hasNoMessages = (row: Row, scan: Scan): boolean => row.source === 'hr' && !!scan.messaging && !scan.messaging.error;

/** The button's counts: how many will be brought in, and how many are waiting on details. */
export function counts(rows: Row[]): { ready: number; needDetails: number; alreadyIn: number; paused: number } {
	return {
		ready: rows.filter(isReady).length,
		needDetails: rows.filter(needsDetails).length,
		alreadyIn: rows.filter((r) => r.existingMemberId).length,
		paused: rows.filter((r) => isReady(r) && r.choice === 'paused').length
	};
}

// ---- role and title (rev 14): both are required by the agent record; the Role comes from the catalog (HR's role files).

const words = (text: string) => lower(text).split(/[^a-z0-9]+/).filter(Boolean);

/** The catalog role whose words overlap the title's the most, when one does clearly (not a tie): "Staff Engineer" -> "staff engineer". */
export function suggestRole(title: string, roles: string[]): string | undefined {
	const wanted = new Set(words(title));
	const scored = roles.map((slug) => ({ role: roleWords(slug)!, hits: words(slug).filter((w) => wanted.has(w)).length })).filter((r) => r.hits > 0);
	const best = Math.max(0, ...scored.map((r) => r.hits));
	const top = scored.filter((r) => r.hits === best);
	return top.length === 1 ? top[0].role : undefined;
}

/** "software engineer" -> "Software Engineer". */
const titleCase = (text: string): string => text.replace(/\b\p{L}/gu, (c) => c.toUpperCase());

/** The owner picked a role: the title follows its words until the owner gives their own. */
export function chooseRole(row: Row, role: string): Row {
	const keepsTitle = !!row.title && row.title !== titleCase(row.role ?? '');
	return { ...row, role, roleSuggested: false, title: keepsTitle ? row.title : titleCase(role) };
}

/** "Custom title…": the title is the owner's own, and an empty role gets the catalog's best match for it, marked Suggested. */
export function chooseTitle(row: Row, title: string, roles: string[]): Row {
	const next: Row = { ...row, title: plain(title) };
	if (!next.role?.trim()) {
		const suggested = suggestRole(next.title ?? '', roles);
		if (suggested) return { ...next, role: suggested, roleSuggested: true };
	}
	return next;
}
