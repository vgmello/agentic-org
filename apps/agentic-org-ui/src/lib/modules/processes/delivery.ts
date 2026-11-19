// What an agent is handed at the Starting a task and Waking up moments, and what a new task's checklist takes from the set
// (spec §3, PROC-7, PROC-12). Pure: the words and the merging, not the delivery.

import type { ChecklistStep, EffectiveSet, Entry, Level } from './types';

/** "org v3", "Checkout v2", "chk v1", "Agent v1": where a version comes from, short. */
export function sourceShort(entry: Pick<Entry, 'source' | 'scopeName' | 'doc'>): string {
	const where = entry.source === 'organization' ? 'org' : entry.source === 'agent' ? 'Agent' : (entry.scopeName ?? entry.source);
	return `${where} v${entry.doc.version}`;
}

/** What an agent last read of a key: the doc and version. */
export type Reads = Record<string, { docId: string; version: number }>;

/** A process's last read: an own property only (a key such as `constructor` is not a read). */
export const readOf = (reads: Reads, key: string) => (Object.hasOwn(reads, key) ? reads[key] : undefined);

/** The processes whose winning file is not the one the agent last read (a new version, a new winner and a conflict fallback all count). */
export const changedSince = (set: EffectiveSet, reads: Reads): Entry[] => set.entries.filter((e) => readOf(reads, e.key)?.docId !== e.doc.id || readOf(reads, e.key)?.version !== e.doc.version);

/**
 * "Your processes for chk-T-17: Branch, review and merge (org v3), Code review (Checkout v2). Changed since you last read: Branch,
 * review and merge v2 → v3." (`taskRef` undefined: no current work.)
 */
export function startLine(set: EffectiveSet, reads: Reads, taskRef?: string): string {
	const list = set.entries.map((e) => `${e.doc.name} (${sourceShort(e)})`).join(', ') || 'none';
	const off = set.turnedOff.map((t) => `${t.name} (turned off${t.scopeName ? ` for ${t.scopeName}` : ''}: ${t.reason})`);
	const conflicts = set.conflicts.map((c) => `${c.key}: ${c.teams.join(' and ')} disagree. ${c.applies}`);
	const head = `Your processes${taskRef ? ` for ${taskRef}` : ''}: ${list}.${off.length ? ` Turned off: ${off.join(', ')}.` : ''}${conflicts.length ? ` Conflicts: ${conflicts.join('; ')}.` : ''}`;
	const changed = changedSince(set, reads);
	if (changed.length === 0) return head;
	const words = changed.map((e) => {
		const read = readOf(reads, e.key);
		return read ? `${e.doc.name} v${read.version} → v${e.doc.version}` : `${e.doc.name} (new, v${e.doc.version})`;
	});
	return `${head} Changed since you last read: ${words.join(', ')}.`;
}

export type Moment = 'starting_task' | 'waking_up' | 'finishing_task';

/** The prompts of the set for a moment: shown after the moment's guidance text, and only while the moment is On. */
export function momentPrompts(set: EffectiveSet, moment: Moment, momentOn: boolean): { name: string; text: string }[] {
	if (!momentOn) return [];
	return set.entries.flatMap((e) => {
		const text = e.doc.moments?.[moment];
		return text ? [{ name: e.doc.name, text }] : [];
	});
}

/**
 * A new task's checklist: the project's default steps, then the steps of the project's processes (organization, team chain and
 * project, never private ones); a step with the same text is merged, and required wins.
 */
export function taskChecklist(set: EffectiveSet, projectDefault: ChecklistStep[]): ChecklistStep[] {
	const result: ChecklistStep[] = [];
	const add = (step: ChecklistStep) => {
		const found = result.find((s) => s.text.trim().toLowerCase() === step.text.trim().toLowerCase());
		if (found) found.required = found.required || step.required;
		else result.push({ ...step });
	};
	projectDefault.forEach(add);
	// Organization, then the team chain from the outermost parent down, then the project; within one place, by name.
	const order: Level[] = ['organization', 'team', 'project'];
	const chainOrdered = [...set.entries]
		.filter((e) => e.source !== 'agent')
		.sort((a, b) => order.indexOf(a.source) - order.indexOf(b.source) || (a.depth ?? 0) - (b.depth ?? 0));
	for (const entry of chainOrdered) (entry.doc.checklist ?? []).forEach(add);
	return result;
}
