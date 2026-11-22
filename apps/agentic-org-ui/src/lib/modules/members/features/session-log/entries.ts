// The session log's one entry model (REQ-05 "One event model, two parsers"): a Claude parser and a Codex parser
// each turn their vendor's conversation file into this list, and the view and its toggles never see the format.

export interface DiffLine {
	op: 'add' | 'remove' | 'context';
	text: string;
}

export type FileAction = 'read' | 'edit' | 'create' | 'delete' | 'reference';

interface Base {
	/** Stable within one log: the position in the file. */
	id: string;
	/** RFC 3339, when the file recorded it. */
	at?: string;
}

export type SessionEntry =
	| (Base & { kind: 'message'; role: 'user' | 'assistant' | 'system'; text: string })
	| (Base & { kind: 'tool_call'; callId?: string; tool: string; summary: string; input: string })
	| (Base & { kind: 'tool_result'; callId?: string; ok: boolean; output: string })
	| (Base & { kind: 'file'; path: string; action: FileAction; diff?: DiffLine[] })
	| (Base & {
			kind: 'usage';
			inputTokens?: number;
			outputTokens?: number;
			cachedTokens?: number;
			complete: boolean;
	  })
	| (Base & { kind: 'notice'; text: string })
	/** A line the parser does not recognise: shown as it was, never dropped (REQ-05 settle 1). */
	| (Base & { kind: 'raw'; source: string });

export type SessionFormat = 'claude' | 'codex';

export interface SessionLog {
	runId: string;
	memberId: string;
	format: SessionFormat;
	/** The tool version the parser was tested with, as the file states it. */
	toolVersion?: string;
	startedAt?: string;
	entries: SessionEntry[];
}

export const isToolEntry = (e: SessionEntry) => e.kind === 'tool_call' || e.kind === 'tool_result';

/** What the toggles leave on screen: tool calls and results, and file entries, can each be hidden. */
export function visibleEntries(
	entries: SessionEntry[],
	show: { tools: boolean; files: boolean }
): SessionEntry[] {
	return entries.filter(
		(e) => (show.tools || !isToolEntry(e)) && (show.files || e.kind !== 'file')
	);
}

export interface TouchedFile {
	path: string;
	/** The strongest thing done to it: create/delete/edit over read/reference. */
	action: FileAction;
	added: number;
	removed: number;
	/** Entry ids that mention it, in order. */
	entryIds: string[];
}

const STRENGTH: Record<FileAction, number> = {
	reference: 0,
	read: 1,
	edit: 2,
	create: 3,
	delete: 3
};

/** The touched-files list: each file once, in first-touched order, with its added/removed line counts. */
export function touchedFiles(entries: SessionEntry[]): TouchedFile[] {
	const byPath = new Map<string, TouchedFile>();
	for (const e of entries) {
		if (e.kind !== 'file') continue;
		const file = byPath.get(e.path) ?? {
			path: e.path,
			action: e.action,
			added: 0,
			removed: 0,
			entryIds: []
		};
		if (STRENGTH[e.action] > STRENGTH[file.action]) file.action = e.action;
		file.added += e.diff?.filter((l) => l.op === 'add').length ?? 0;
		file.removed += e.diff?.filter((l) => l.op === 'remove').length ?? 0;
		file.entryIds.push(e.id);
		byPath.set(e.path, file);
	}
	return [...byPath.values()];
}

/** Token totals across usage entries; incomplete if any entry is (unknown usage is never zero). */
export function usageTotals(entries: SessionEntry[]) {
	let input = 0;
	let output = 0;
	let complete = true;
	let any = false;
	for (const e of entries) {
		if (e.kind !== 'usage') continue;
		any = true;
		input += e.inputTokens ?? 0;
		output += e.outputTokens ?? 0;
		if (!e.complete || e.inputTokens === undefined || e.outputTokens === undefined)
			complete = false;
	}
	return any ? { input, output, complete } : undefined;
}

/** Diff lines from a unified patch body (`+`, `-`, ` ` prefixes; headers dropped). */
export function diffFromPatch(lines: string[]): DiffLine[] {
	const out: DiffLine[] = [];
	for (const line of lines) {
		if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('@@')) continue;
		if (line.startsWith('+')) out.push({ op: 'add', text: line.slice(1) });
		else if (line.startsWith('-')) out.push({ op: 'remove', text: line.slice(1) });
		else if (line.startsWith(' ')) out.push({ op: 'context', text: line.slice(1) });
	}
	return out;
}

/** A before/after replacement as removed then added lines (Claude's Edit tool). */
export function diffFromReplace(before: string, after: string): DiffLine[] {
	const split = (s: string) => (s === '' ? [] : s.split('\n'));
	return [
		...split(before).map((text) => ({ op: 'remove' as const, text })),
		...split(after).map((text) => ({ op: 'add' as const, text }))
	];
}

/** Long tool output is cut for the row; the full text stays one click away. */
export const PREVIEW_LINES = 12;
