import { isItemDone } from '$lib/format';
import type { InboxItem } from '$lib/modules/inbox/types';
import { byName, memberById } from '$lib/modules/organization/model';
import type { Area, OrgSnapshot, ProjectSummary, WorkItemSummary } from '$lib/proto';
import type {
	AssemblyChip,
	AssemblyLane,
	AssemblyMilestone,
	AssemblyStage
} from './assembly-types';

const TELEMETRY_HOURS = new Intl.NumberFormat('en-US', {
	minimumFractionDigits: 1,
	maximumFractionDigits: 1
});

/** Formats a telemetry duration in hours (e.g. `14.5 h`), or `—` when null/unavailable. */
export function formatTelemetryHours(hours: number | null | undefined): string {
	if (hours === null || hours === undefined || !Number.isFinite(hours) || hours < 0) return '—';
	return `${TELEMETRY_HOURS.format(hours)} h`;
}

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
	// Match both plain (`T-14`, `BUG-44`) and office-prefixed (`T-A40`, `T-B37`, `BUG-A12`) references.
	const matches = text.match(/\b[A-Z]+-[A-Z]{0,3}\d+\b/g);
	return matches ? matches.map((m) => m.toUpperCase()) : [];
}

function safeDecodeUpper(raw: string): string {
	try {
		return decodeURIComponent(raw).toUpperCase();
	} catch {
		return raw.toUpperCase();
	}
}

export function extractWorkRefFromPath(path: string | undefined): string | undefined {
	if (!path) return undefined;
	const workMatch = path.match(/\/work\/([^/?#]+)/);
	if (workMatch) return safeDecodeUpper(workMatch[1]);
	const reqMatch = path.match(/[?&]request=([^&#]+)/);
	if (reqMatch) return safeDecodeUpper(reqMatch[1]);
	return undefined;
}

/**
 * Extracts work item references for an Inbox item.
 * When `item.open?.path` explicitly identifies a work subject (`/work/<ref>` or `?request=<id>`),
 * return ONLY that explicit subject ref so mentioned tickets in the title are not misattributed.
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

export function resolveInboxProjectId(
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

export function belongsOnlyToArchivedProjects(
	projectIds: readonly string[],
	archivedProjectIds: ReadonlySet<string>
): boolean {
	return projectIds.length > 0 && projectIds.every((pid) => archivedProjectIds.has(pid));
}

/**
 * Lookup of open Inbox items that represent a genuine human review/decision/question gate
 * on a work item (never pulling queued tasks into review from runtime `problem`/crash notices).
 */
export function buildReviewInboxLookup(openInbox: readonly InboxItem[]): Map<string, InboxItem> {
	const map = new Map<string, InboxItem>();
	for (const inbox of openInbox) {
		if (inbox.kind === 'problem') continue;
		for (const ref of extractInboxWorkRefs(inbox)) {
			if (!map.has(ref)) map.set(ref, inbox);
		}
	}
	return map;
}

export function isExcludedTerminalItem(item: WorkItemSummary): boolean {
	if (item.deleted) return true;
	if (item.kind === 'request' && item.status === 'declined') return true;
	return (
		item.kind === 'task' &&
		((item.status as string) === 'cancelled' || item.resolution === 'cancelled')
	);
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
	if (hasOpenInbox || isActionableWorkItem(item, ownerId)) return 'review';

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

export function compareItems(a: WorkItemSummary, b: WorkItemSummary): number {
	const numDiff = (a.number ?? Number.MAX_SAFE_INTEGER) - (b.number ?? Number.MAX_SAFE_INTEGER);
	if (numDiff !== 0) return numDiff;
	return a.short_id.localeCompare(b.short_id, undefined, { numeric: true, sensitivity: 'base' });
}

function emptyStages(): Record<AssemblyStage, AssemblyChip[]> {
	return { queue: [], agents: [], ci: [], review: [], merged: [] };
}

function buildChipTooltip(
	item: WorkItemSummary,
	stage: AssemblyStage,
	assigneeName: string | undefined,
	linkedInbox: InboxItem | undefined
): string {
	const parts = [`${item.short_id} · ${item.title}`];
	if (assigneeName) parts.push(`Assigned to ${assigneeName}`);
	if (stage === 'review' && linkedInbox) parts.push(`Inbox: ${linkedInbox.title}`);
	else if (stage === 'ci') parts.push('Checklist complete · ready for checks');
	else if (item.blocked_reason) parts.push(`Blocked: ${item.blocked_reason}`);
	return parts.join(' — ');
}

function laneStateWord(
	stages: Record<AssemblyStage, AssemblyChip[]>,
	totalCount: number,
	isComplete: boolean
): string {
	if (totalCount === 0) return 'no items';
	if (isComplete) return 'complete';
	if (stages.review.length > 0) return 'awaiting review';
	if (stages.ci.length > 0) return 'ready for checks';
	if (stages.agents.length > 0) return 'agent active';
	if (stages.queue.length > 0) return 'queued';
	return 'ready';
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
	for (const item of [...items].sort(compareItems)) {
		const stage = classifyAssemblyStage(item, inboxByRef, ownerId);
		if (!stage) continue;

		const assignee = memberById(snapshot, item.assignee_id);
		const linkedInbox =
			inboxByRef.get(item.short_id.toUpperCase()) ?? inboxByRef.get(item.id.toUpperCase());
		const href =
			stage === 'review' && linkedInbox ? inboxItemHref(linkedInbox.id) : workItemHref(item);

		stages[stage].push({
			id: item.id,
			shortId: item.short_id,
			title: item.title,
			stage,
			href,
			agentFirstName: firstNameOf(assignee?.name),
			inboxId: linkedInbox?.id,
			tooltip: buildChipTooltip(item, stage, assignee?.name, linkedInbox)
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
	const stateWord = laneStateWord(stages, totalCount, isComplete);

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

function buildAreaLanes(
	projectId: string,
	items: readonly WorkItemSummary[],
	activeAreas: readonly Area[],
	snapshot: OrgSnapshot,
	inboxByRef: ReadonlyMap<string, InboxItem>,
	ownerId: string
): AssemblyLane[] {
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

export function buildLanesForProject(
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
		return buildAreaLanes(projectId, items, activeAreas, snapshot, inboxByRef, ownerId);
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

function summarizeMilestone(
	id: string,
	name: string,
	key: string | undefined,
	lanes: AssemblyLane[]
): AssemblyMilestone {
	const doneCount = lanes.reduce((sum, l) => sum + l.doneCount, 0);
	const totalCount = lanes.reduce((sum, l) => sum + l.totalCount, 0);
	const isComplete = totalCount > 0 && doneCount === totalCount;
	const pct = computeCompletionPct(doneCount, totalCount);
	const completeTracks = lanes.filter((l) => l.isComplete).length;
	const trackWord = lanes.length === 1 ? 'track' : 'tracks';
	return {
		id,
		name,
		key,
		summary: `${pct}% · ${doneCount}/${totalCount} tasks · ${completeTracks}/${lanes.length} ${trackWord}${isComplete ? ' · Complete' : ''}`,
		doneCount,
		totalCount,
		pct,
		isComplete,
		lanes
	};
}

export function buildMilestones(input: {
	projectsToRender: readonly ProjectSummary[];
	scopedProjectId: string | undefined;
	activeProjectIds: ReadonlySet<string>;
	scopedWork: readonly WorkItemSummary[];
	areas: readonly Area[];
	snapshot: OrgSnapshot;
	reviewInboxByRef: ReadonlyMap<string, InboxItem>;
	ownerId: string;
}): AssemblyMilestone[] {
	const milestones: AssemblyMilestone[] = [];
	for (const project of input.projectsToRender) {
		const projItems = input.scopedWork.filter((i) => i.project_id === project.id);
		const lanes = buildLanesForProject(
			project.id,
			projItems,
			input.areas,
			input.snapshot,
			input.reviewInboxByRef,
			input.ownerId
		);
		milestones.push(summarizeMilestone(project.id, project.name, project.key, lanes));
	}

	if (!input.scopedProjectId) {
		const unassignedItems = input.scopedWork.filter(
			(i) => !i.project_id || !input.activeProjectIds.has(i.project_id)
		);
		if (unassignedItems.length > 0) {
			const lanes = buildLanesForProject(
				'__unassigned__',
				unassignedItems,
				[],
				input.snapshot,
				input.reviewInboxByRef,
				input.ownerId
			);
			milestones.push(summarizeMilestone('__unassigned__', 'Organization', undefined, lanes));
		}
	}

	return milestones;
}
