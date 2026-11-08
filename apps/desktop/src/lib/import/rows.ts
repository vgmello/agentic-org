// "Bring in an existing team": from what the scan found to the rows the owner reviews (spec our-org-in-the-app §4).
// Pure: nothing here reads a file or calls the office. The scan's two sources are merged by name, one row per agent.

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
	title?: string;
	focus?: string;
	runtime?: AgentRuntime;
	model?: string;
	home?: string;
	/** The row of its team: set from HR's department, changed by the owner. */
	teamKey?: string;
	/** Who it reports to: another row's key, or `owner`. */
	managerKey: string;
	choice: Choice;
	/** The member already in the organization with this messaging name ("Already in <org>"). */
	existingMemberId?: string;
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
	const records = new Map(hr.filter((a) => !offboarded.includes(a)).map((a) => [lower(a.id), a]));
	// The same name in other capitals is the same handle (HQ compares them ignoring case): the first one is kept.
	const handles = new Map<string, string>();
	for (const name of names) if (!skip.has(lower(name)) && !handles.has(lower(name))) handles.set(lower(name), name);
	const existing = new Map(
		members.filter((m) => m.kind === 'agent' && m.agent).map((m) => [lower(m.agent!.messaging_name), m.id])
	);

	const keys = new Set([...records.keys(), ...handles.keys()]);
	const rows: Row[] = [...keys].sort().map((key) => {
		const record = records.get(key);
		const handle = handles.get(key);
		const messagingName = handle ?? record!.id;
		return {
			key,
			name: record?.name || messagingName,
			messagingName,
			source: record && handle ? 'both' : record ? 'hr' : 'messaging',
			...(record ? { hr: record } : {}),
			...(record ? fromHr(record) : {}),
			managerKey: OWNER,
			choice: 'active',
			...(existing.has(key) ? { existingMemberId: existing.get(key) } : {})
		};
	});
	// Reporting lines from HR: to another row's agent, or to the owner.
	const byHrId = new Map(rows.filter((r) => r.hr).map((r) => [lower(r.hr!.id), r.key]));
	for (const row of rows) {
		const boss = row.hr?.reports_to;
		const bossKey = boss ? byHrId.get(lower(boss)) : undefined;
		if (bossKey && bossKey !== row.key) row.managerKey = bossKey;
	}
	return { rows, offboarded };
}

/** What an HR record says about the agent. HR agents are Claude Code subagents (the toolkit registers them as such). */
function fromHr(record: HrAgent): Partial<Row> {
	return {
		...(roleWords(record.role) ? { role: roleWords(record.role) } : {}),
		...(record.title ? { title: record.title } : {}),
		...(record.focus ? { focus: record.focus } : {}),
		runtime: 'claude_code',
		// "inherit" is HR saying it sets no model.
		...(record.model && lower(record.model) !== 'inherit' ? { model: record.model } : {}),
		...(record.home_path ? { home: record.home_path } : {}),
		...(record.department ? { teamKey: record.department } : {})
	};
}

/** What a row still needs from the owner before it can be brought in. */
export function missingOf(row: Row): ('role' | 'runtime')[] {
	const missing: ('role' | 'runtime')[] = [];
	if (!row.role?.trim() || !row.title?.trim()) missing.push('role');
	if (row.runtime === undefined || row.runtime === 'unknown') missing.push('runtime');
	return missing;
}

/** Whether a row will be brought in as it is: chosen, not already in, and nothing missing. */
export const isReady = (row: Row): boolean => row.choice !== 'skip' && !row.existingMemberId && missingOf(row).length === 0;

/** Whether the row still has to be filled in to be brought in. */
export const needsDetails = (row: Row): boolean => row.choice !== 'skip' && !row.existingMemberId && missingOf(row).length > 0;

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
