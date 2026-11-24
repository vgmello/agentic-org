import { isItemDone, initialsOf } from '$lib/home/model';
import { matchesQuery } from '$lib/modules/activity/filter';
import type { ActivityKind, ActivityQuery, ActivityRow } from '$lib/modules/activity/types';
import { KIND_WORD } from '$lib/modules/activity/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import { dayKey, formatDate } from '$lib/modules/preferences';
import {
	agentsOf,
	byName,
	isFormer,
	memberById,
	nonOwners,
	owner
} from '$lib/modules/organization/model';
import type { Area, OrgSnapshot, ProjectSummary, WorkItemSummary } from '$lib/proto';

export type AssemblyStage = 'queue' | 'agents' | 'ci' | 'review' | 'merged';
export type StreamFilterTab = 'all' | 'needs_you' | ActivityKind;
export type FeedPhase = 'loading' | 'ready' | 'error';
export type ThroughputSource =
	'events' | 'events_capped' | 'work_updates' | 'loading' | 'unavailable';

export interface AssemblyChip {
	id: string;
	shortId: string;
	title: string;
	stage: AssemblyStage;
	href: string;
	agentFirstName?: string;
	inboxId?: string;
	tooltip: string;
}

export interface AssemblyLane {
	id: string;
	name: string;
	subtitle: string;
	stages: Record<AssemblyStage, AssemblyChip[]>;
	doneCount: number;
	totalCount: number;
	pct: number;
	isComplete: boolean;
}

export interface AssemblyMilestone {
	id: string;
	name: string;
	key?: string;
	summary: string;
	doneCount: number;
	totalCount: number;
	pct: number;
	isComplete: boolean;
	lanes: AssemblyLane[];
}

export interface BottleneckSummary {
	totalOpen: number;
	agentsPct: number;
	ciPct: number;
	reviewPct: number;
	queuePct: number;
	legend: string;
	note: string;
}

export interface ReleaseGateRow {
	id: string;
	label: string;
	statusLabel: string;
	tone: 'ok' | 'warn' | 'bad' | 'neutral';
	count: number;
	href: string;
}

export interface ThroughputDay {
	key: string;
	label: string;
	count: number;
	heightPct: number;
	isToday: boolean;
}

export interface RailAgentRow {
	id: string;
	name: string;
	initials: string;
	title: string;
	status: 'building' | 'waiting' | 'paused' | 'idle';
	statusLabel: string;
	currentWork?: {
		shortId: string;
		title: string;
		href: string;
	};
	href: string;
}

export interface StreamEntry {
	id: string;
	kind: ActivityKind;
	kindLabel: string;
	actorName: string;
	actorInitials: string;
	actorKind: 'agent' | 'person';
	text: string;
	projects: string[];
	preview?: string;
	at: string;
	tone: 'ok' | 'warn' | 'bad' | 'accent' | 'sky';
	href: string;
	needsYou: boolean;
	inboxHref?: string;
	inboxLabel?: string;
	inboxTitle?: string;
	run?: {
		id: string;
		state: 'running' | 'succeeded' | 'failed';
		trigger: string;
		outcome?: string;
		log?: string[];
	};
}

export interface ActivityCommandCenterModel {
	activeProjects: ProjectSummary[];
	selectedProject?: ProjectSummary;
	scopeTitle: string;
	scopeEyebrow: string;
	scopeSubtitle: string;
	liveStatusText: string;
	stageCounts: Record<AssemblyStage, number>;
	ciColumnSubLabel: string;
	ciStationLabel: string;
	totalItems: number;
	openItems: number;
	mergedItems: number;
	readinessPct: number;
	isComplete: boolean;
	busyAgentsCount: number;
	activeAgentsCount: number;
	needsYouCount: number;
	openInboxCount: number;
	actionableWorkCount: number;
	milestones: AssemblyMilestone[];
	bottleneck: BottleneckSummary;
	releaseGates: ReleaseGateRow[];
	pendingApprovals: { id: string; title: string; href: string }[];
	stationsNote: string;
	throughput7d: ThroughputDay[];
	throughputSource: ThroughputSource;
	throughputMeta: string;
	throughputAriaLabel: string;
	throughputUnavailableMessage?: string;
	railAgents: RailAgentRow[];
	extraAgentsCount: number;
	streamEntries: StreamEntry[];
	streamNeedsYouCount: number;
}

export interface CommandCenterInput {
	snapshot: OrgSnapshot;
	organizationName: string;
	workItems: readonly WorkItemSummary[];
	inboxItems: readonly InboxItem[];
	activityRows: readonly ActivityRow[];
	projectScope: string;
	ownerId?: string;
	feedPhase?: FeedPhase;
	feedMore?: number;
	workPhase?: FeedPhase;
	inboxPhase?: FeedPhase;
	nowMs?: number;
}

const DAY_MS = 86_400_000;
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'] as const;

export function computeCompletionPct(done: number, total: number): number {
	if (total <= 0 || done <= 0) return 0;
	if (done === total) return 100;
	return Math.min(99, Math.round((Math.min(done, total) * 100) / total));
}

export function workItemHref(item: WorkItemSummary): string {
	return item.kind === 'request'
		? `/work?request=${encodeURIComponent(item.id)}`
		: `/work/${encodeURIComponent(item.short_id)}`;
}

export function inboxItemHref(inboxId: string): string {
	return `/inbox?item=${encodeURIComponent(inboxId)}`;
}

export function extractWorkRefsFromText(text: string | undefined): string[] {
	if (!text) return [];
	const matches = text.match(/\b[A-Z]+-\d+\b/g);
	return matches ? matches.map((m) => m.toUpperCase()) : [];
}

export function extractWorkRefFromPath(path: string | undefined): string | undefined {
	if (!path) return undefined;
	const workMatch = path.match(/\/work\/([^/?#]+)/);
	if (workMatch) {
		try {
			return decodeURIComponent(workMatch[1]).toUpperCase();
		} catch {
			return workMatch[1].toUpperCase();
		}
	}
	const reqMatch = path.match(/[?&]request=([^&#]+)/);
	if (reqMatch) {
		try {
			return decodeURIComponent(reqMatch[1]).toUpperCase();
		} catch {
			return reqMatch[1].toUpperCase();
		}
	}
	return undefined;
}

/**
 * Extracts work item references for an Inbox item.
 * When `item.open?.path` explicitly identifies a work subject (`/work/<ref>` or `?request=<id>`),
 * return ONLY that explicit subject ref so mentioned tickets in the title are not misattributed.
 * Fall back to scanning `item.title` only when `open.path` does not name a work item.
 */
export function extractInboxWorkRefs(item: InboxItem): string[] {
	const pathRef = extractWorkRefFromPath(item.open?.path);
	if (pathRef) return [pathRef];
	const refs: string[] = [];
	for (const ref of extractWorkRefsFromText(item.title)) {
		if (!refs.includes(ref)) refs.push(ref);
	}
	return refs;
}

function resolveInboxProjectId(
	inbox: InboxItem,
	workByRef: ReadonlyMap<string, WorkItemSummary>
): string | undefined {
	if (inbox.project?.id) return inbox.project.id;
	for (const ref of extractInboxWorkRefs(inbox)) {
		const matched = workByRef.get(ref);
		if (matched?.project_id) return matched.project_id;
	}
	return undefined;
}

function belongsOnlyToArchivedProjects(
	projectIds: readonly string[],
	archivedProjectIds: ReadonlySet<string>
): boolean {
	return projectIds.length > 0 && projectIds.every((pid) => archivedProjectIds.has(pid));
}

function buildInboxLookup(openInbox: readonly InboxItem[]): Map<string, InboxItem> {
	const map = new Map<string, InboxItem>();
	for (const inbox of openInbox) {
		for (const ref of extractInboxWorkRefs(inbox)) {
			if (!map.has(ref)) map.set(ref, inbox);
		}
	}
	return map;
}

export function isExcludedTerminalItem(item: WorkItemSummary): boolean {
	if (item.deleted) return true;
	if (item.kind === 'request' && item.status === 'declined') return true;
	if (
		item.kind === 'task' &&
		((item.status as string) === 'cancelled' || item.resolution === 'cancelled')
	) {
		return true;
	}
	return false;
}

export function isPipelineMerged(item: WorkItemSummary): boolean {
	if (isExcludedTerminalItem(item)) return false;
	if (item.kind === 'request' && item.status === 'merged') return true;
	return isItemDone(item);
}

/**
 * True for current open work items directly awaiting owner action (matching `deriveDecisionSpotlight`):
 * scoped requests, fixed bugs, and blocked tasks assigned to the owner.
 */
export function isActionableWorkItem(item: WorkItemSummary, ownerId: string): boolean {
	if (isExcludedTerminalItem(item) || isPipelineMerged(item)) return false;
	return (
		(item.kind === 'request' && item.status === 'scoped') ||
		(item.kind === 'bug' && item.status === 'fixed') ||
		(item.kind === 'task' && item.status === 'blocked' && item.assignee_id === ownerId)
	);
}

/**
 * Classifies a work item into a pipeline stage, or returns `null` for deleted / terminal non-merged
 * items (`declined` requests, `cancelled` tasks) that must never inflate open queue counts.
 */
export function classifyAssemblyStage(
	item: WorkItemSummary,
	inboxByRef: ReadonlyMap<string, InboxItem>,
	ownerId = 'owner'
): AssemblyStage | null {
	if (isExcludedTerminalItem(item)) return null;
	if (isPipelineMerged(item)) return 'merged';

	const hasOpenInbox =
		inboxByRef.has(item.short_id.toUpperCase()) || inboxByRef.has(item.id.toUpperCase());
	if (hasOpenInbox || isActionableWorkItem(item, ownerId)) {
		return 'review';
	}

	if (
		item.status === 'in_progress' &&
		item.checklist.total > 0 &&
		item.checklist.required_left === 0 &&
		item.checklist.done === item.checklist.total
	) {
		return 'ci';
	}

	if (item.status === 'in_progress') return 'agents';

	return 'queue';
}

function firstNameOf(name: string | undefined): string | undefined {
	if (!name) return undefined;
	const first = name.trim().split(/\s+/)[0];
	return first || undefined;
}

function compareItems(a: WorkItemSummary, b: WorkItemSummary): number {
	const numDiff = (a.number ?? Number.MAX_SAFE_INTEGER) - (b.number ?? Number.MAX_SAFE_INTEGER);
	if (numDiff !== 0) return numDiff;
	return a.short_id.localeCompare(b.short_id, undefined, { numeric: true, sensitivity: 'base' });
}

function emptyStages(): Record<AssemblyStage, AssemblyChip[]> {
	return {
		queue: [],
		agents: [],
		ci: [],
		review: [],
		merged: []
	};
}

function buildLane(
	id: string,
	name: string,
	items: readonly WorkItemSummary[],
	snapshot: OrgSnapshot,
	inboxByRef: ReadonlyMap<string, InboxItem>,
	ownerId: string
): AssemblyLane {
	const stages = emptyStages();
	const sorted = [...items].sort(compareItems);

	for (const item of sorted) {
		const stage = classifyAssemblyStage(item, inboxByRef, ownerId);
		if (!stage) continue;

		const assignee = memberById(snapshot, item.assignee_id);
		const agentFirstName = firstNameOf(assignee?.name);
		const linkedInbox =
			inboxByRef.get(item.short_id.toUpperCase()) ?? inboxByRef.get(item.id.toUpperCase());

		const href =
			stage === 'review' && linkedInbox ? inboxItemHref(linkedInbox.id) : workItemHref(item);
		const tooltipParts = [`${item.short_id} · ${item.title}`];
		if (assignee) tooltipParts.push(`Assigned to ${assignee.name}`);
		if (stage === 'review' && linkedInbox) tooltipParts.push(`Inbox: ${linkedInbox.title}`);
		else if (stage === 'ci') tooltipParts.push('Checklist complete · ready for checks');
		else if (item.blocked_reason) tooltipParts.push(`Blocked: ${item.blocked_reason}`);

		stages[stage].push({
			id: item.id,
			shortId: item.short_id,
			title: item.title,
			stage,
			href,
			agentFirstName,
			inboxId: linkedInbox?.id,
			tooltip: tooltipParts.join(' — ')
		});
	}

	const totalCount =
		stages.queue.length +
		stages.agents.length +
		stages.ci.length +
		stages.review.length +
		stages.merged.length;
	const doneCount = stages.merged.length;
	const isComplete = totalCount > 0 && doneCount === totalCount;
	const pct = computeCompletionPct(doneCount, totalCount);

	let stateWord = 'ready';
	if (totalCount === 0) stateWord = 'no items';
	else if (isComplete) stateWord = 'complete';
	else if (stages.review.length > 0) stateWord = 'awaiting review';
	else if (stages.ci.length > 0) stateWord = 'ready for checks';
	else if (stages.agents.length > 0) stateWord = 'agent active';
	else if (stages.queue.length > 0) stateWord = 'queued';

	return {
		id,
		name,
		subtitle: `${doneCount}/${totalCount} done · ${stateWord}`,
		stages,
		doneCount,
		totalCount,
		pct,
		isComplete
	};
}

function buildLanesForProject(
	projectId: string,
	items: readonly WorkItemSummary[],
	areas: readonly Area[],
	snapshot: OrgSnapshot,
	inboxByRef: ReadonlyMap<string, InboxItem>,
	ownerId: string
): AssemblyLane[] {
	const activeAreas = areas
		.filter((a) => a.project_id === projectId && !a.deleted)
		.sort((a, b) => {
			if (Boolean(a.general) !== Boolean(b.general)) return a.general ? 1 : -1;
			return byName(a, b);
		});

	if (activeAreas.length > 1) {
		const byArea = new Map<string, WorkItemSummary[]>();
		for (const area of activeAreas) byArea.set(area.id, []);
		const fallbackArea = activeAreas.find((a) => a.general) ?? activeAreas[0];
		for (const item of items) {
			const bucketKey = item.area_id && byArea.has(item.area_id) ? item.area_id : fallbackArea.id;
			byArea.get(bucketKey)?.push(item);
		}
		const populated = activeAreas
			.filter((a) => (byArea.get(a.id)?.length ?? 0) > 0)
			.map((a) =>
				buildLane(
					`${projectId}:area:${a.id}`,
					a.name,
					byArea.get(a.id) ?? [],
					snapshot,
					inboxByRef,
					ownerId
				)
			);
		if (populated.length > 0) return populated;
		return [
			buildLane(
				`${projectId}:area:${activeAreas[0].id}`,
				activeAreas[0].name,
				[],
				snapshot,
				inboxByRef,
				ownerId
			)
		];
	}

	const tracks: { key: WorkItemSummary['kind']; label: string }[] = [
		{ key: 'task', label: 'Tasks' },
		{ key: 'bug', label: 'Bugs' },
		{ key: 'request', label: 'Requests' },
		{ key: 'unknown', label: 'Other' }
	];

	const lanes: AssemblyLane[] = [];
	for (const track of tracks) {
		const trackItems = items.filter((i) => i.kind === track.key);
		if (trackItems.length > 0) {
			lanes.push(
				buildLane(
					`${projectId}:track:${track.key}`,
					track.label,
					trackItems,
					snapshot,
					inboxByRef,
					ownerId
				)
			);
		}
	}

	if (lanes.length === 0) {
		lanes.push(buildLane(`${projectId}:track:task`, 'Tasks', [], snapshot, inboxByRef, ownerId));
	}

	return lanes;
}

export function computeBottleneck(
	stageCounts: Readonly<Record<AssemblyStage, number>>
): BottleneckSummary {
	const totalOpen = stageCounts.queue + stageCounts.agents + stageCounts.ci + stageCounts.review;
	if (totalOpen === 0) {
		return {
			totalOpen: 0,
			agentsPct: 0,
			ciPct: 0,
			reviewPct: 0,
			queuePct: 0,
			legend: '0% agents · 0% ready for checks · 0% waiting on you · 0% queued',
			note: 'All work items in scope are complete'
		};
	}

	const raw = [
		{ key: 'agents', value: (stageCounts.agents * 100) / totalOpen },
		{ key: 'ci', value: (stageCounts.ci * 100) / totalOpen },
		{ key: 'review', value: (stageCounts.review * 100) / totalOpen },
		{ key: 'queue', value: (stageCounts.queue * 100) / totalOpen }
	] as const;

	const floored = raw.map((r) => ({
		key: r.key,
		pct: Math.floor(r.value),
		rem: r.value - Math.floor(r.value)
	}));
	let remaining = 100 - floored.reduce((acc, r) => acc + r.pct, 0);
	const byRem = [...floored].sort((a, b) => b.rem - a.rem);
	for (const entry of byRem) {
		if (remaining <= 0) break;
		entry.pct += 1;
		remaining -= 1;
	}

	const pctMap = Object.fromEntries(floored.map((f) => [f.key, f.pct])) as Record<
		'agents' | 'ci' | 'review' | 'queue',
		number
	>;

	let note = 'Pipeline flowing smoothly';
	if (stageCounts.review > 0) {
		note = `Reviewing ${stageCounts.review} ${stageCounts.review === 1 ? 'item' : 'items'} unblocks the line`;
	} else if (stageCounts.queue > stageCounts.agents && stageCounts.queue > 0) {
		note = `${stageCounts.queue} ${stageCounts.queue === 1 ? 'item' : 'items'} queued for agent pickup`;
	} else if (stageCounts.ci > 0) {
		note = `${stageCounts.ci} ${stageCounts.ci === 1 ? 'item' : 'items'} ready for checks`;
	} else if (stageCounts.agents > 0) {
		note = `${stageCounts.agents} ${stageCounts.agents === 1 ? 'item' : 'items'} actively in progress`;
	}

	return {
		totalOpen,
		agentsPct: pctMap.agents,
		ciPct: pctMap.ci,
		reviewPct: pctMap.review,
		queuePct: pctMap.queue,
		legend: `${pctMap.agents}% agents · ${pctMap.ci}% ready for checks · ${pctMap.review}% waiting on you · ${pctMap.queue}% queued`,
		note
	};
}

export function computeThroughput7d(timestamps: readonly string[], nowMs: number): ThroughputDay[] {
	const todayStartUtc = Date.UTC(
		new Date(nowMs).getUTCFullYear(),
		new Date(nowMs).getUTCMonth(),
		new Date(nowMs).getUTCDate()
	);
	const startWindow = todayStartUtc - 6 * DAY_MS;
	const endWindow = todayStartUtc + DAY_MS;

	const counts = new Array<number>(7).fill(0);
	for (const iso of timestamps) {
		const ts = Date.parse(iso);
		if (Number.isNaN(ts) || ts < startWindow || ts >= endWindow) continue;
		const bucket = Math.floor((ts - startWindow) / DAY_MS);
		if (bucket >= 0 && bucket < 7) counts[bucket] += 1;
	}

	const maxCount = Math.max(0, ...counts);
	return counts.map((count, index) => {
		const dayUtc = new Date(startWindow + index * DAY_MS);
		const key = dayUtc.toISOString().slice(0, 10);
		const isToday = index === 6;
		const label = isToday ? 'Today' : WEEKDAYS[dayUtc.getUTCDay()];
		const heightPct = maxCount > 0 ? Math.round((count * 100) / maxCount) : 0;
		return { key, label, count, heightPct, isToday };
	});
}

function matchInboxForRow(
	row: ActivityRow,
	inboxByRef: ReadonlyMap<string, InboxItem>,
	openInbox: readonly InboxItem[]
): InboxItem | undefined {
	const pathRef = extractWorkRefFromPath(row.href?.path);
	if (pathRef) {
		const found = inboxByRef.get(pathRef);
		if (found) return found;
		if (row.href?.path) {
			return openInbox.find((i) => i.open?.path === row.href?.path);
		}
		return undefined;
	}
	if (row.href?.path) {
		const exact = openInbox.find((i) => i.open?.path === row.href?.path);
		if (exact) return exact;
	}
	for (const ref of extractWorkRefsFromText(row.text)) {
		const found = inboxByRef.get(ref);
		if (found) return found;
	}
	return undefined;
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
			.map((row): StreamEntry => {
				const matchedInbox = matchInboxForRow(row, inboxByRef, scopedOpenInbox);
				const pathRef = extractWorkRefFromPath(row.href?.path);
				const rowRefs = pathRef ? [pathRef] : extractWorkRefsFromText(row.text);
				const touchesActionableWork = rowRefs.some((ref) => actionableWorkRefs.has(ref));
				const needsYou = Boolean(matchedInbox) || touchesActionableWork;

				const tone: StreamEntry['tone'] =
					row.run?.state === 'failed'
						? 'bad'
						: needsYou
							? 'warn'
							: row.action === 'task-done' ||
								  row.action === 'merge' ||
								  row.run?.state === 'succeeded'
								? 'ok'
								: row.kind === 'conversation'
									? 'sky'
									: 'accent';

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
					tone,
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
			});
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
				tone:
					stage === 'merged'
						? 'ok'
						: stage === 'review'
							? 'warn'
							: stage === 'ci'
								? 'sky'
								: 'accent',
				href: workItemHref(item),
				needsYou,
				inboxHref: linkedInbox ? inboxItemHref(linkedInbox.id) : undefined,
				inboxLabel: linkedInbox ? 'Review in Inbox' : undefined,
				inboxTitle: linkedInbox?.title
			};
		});
}

export interface StreamDayGroup {
	key: string;
	label: string;
	entries: StreamEntry[];
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

/**
 * Groups stream entries chronologically by calendar day (`Today`, `Yesterday`, or formatted date)
 * in `zone`, sorting newest-first within and across day sections (`poc` commit `580b6715`).
 * Entries with unparseable timestamps are placed last under `Earlier`.
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
		.sort((a, b) => {
			if (a.ms !== b.ms) return b.ms - a.ms;
			return a.index - b.index;
		});

	const groups: StreamDayGroup[] = [];
	const byKey = new Map<string, StreamDayGroup>();
	for (const { entry, ms } of ordered) {
		const isValid = Number.isFinite(ms);
		const date = isValid ? new Date(ms) : undefined;
		const key = date ? dayKey(date, zone) : 'earlier';
		let group = byKey.get(key);
		if (!group) {
			const label = !date
				? 'Earlier'
				: key === todayKey
					? 'Today'
					: key === yesterdayKey
						? 'Yesterday'
						: `${weekdayShort(date, zone)} ${formatDate(date, zone, now)}`;
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

function shouldReplaceActivityRow(existing: ActivityRow, incoming: ActivityRow): boolean {
	if (existing.run && incoming.run) {
		const existingTerminal = isTerminalRunState(existing.run.state);
		const incomingTerminal = isTerminalRunState(incoming.run.state);
		if (existingTerminal && !incomingTerminal) return false;
		if (!existingTerminal && incomingTerminal) return true;

		if (existing.run.ended_at && !incoming.run.ended_at) return false;
		if (!existing.run.ended_at && incoming.run.ended_at) return true;
		if (existing.run.ended_at && incoming.run.ended_at) {
			const existingEnd = Date.parse(existing.run.ended_at);
			const incomingEnd = Date.parse(incoming.run.ended_at);
			if (!Number.isNaN(existingEnd) && !Number.isNaN(incomingEnd)) {
				if (incomingEnd < existingEnd) return false;
				if (incomingEnd > existingEnd) return true;
			}
		}
	}

	const existingAt = Date.parse(existing.at);
	const incomingAt = Date.parse(incoming.at);
	if (!Number.isNaN(existingAt) && !Number.isNaN(incomingAt)) {
		if (incomingAt < existingAt) return false;
		if (incomingAt > existingAt) return true;
	}

	return true;
}

/**
 * Merges live subscription rows into an existing feed slice:
 * - filters through canonical `matchesQuery(row, query)` (hiding noisy status/out-of-scope rows),
 * - upserts by `row.id` while preserving newer rows (never downgrading terminal runs or later timestamps),
 * - sorts newest-first and tracks evicted rows in `more` when trimming to `limit`.
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
		const evicted = rows.length - limit;
		rows = rows.slice(0, limit);
		more += evicted;
	}
	return { rows, more, changed: true };
}

/**
 * Derives stream entries specifically for the `Decisions` tab (`kind: 'decision'`), which
 * `matchesQuery` excludes from the default `kind: 'all'` feed. Never falls back to work items
 * when the decision feed is empty, and excludes archived-only project rows on `All projects`.
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
	const activeProjects = (snapshot.projects ?? []).filter((p) => !p.archived);
	const archivedProjectIds = new Set(
		(snapshot.projects ?? []).filter((p) => p.archived).map((p) => p.id)
	);
	const selectedProject =
		projectScope === 'all' ? undefined : activeProjects.find((p) => p.id === projectScope);

	const scopedDecisionRows = selectedProject
		? decisionRows.filter((row) => row.projectIds.includes(selectedProject.id))
		: decisionRows.filter(
				(row) => !belongsOnlyToArchivedProjects(row.projectIds, archivedProjectIds)
			);
	if (scopedDecisionRows.length === 0) return [];

	const nonDeletedWork = workItems.filter((i) => !i.deleted);
	const workByRef = new Map<string, WorkItemSummary>();
	for (const item of nonDeletedWork) {
		workByRef.set(item.short_id.toUpperCase(), item);
		workByRef.set(item.id.toUpperCase(), item);
	}

	const pipelineWork = workItems.filter((i) => !isExcludedTerminalItem(i));
	const allOpenInbox = inboxPhase === 'ready' ? inboxItems.filter((i) => i.state === 'open') : [];
	const scopedOpenInbox = selectedProject
		? allOpenInbox.filter((i) => resolveInboxProjectId(i, workByRef) === selectedProject.id)
		: allOpenInbox.filter((i) => {
				const pid = resolveInboxProjectId(i, workByRef);
				return !pid || !archivedProjectIds.has(pid);
			});
	const inboxByRef = buildInboxLookup(scopedOpenInbox);

	const actionableWorkRefs = new Set<string>();
	for (const item of pipelineWork) {
		if (!isActionableWorkItem(item, resolvedOwnerId)) continue;
		if (selectedProject && item.project_id !== selectedProject.id) continue;
		if (!selectedProject && item.project_id && archivedProjectIds.has(item.project_id)) continue;
		actionableWorkRefs.add(item.short_id.toUpperCase());
		actionableWorkRefs.add(item.id.toUpperCase());
	}

	return deriveStreamEntries(
		snapshot,
		scopedDecisionRows,
		[],
		scopedOpenInbox,
		inboxByRef,
		actionableWorkRefs,
		resolvedOwnerId,
		decisionPhase,
		workPhase
	);
}

export function deriveActivityCommandCenter(input: CommandCenterInput): ActivityCommandCenterModel {
	const nowMs = input.nowMs ?? Date.now();
	const feedPhase: FeedPhase = input.feedPhase ?? 'ready';
	const feedMore = input.feedMore ?? 0;
	const workPhase: FeedPhase = input.workPhase ?? 'ready';
	const inboxPhase: FeedPhase = input.inboxPhase ?? 'ready';
	const { snapshot, organizationName, workItems, inboxItems, activityRows } = input;
	const resolvedOwnerId = input.ownerId ?? owner(snapshot)?.id ?? 'owner';

	const activeProjects = [...(snapshot.projects ?? [])]
		.filter((p) => !p.archived)
		.sort((a, b) => byName(a, b) || a.id.localeCompare(b.id));
	const archivedProjectIds = new Set(
		(snapshot.projects ?? []).filter((p) => p.archived).map((p) => p.id)
	);
	const activeProjectIds = new Set(activeProjects.map((p) => p.id));

	const selectedProject =
		input.projectScope === 'all'
			? undefined
			: activeProjects.find((p) => p.id === input.projectScope);

	const nonDeletedWork = workItems.filter((i) => !i.deleted);
	const workByRef = new Map<string, WorkItemSummary>();
	for (const item of nonDeletedWork) {
		workByRef.set(item.short_id.toUpperCase(), item);
		workByRef.set(item.id.toUpperCase(), item);
	}

	const pipelineWork = workItems.filter((i) => !isExcludedTerminalItem(i));
	const scopedWork = selectedProject
		? pipelineWork.filter((i) => i.project_id === selectedProject.id)
		: pipelineWork.filter((i) => !i.project_id || !archivedProjectIds.has(i.project_id));

	const allOpenInbox = inboxPhase === 'ready' ? inboxItems.filter((i) => i.state === 'open') : [];
	const scopedOpenInbox = selectedProject
		? allOpenInbox.filter((i) => resolveInboxProjectId(i, workByRef) === selectedProject.id)
		: allOpenInbox.filter((i) => {
				const pid = resolveInboxProjectId(i, workByRef);
				return !pid || !archivedProjectIds.has(pid);
			});

	const inboxByRef = buildInboxLookup(scopedOpenInbox);

	// Scoped activity rows: exclude archived-only project rows on All projects, retain org-wide rows.
	const scopedActivityRows = selectedProject
		? activityRows.filter((row) => row.projectIds.includes(selectedProject.id))
		: activityRows.filter(
				(row) => !belongsOnlyToArchivedProjects(row.projectIds, archivedProjectIds)
			);

	const areas = snapshot.areas ?? [];
	const milestones: AssemblyMilestone[] = [];

	const projectsToRender = selectedProject ? [selectedProject] : activeProjects;
	for (const project of projectsToRender) {
		const projItems = scopedWork.filter((i) => i.project_id === project.id);
		const lanes = buildLanesForProject(
			project.id,
			projItems,
			areas,
			snapshot,
			inboxByRef,
			resolvedOwnerId
		);
		const doneCount = lanes.reduce((sum, l) => sum + l.doneCount, 0);
		const totalCount = lanes.reduce((sum, l) => sum + l.totalCount, 0);
		const isComplete = totalCount > 0 && doneCount === totalCount;
		const pct = computeCompletionPct(doneCount, totalCount);
		const completeTracks = lanes.filter((l) => l.isComplete).length;
		const trackWord = lanes.length === 1 ? 'track' : 'tracks';
		milestones.push({
			id: project.id,
			name: project.name,
			key: project.key,
			summary: `${pct}% · ${doneCount}/${totalCount} tasks · ${completeTracks}/${lanes.length} ${trackWord}${isComplete ? ' · Complete' : ''}`,
			doneCount,
			totalCount,
			pct,
			isComplete,
			lanes
		});
	}

	if (!selectedProject) {
		const unassignedItems = scopedWork.filter(
			(i) => !i.project_id || !activeProjectIds.has(i.project_id)
		);
		if (unassignedItems.length > 0) {
			const lanes = buildLanesForProject(
				'__unassigned__',
				unassignedItems,
				[],
				snapshot,
				inboxByRef,
				resolvedOwnerId
			);
			const doneCount = lanes.reduce((sum, l) => sum + l.doneCount, 0);
			const totalCount = lanes.reduce((sum, l) => sum + l.totalCount, 0);
			const isComplete = totalCount > 0 && doneCount === totalCount;
			const pct = computeCompletionPct(doneCount, totalCount);
			const completeTracks = lanes.filter((l) => l.isComplete).length;
			const trackWord = lanes.length === 1 ? 'track' : 'tracks';
			milestones.push({
				id: '__unassigned__',
				name: 'Organization',
				summary: `${pct}% · ${doneCount}/${totalCount} tasks · ${completeTracks}/${lanes.length} ${trackWord}${isComplete ? ' · Complete' : ''}`,
				doneCount,
				totalCount,
				pct,
				isComplete,
				lanes
			});
		}
	}

	const stageCounts: Record<AssemblyStage, number> = {
		queue: 0,
		agents: 0,
		ci: 0,
		review: 0,
		merged: 0
	};
	const actionableWorkRefs = new Set<string>();
	const busyAssigneeIds = new Set<string>();
	const linkedInboxIds = new Set<string>();
	let actionableWorkCount = 0;

	for (const item of scopedWork) {
		const stage = classifyAssemblyStage(item, inboxByRef, resolvedOwnerId);
		if (!stage) continue;
		stageCounts[stage] += 1;
		if (isActionableWorkItem(item, resolvedOwnerId)) {
			actionableWorkCount += 1;
			actionableWorkRefs.add(item.short_id.toUpperCase());
			actionableWorkRefs.add(item.id.toUpperCase());
		}
		if (stage === 'review') {
			const linked =
				inboxByRef.get(item.short_id.toUpperCase()) ?? inboxByRef.get(item.id.toUpperCase());
			if (linked) linkedInboxIds.add(linked.id);
		}
		if ((stage === 'agents' || stage === 'ci') && item.assignee_id) {
			busyAssigneeIds.add(item.assignee_id);
		}
	}

	const totalItems =
		stageCounts.queue +
		stageCounts.agents +
		stageCounts.ci +
		stageCounts.review +
		stageCounts.merged;
	const mergedItems = stageCounts.merged;
	const openItems = totalItems - mergedItems;
	const isComplete = totalItems > 0 && mergedItems === totalItems;
	const readinessPct = computeCompletionPct(mergedItems, totalItems);

	const unlinkedOpenInboxCount = scopedOpenInbox.filter((i) => !linkedInboxIds.has(i.id)).length;
	const needsYouCount = stageCounts.review + unlinkedOpenInboxCount;

	// Agents roster for KPI & rail
	const nonFormerAgents = agentsOf(nonOwners(snapshot)).filter((m) => !isFormer(m));
	const activeAgents = nonFormerAgents.filter((m) => m.agent?.status === 'active');
	const activeAgentsCount = activeAgents.length;
	const busyAgentsCount = activeAgents.filter((a) => busyAssigneeIds.has(a.id)).length;

	const workByAssignee = new Map<string, WorkItemSummary[]>();
	for (const item of scopedWork) {
		if (!item.assignee_id || isPipelineMerged(item)) continue;
		const list = workByAssignee.get(item.assignee_id) ?? [];
		list.push(item);
		workByAssignee.set(item.assignee_id, list);
	}

	const stagePriority: Record<AssemblyStage, number> = {
		agents: 0,
		ci: 1,
		review: 2,
		queue: 3,
		merged: 4
	};

	const allRailAgents: RailAgentRow[] = nonFormerAgents.map((member) => {
		const assigned = [...(workByAssignee.get(member.id) ?? [])].sort((a, b) => {
			const sa = classifyAssemblyStage(a, inboxByRef, resolvedOwnerId) ?? 'merged';
			const sb = classifyAssemblyStage(b, inboxByRef, resolvedOwnerId) ?? 'merged';
			return stagePriority[sa] - stagePriority[sb] || compareItems(a, b);
		});
		const leadItem = assigned[0];
		const leadStage = leadItem
			? classifyAssemblyStage(leadItem, inboxByRef, resolvedOwnerId)
			: null;

		let status: RailAgentRow['status'] = 'idle';
		let statusLabel = 'Idle';
		if (member.agent?.status === 'paused') {
			status = 'paused';
			statusLabel = 'Paused';
		} else if (leadStage === 'agents' || leadStage === 'ci') {
			status = 'building';
			statusLabel = leadStage === 'ci' ? 'Checks ready' : 'Building';
		} else if (leadStage === 'review') {
			status = 'waiting';
			statusLabel = 'Needs input';
		}

		return {
			id: member.id,
			name: member.name,
			initials: initialsOf(member.name),
			title: member.agent?.title || member.agent?.role || 'Agent',
			status,
			statusLabel,
			currentWork: leadItem
				? {
						shortId: leadItem.short_id,
						title: leadItem.title,
						href: workItemHref(leadItem)
					}
				: undefined,
			href: `/members/${encodeURIComponent(member.id)}`
		};
	});

	const agentStatusRank: Record<RailAgentRow['status'], number> = {
		building: 0,
		waiting: 1,
		paused: 2,
		idle: 3
	};
	allRailAgents.sort(
		(a, b) => agentStatusRank[a.status] - agentStatusRank[b.status] || byName(a, b)
	);
	const railAgents = allRailAgents.slice(0, 6);
	const extraAgentsCount = Math.max(0, allRailAgents.length - railAgents.length);

	const streamEntries = deriveStreamEntries(
		snapshot,
		scopedActivityRows,
		scopedWork,
		scopedOpenInbox,
		inboxByRef,
		actionableWorkRefs,
		resolvedOwnerId,
		feedPhase,
		workPhase
	);
	const streamNeedsYouCount = streamEntries.filter((e) => e.needsYou).length;

	// Honest 7-day throughput derivation (source-specific labels and unavailable messaging)
	let throughputSource: ThroughputSource;
	let throughputTimestamps: string[] = [];
	if (feedPhase === 'loading') {
		throughputSource = 'loading';
	} else if (feedPhase === 'error') {
		throughputSource = 'unavailable';
	} else if (scopedActivityRows.length > 0) {
		throughputSource = feedMore > 0 ? 'events_capped' : 'events';
		throughputTimestamps = scopedActivityRows.map((r) => r.at);
	} else if (workPhase === 'ready') {
		throughputSource = 'work_updates';
		throughputTimestamps = scopedWork.map((i) => i.updated_at);
	} else if (workPhase === 'loading') {
		throughputSource = 'loading';
	} else {
		throughputSource = 'unavailable';
	}

	const throughput7d = computeThroughput7d(throughputTimestamps, nowMs);
	const total7dCount = throughput7d.reduce((sum, d) => sum + d.count, 0);

	let throughputMeta = `${total7dCount} ${total7dCount === 1 ? 'event' : 'events'}`;
	let throughputAriaLabel = '7-day activity events';
	let throughputUnavailableMessage: string | undefined;

	if (throughputSource === 'loading') {
		throughputMeta = 'Loading…';
	} else if (throughputSource === 'unavailable') {
		if (feedPhase === 'error') {
			throughputMeta = 'Feed unavailable';
			throughputUnavailableMessage = 'Activity feed is unavailable for 7-day throughput.';
		} else {
			throughputMeta = 'Work data unavailable';
			throughputUnavailableMessage = 'Work data is unavailable for 7-day throughput.';
		}
	} else if (throughputSource === 'events_capped') {
		throughputMeta = 'Recent events (capped)';
		throughputAriaLabel = 'Recent 7-day activity events (capped)';
	} else if (throughputSource === 'work_updates') {
		throughputMeta = 'Items last updated (7d)';
		throughputAriaLabel = '7-day items last updated';
	}

	// Release gates: strict approval filtering + honest Inbox availability state
	const openApprovals = scopedOpenInbox.filter((i) => i.kind === 'approval');
	const openDecisions = scopedOpenInbox.filter((i) => i.kind === 'decision');
	const criticalOpenBugs = scopedWork.filter(
		(i) => i.kind === 'bug' && i.status !== 'closed' && i.severity === 'critical'
	).length;
	const blockedTasks = scopedWork.filter((i) => i.kind === 'task' && i.status === 'blocked').length;

	const approvalGate: ReleaseGateRow =
		inboxPhase === 'loading'
			? {
					id: 'approvals',
					label: 'Human approvals',
					statusLabel: 'Loading…',
					tone: 'neutral',
					count: 0,
					href: '/inbox'
				}
			: inboxPhase === 'error'
				? {
						id: 'approvals',
						label: 'Human approvals',
						statusLabel: 'Unavailable',
						tone: 'warn',
						count: 0,
						href: '/inbox'
					}
				: {
						id: 'approvals',
						label: 'Human approvals',
						statusLabel: openApprovals.length === 0 ? 'Clear' : `${openApprovals.length} pending`,
						tone: openApprovals.length === 0 ? 'ok' : 'warn',
						count: openApprovals.length,
						href: '/inbox'
					};

	const releaseGates: ReleaseGateRow[] = [
		approvalGate,
		...(inboxPhase === 'ready' && openDecisions.length > 0
			? [
					{
						id: 'decisions',
						label: 'Open decisions',
						statusLabel: `${openDecisions.length} pending`,
						tone: 'warn' as const,
						count: openDecisions.length,
						href: '/inbox'
					}
				]
			: []),
		{
			id: 'critical-bugs',
			label: 'Critical bugs',
			statusLabel: criticalOpenBugs === 0 ? 'Clear' : `${criticalOpenBugs} open`,
			tone: criticalOpenBugs === 0 ? 'ok' : 'bad',
			count: criticalOpenBugs,
			href: '/work'
		},
		{
			id: 'blocked-tasks',
			label: 'Blocked tasks',
			statusLabel: blockedTasks === 0 ? 'None' : `${blockedTasks} blocked`,
			tone: blockedTasks === 0 ? 'ok' : 'warn',
			count: blockedTasks,
			href: '/work'
		}
	];

	const pendingApprovals =
		inboxPhase === 'ready'
			? openApprovals.slice(0, 3).map((item) => ({
					id: item.id,
					title: item.title,
					href: inboxItemHref(item.id)
				}))
			: [];

	const bottleneck = computeBottleneck(stageCounts);

	const ciColumnSubLabel = `${stageCounts.ci} ready for checks`;
	const ciStationLabel = ciColumnSubLabel;

	const scopeEyebrow = selectedProject
		? `${selectedProject.key ? `${selectedProject.key.toUpperCase()} · ` : ''}PRODUCTION LINE`
		: 'LIVE PIPELINE';
	const scopeTitle = selectedProject ? selectedProject.name : 'All projects';
	const scopeSubtitle = selectedProject
		? `${selectedProject.name} · ${openItems} open, ${mergedItems} done`
		: `Assembly line and live stream across ${activeProjects.length} active ${activeProjects.length === 1 ? 'project' : 'projects'} in ${organizationName}`;

	let liveStatusText = 'Ready for work';
	if (isComplete) {
		liveStatusText = 'All items complete';
	} else if (stageCounts.review > 0) {
		liveStatusText = `${stageCounts.review} awaiting human review`;
	} else if (stageCounts.agents > 0 || stageCounts.ci > 0) {
		liveStatusText = `Assembling · ${stageCounts.agents + stageCounts.ci} active`;
	} else if (stageCounts.queue > 0) {
		liveStatusText = `${stageCounts.queue} queued`;
	}

	const stationsNote =
		inboxPhase === 'error'
			? 'Inbox unavailable; approval wait counts may be incomplete'
			: inboxPhase === 'loading'
				? 'Loading Inbox review items…'
				: needsYouCount > 0
					? `${needsYouCount} ${needsYouCount === 1 ? 'item' : 'items'} waiting on human review`
					: 'No human review bottlenecks right now';

	return {
		activeProjects,
		selectedProject,
		scopeTitle,
		scopeEyebrow,
		scopeSubtitle,
		liveStatusText,
		stageCounts,
		ciColumnSubLabel,
		ciStationLabel,
		totalItems,
		openItems,
		mergedItems,
		readinessPct,
		isComplete,
		busyAgentsCount,
		activeAgentsCount,
		needsYouCount,
		openInboxCount: scopedOpenInbox.length,
		actionableWorkCount,
		milestones,
		bottleneck,
		releaseGates,
		pendingApprovals,
		stationsNote,
		throughput7d,
		throughputSource,
		throughputMeta,
		throughputAriaLabel,
		throughputUnavailableMessage,
		railAgents,
		extraAgentsCount,
		streamEntries,
		streamNeedsYouCount
	};
}
