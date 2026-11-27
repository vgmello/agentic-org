import { badgeInitials as initialsOf } from '$lib/format';
import { matchesQuery } from '$lib/modules/activity/filter';
import type { ActivityKind, ActivityQuery, ActivityRow } from '$lib/modules/activity/types';
import { KIND_WORD } from '$lib/modules/activity/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import { dayKey, formatDate } from '$lib/modules/preferences';
import { memberById, owner } from '$lib/modules/organization/model';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import {
	belongsOnlyToArchivedProjects,
	buildReviewInboxLookup,
	classifyAssemblyStage,
	extractWorkRefFromPath,
	extractWorkRefsFromText,
	inboxItemHref,
	isActionableWorkItem,
	isExcludedTerminalItem,
	resolveInboxProjectId,
	workItemHref
} from './assembly-stages';
import type { FeedPhase, StreamDayGroup, StreamEntry, StreamFilterTab } from './assembly-types';

function matchInboxForRow(
	row: ActivityRow,
	reviewInboxByRef: ReadonlyMap<string, InboxItem>,
	openInbox: readonly InboxItem[]
): InboxItem | undefined {
	const pathRef = extractWorkRefFromPath(row.href?.path);
	if (pathRef) {
		const found = reviewInboxByRef.get(pathRef);
		if (found) return found;
		return row.href?.path
			? openInbox.find((i) => i.kind !== 'problem' && i.open?.path === row.href?.path)
			: undefined;
	}
	if (row.href?.path) {
		const exact = openInbox.find((i) => i.open?.path === row.href?.path);
		if (exact) return exact;
	}
	for (const ref of extractWorkRefsFromText(row.text)) {
		const found = reviewInboxByRef.get(ref);
		if (found) return found;
	}
	return undefined;
}

function rowTone(row: ActivityRow, needsYou: boolean): StreamEntry['tone'] {
	if (row.run?.state === 'failed') return 'bad';
	if (needsYou) return 'warn';
	if (row.action === 'task-done' || row.action === 'merge' || row.run?.state === 'succeeded') {
		return 'ok';
	}
	return row.kind === 'conversation' ? 'sky' : 'accent';
}

function mapActivityRowToStreamEntry(
	row: ActivityRow,
	projectsById: ReadonlyMap<string, string>,
	inboxByRef: ReadonlyMap<string, InboxItem>,
	scopedOpenInbox: readonly InboxItem[],
	actionableWorkRefs: ReadonlySet<string>
): StreamEntry {
	const matchedInbox = matchInboxForRow(row, inboxByRef, scopedOpenInbox);
	const pathRef = extractWorkRefFromPath(row.href?.path);
	const rowRefs = pathRef ? [pathRef] : extractWorkRefsFromText(row.text);
	const needsYou = Boolean(matchedInbox) || rowRefs.some((ref) => actionableWorkRefs.has(ref));

	return {
		id: row.id,
		kind: row.kind,
		kindLabel: KIND_WORD[row.kind],
		actorName: row.actor.name,
		actorInitials: initialsOf(row.actor.name),
		actorKind: row.actor.kind,
		text: row.text,
		projects: row.projectIds
			.map((pid) => projectsById.get(pid))
			.filter((name): name is string => Boolean(name)),
		preview: row.preview ?? row.run?.outcome ?? row.run?.trigger,
		at: row.at,
		tone: rowTone(row, needsYou),
		href: row.href?.path ?? '/work',
		needsYou,
		inboxHref: matchedInbox ? inboxItemHref(matchedInbox.id) : undefined,
		inboxLabel: matchedInbox ? 'Review in Inbox' : undefined,
		inboxTitle: matchedInbox?.title,
		run: row.run
			? {
					id: row.run.id,
					state: row.run.state,
					trigger: row.run.trigger,
					outcome: row.run.outcome,
					log: row.run.log
				}
			: undefined
	};
}

function workItemTone(stage: string | null): StreamEntry['tone'] {
	if (stage === 'merged') return 'ok';
	if (stage === 'review') return 'warn';
	if (stage === 'ci') return 'sky';
	return 'accent';
}

export function deriveStreamEntries(
	snapshot: OrgSnapshot,
	scopedActivityRows: readonly ActivityRow[],
	scopedWorkItems: readonly WorkItemSummary[],
	scopedOpenInbox: readonly InboxItem[],
	inboxByRef: ReadonlyMap<string, InboxItem>,
	actionableWorkRefs: ReadonlySet<string>,
	ownerId: string,
	feedPhase: FeedPhase = 'ready',
	workPhase: FeedPhase = 'ready'
): StreamEntry[] {
	if (feedPhase !== 'ready') return [];

	const projectsById = new Map((snapshot.projects ?? []).map((p) => [p.id, p.name]));

	if (scopedActivityRows.length > 0) {
		return [...scopedActivityRows]
			.sort((a, b) => Date.parse(b.at) - Date.parse(a.at))
			.map((row) =>
				mapActivityRowToStreamEntry(
					row,
					projectsById,
					inboxByRef,
					scopedOpenInbox,
					actionableWorkRefs
				)
			);
	}

	if (workPhase !== 'ready') return [];

	return [...scopedWorkItems]
		.sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at))
		.map((item): StreamEntry => {
			const actor = memberById(snapshot, item.assignee_id) ?? memberById(snapshot, item.created_by);
			const actorName = actor?.name ?? 'Team';
			const projName = item.project_id ? projectsById.get(item.project_id) : undefined;
			const stage = classifyAssemblyStage(item, inboxByRef, ownerId);
			const linkedInbox =
				inboxByRef.get(item.short_id.toUpperCase()) ?? inboxByRef.get(item.id.toUpperCase());
			const needsYou = Boolean(linkedInbox) || isActionableWorkItem(item, ownerId);
			const kind: ActivityKind = item.kind === 'request' ? 'decision' : 'work';

			return {
				id: item.id,
				kind,
				kindLabel: KIND_WORD[kind],
				actorName,
				actorInitials: initialsOf(actorName),
				actorKind: actor?.kind === 'person' ? 'person' : 'agent',
				text: `${item.short_id} ${item.title} (${item.status.replace('_', ' ')})`,
				projects: projName ? [projName] : [],
				preview: item.blocked_reason ?? linkedInbox?.title,
				at: item.updated_at,
				tone: workItemTone(stage),
				href: workItemHref(item),
				needsYou,
				inboxHref: linkedInbox ? inboxItemHref(linkedInbox.id) : undefined,
				inboxLabel: linkedInbox ? 'Review in Inbox' : undefined,
				inboxTitle: linkedInbox?.title
			};
		});
}

const weekdayFormats = new Map<string, Intl.DateTimeFormat>();

function weekdayShort(date: Date, zone: string): string {
	let fmt = weekdayFormats.get(zone);
	if (!fmt) {
		fmt = new Intl.DateTimeFormat('en-GB', { timeZone: zone, weekday: 'short' });
		weekdayFormats.set(zone, fmt);
	}
	return fmt.format(date);
}

/** Exact calendar-day subtraction (`YYYY-MM-DD`) so 23h/25h DST transitions never skip Yesterday. */
function previousCalendarDayKey(isoDayKey: string): string {
	const [year, month, day] = isoDayKey.split('-').map(Number);
	const prev = new Date(Date.UTC(year, month - 1, day - 1));
	const y = String(prev.getUTCFullYear()).padStart(4, '0');
	const m = String(prev.getUTCMonth() + 1).padStart(2, '0');
	const d = String(prev.getUTCDate()).padStart(2, '0');
	return `${y}-${m}-${d}`;
}

function dayGroupLabel(
	date: Date | undefined,
	key: string,
	todayKey: string,
	yesterdayKey: string,
	zone: string,
	now: Date
): string {
	if (!date) return 'Earlier';
	if (key === todayKey) return 'Today';
	if (key === yesterdayKey) return 'Yesterday';
	return `${weekdayShort(date, zone)} ${formatDate(date, zone, now)}`;
}

/**
 * Groups stream entries chronologically by calendar day (`Today`, `Yesterday`, or formatted date)
 * in `zone`, sorting newest-first within and across day sections (`poc` commit `580b6715`).
 */
export function groupStreamEntriesByDay(
	entries: readonly StreamEntry[],
	nowMs: number,
	zone = 'UTC'
): StreamDayGroup[] {
	if (entries.length === 0) return [];
	const now = new Date(nowMs);
	const todayKey = dayKey(now, zone);
	const yesterdayKey = previousCalendarDayKey(todayKey);
	const ordered = entries
		.map((entry, index) => {
			const ms = Date.parse(entry.at);
			return { entry, index, ms: Number.isNaN(ms) ? -Infinity : ms };
		})
		.sort((a, b) => (a.ms !== b.ms ? b.ms - a.ms : a.index - b.index));

	const groups: StreamDayGroup[] = [];
	const byKey = new Map<string, StreamDayGroup>();
	for (const { entry, ms } of ordered) {
		const date = Number.isFinite(ms) ? new Date(ms) : undefined;
		const key = date ? dayKey(date, zone) : 'earlier';
		let group = byKey.get(key);
		if (!group) {
			const label = dayGroupLabel(date, key, todayKey, yesterdayKey, zone, now);
			group = { key, label, entries: [] };
			byKey.set(key, group);
			groups.push(group);
		}
		group.entries.push(entry);
	}
	return groups;
}

export function filterStreamEntries(
	entries: readonly StreamEntry[],
	tab: StreamFilterTab
): StreamEntry[] {
	if (tab === 'all') return [...entries];
	if (tab === 'needs_you') return entries.filter((e) => e.needsYou);
	return entries.filter((e) => e.kind === tab);
}

function isTerminalRunState(state: 'running' | 'succeeded' | 'failed'): boolean {
	return state === 'succeeded' || state === 'failed';
}

function compareRunPreference(
	existingRun: NonNullable<ActivityRow['run']>,
	incomingRun: NonNullable<ActivityRow['run']>
): boolean | undefined {
	const existingTerminal = isTerminalRunState(existingRun.state);
	const incomingTerminal = isTerminalRunState(incomingRun.state);
	if (existingTerminal !== incomingTerminal) return incomingTerminal;

	if (existingRun.ended_at && !incomingRun.ended_at) return false;
	if (!existingRun.ended_at && incomingRun.ended_at) return true;
	if (existingRun.ended_at && incomingRun.ended_at) {
		const existingEnd = Date.parse(existingRun.ended_at);
		const incomingEnd = Date.parse(incomingRun.ended_at);
		if (!Number.isNaN(existingEnd) && !Number.isNaN(incomingEnd) && incomingEnd !== existingEnd) {
			return incomingEnd > existingEnd;
		}
	}
	return undefined;
}

function shouldReplaceActivityRow(existing: ActivityRow, incoming: ActivityRow): boolean {
	if (existing.run && incoming.run) {
		const runDecision = compareRunPreference(existing.run, incoming.run);
		if (runDecision !== undefined) return runDecision;
	}
	const existingAt = Date.parse(existing.at);
	const incomingAt = Date.parse(incoming.at);
	if (!Number.isNaN(existingAt) && !Number.isNaN(incomingAt) && incomingAt !== existingAt) {
		return incomingAt > existingAt;
	}
	return true;
}

/**
 * Merges live subscription rows into an existing feed slice via `matchesQuery`, upserting by ID
 * while preserving newer/terminal rows and tracking evicted rows in `more`.
 */
export function mergeLiveActivityRows(
	baseRows: readonly ActivityRow[],
	incomingRows: readonly ActivityRow[],
	query: ActivityQuery,
	limit: number,
	currentMore: number
): { rows: ActivityRow[]; more: number; changed: boolean } {
	let rows = [...baseRows];
	let more = currentMore;
	let changed = false;

	for (const incoming of incomingRows) {
		if (!matchesQuery(incoming, query)) continue;
		const idx = rows.findIndex((r) => r.id === incoming.id);
		if (idx >= 0) {
			if (shouldReplaceActivityRow(rows[idx], incoming)) {
				rows[idx] = incoming;
				changed = true;
			}
		} else {
			rows.unshift(incoming);
			changed = true;
		}
	}

	if (!changed) return { rows, more, changed: false };

	rows.sort((a, b) => Date.parse(b.at) - Date.parse(a.at));
	if (rows.length > limit) {
		more += rows.length - limit;
		rows = rows.slice(0, limit);
	}
	return { rows, more, changed: true };
}

/**
 * Derives stream entries specifically for the `Decisions` tab (`kind: 'decision'`), which
 * `matchesQuery` excludes from the default `kind: 'all'` feed.
 */
export function deriveDecisionStreamEntries(input: {
	snapshot: OrgSnapshot;
	decisionRows: readonly ActivityRow[];
	workItems: readonly WorkItemSummary[];
	inboxItems: readonly InboxItem[];
	projectScope: string;
	ownerId?: string;
	decisionPhase: FeedPhase;
	workPhase: FeedPhase;
	inboxPhase: FeedPhase;
}): StreamEntry[] {
	const {
		snapshot,
		decisionRows,
		workItems,
		inboxItems,
		projectScope,
		decisionPhase,
		workPhase,
		inboxPhase
	} = input;

	if (decisionPhase !== 'ready') return [];

	const resolvedOwnerId = input.ownerId ?? owner(snapshot)?.id ?? 'owner';
	const archivedProjectIds = new Set(
		(snapshot.projects ?? []).filter((p) => p.archived).map((p) => p.id)
	);
	const scopedProjectId = projectScope !== 'all' ? projectScope : undefined;

	const scopedDecisionRows = scopedProjectId
		? decisionRows.filter((row) => row.projectIds.includes(scopedProjectId))
		: decisionRows.filter(
				(row) => !belongsOnlyToArchivedProjects(row.projectIds, archivedProjectIds)
			);
	if (scopedDecisionRows.length === 0) return [];

	const workByRef = new Map<string, WorkItemSummary>();
	for (const item of workItems) {
		if (item.deleted) continue;
		workByRef.set(item.short_id.toUpperCase(), item);
		workByRef.set(item.id.toUpperCase(), item);
	}

	const allOpenInbox = inboxPhase === 'ready' ? inboxItems.filter((i) => i.state === 'open') : [];
	const scopedOpenInbox = scopedProjectId
		? allOpenInbox.filter((i) => resolveInboxProjectId(i, workByRef) === scopedProjectId)
		: allOpenInbox.filter((i) => {
				const pid = resolveInboxProjectId(i, workByRef);
				return !pid || !archivedProjectIds.has(pid);
			});
	const reviewInboxByRef = buildReviewInboxLookup(scopedOpenInbox);

	const actionableWorkRefs = new Set<string>();
	for (const item of workItems) {
		if (isExcludedTerminalItem(item) || !isActionableWorkItem(item, resolvedOwnerId)) continue;
		if (scopedProjectId && item.project_id !== scopedProjectId) continue;
		if (!scopedProjectId && item.project_id && archivedProjectIds.has(item.project_id)) continue;
		actionableWorkRefs.add(item.short_id.toUpperCase());
		actionableWorkRefs.add(item.id.toUpperCase());
	}

	return deriveStreamEntries(
		snapshot,
		scopedDecisionRows,
		[],
		scopedOpenInbox,
		reviewInboxByRef,
		actionableWorkRefs,
		resolvedOwnerId,
		decisionPhase,
		workPhase
	);
}
