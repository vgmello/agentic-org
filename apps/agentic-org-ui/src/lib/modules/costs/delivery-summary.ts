import type { InboxItem } from '$lib/modules/inbox/types';
import { byName, owner } from '$lib/modules/organization/model';
import type { Area, OrgSnapshot, WorkItemSummary } from '$lib/proto';

export type DeliveryStage = 'queue' | 'agents' | 'ci' | 'review' | 'merged';

export interface ProjectDeliveryTrack {
	id: string;
	name: string;
	doneCount: number;
	totalCount: number;
	pct: number;
	isComplete: boolean;
}

export interface ProjectDeliverySummary {
	stageCounts: Record<DeliveryStage, number>;
	totalItems: number;
	tracks: ProjectDeliveryTrack[];
}

function isExcludedTerminalItem(item: WorkItemSummary): boolean {
	if (item.deleted) return true;
	if (item.kind === 'request' && item.status === 'declined') return true;
	return (
		item.kind === 'task' &&
		((item.status as string) === 'cancelled' || item.resolution === 'cancelled')
	);
}

function isMergedItem(item: WorkItemSummary): boolean {
	if (isExcludedTerminalItem(item)) return false;
	if (item.kind === 'request') return item.status === 'merged' || item.status === 'approved';
	if (item.kind === 'task') return item.status === 'done';
	return item.kind === 'bug' && item.status === 'closed';
}

function isActionable(item: WorkItemSummary, ownerId: string): boolean {
	if (isExcludedTerminalItem(item) || isMergedItem(item)) return false;
	if (item.kind === 'request') return item.status === 'scoped';
	if (item.kind === 'bug') return item.status === 'fixed';
	return item.kind === 'task' && item.status === 'blocked' && item.assignee_id === ownerId;
}

function decodeRefToken(raw: string): string {
	try {
		return decodeURIComponent(raw).toUpperCase();
	} catch {
		return raw.toUpperCase();
	}
}

function extractInboxWorkRefs(item: InboxItem): string[] {
	const path = item.open?.path;
	if (path) {
		const workMatch = path.match(/\/work\/([^/?#]+)/);
		if (workMatch) return [decodeRefToken(workMatch[1])];
		const reqMatch = path.match(/[?&]request=([^&#]+)/);
		if (reqMatch) return [decodeRefToken(reqMatch[1])];
	}
	const matches = item.title.match(/\b[A-Z]+-[A-Z]{0,3}\d+\b/g);
	return matches ? [...new Set(matches.map((m) => m.toUpperCase()))] : [];
}

function buildWorkByRef(workItems: readonly WorkItemSummary[]): Map<string, WorkItemSummary> {
	const map = new Map<string, WorkItemSummary>();
	for (const item of workItems) {
		if (item.deleted) continue;
		map.set(item.short_id.toUpperCase(), item);
		map.set(item.id.toUpperCase(), item);
	}
	return map;
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

function buildReviewInboxSet(
	inboxItems: readonly InboxItem[],
	projectId: string,
	workByRef: ReadonlyMap<string, WorkItemSummary>
): Set<string> {
	const refs = new Set<string>();
	for (const inbox of inboxItems) {
		if (inbox.state !== 'open' || inbox.kind === 'problem') continue;
		if (resolveInboxProjectId(inbox, workByRef) !== projectId) continue;
		for (const ref of extractInboxWorkRefs(inbox)) refs.add(ref);
	}
	return refs;
}

function isChecklistReadyForChecks(item: WorkItemSummary): boolean {
	return (
		item.status === 'in_progress' &&
		item.checklist.total > 0 &&
		item.checklist.required_left === 0 &&
		item.checklist.done === item.checklist.total
	);
}

function classifyStage(
	item: WorkItemSummary,
	reviewRefs: ReadonlySet<string>,
	ownerId: string
): DeliveryStage | null {
	if (isExcludedTerminalItem(item)) return null;
	if (isMergedItem(item)) return 'merged';
	const hasInboxGate =
		reviewRefs.has(item.short_id.toUpperCase()) || reviewRefs.has(item.id.toUpperCase());
	if (hasInboxGate || isActionable(item, ownerId)) return 'review';
	if (isChecklistReadyForChecks(item)) return 'ci';
	return item.status === 'in_progress' ? 'agents' : 'queue';
}

function completionPct(done: number, total: number): number {
	if (total <= 0 || done <= 0) return 0;
	if (done === total) return 100;
	return Math.min(99, Math.round((Math.min(done, total) * 100) / total));
}

function summarizeTrack(
	id: string,
	name: string,
	items: readonly WorkItemSummary[],
	reviewRefs: ReadonlySet<string>,
	ownerId: string,
	stageCounts: Record<DeliveryStage, number>
): ProjectDeliveryTrack {
	let doneCount = 0;
	let totalCount = 0;
	for (const item of items) {
		const stage = classifyStage(item, reviewRefs, ownerId);
		if (!stage) continue;
		stageCounts[stage] += 1;
		totalCount += 1;
		if (stage === 'merged') doneCount += 1;
	}
	return {
		id,
		name,
		doneCount,
		totalCount,
		pct: completionPct(doneCount, totalCount),
		isComplete: totalCount > 0 && doneCount === totalCount
	};
}

function groupByArea(
	projectId: string,
	items: readonly WorkItemSummary[],
	activeAreas: readonly Area[],
	reviewRefs: ReadonlySet<string>,
	ownerId: string,
	stageCounts: Record<DeliveryStage, number>
): ProjectDeliveryTrack[] {
	const byArea = new Map<string, WorkItemSummary[]>(activeAreas.map((a) => [a.id, []]));
	const fallback = activeAreas.find((a) => a.general) ?? activeAreas[0];
	for (const item of items) {
		const key = item.area_id && byArea.has(item.area_id) ? item.area_id : fallback.id;
		byArea.get(key)?.push(item);
	}
	const populated = activeAreas
		.filter((a) => (byArea.get(a.id)?.length ?? 0) > 0)
		.map((a) =>
			summarizeTrack(
				`${projectId}:area:${a.id}`,
				a.name,
				byArea.get(a.id) ?? [],
				reviewRefs,
				ownerId,
				stageCounts
			)
		);
	if (populated.length > 0) return populated;
	return [
		summarizeTrack(
			`${projectId}:area:${activeAreas[0].id}`,
			activeAreas[0].name,
			[],
			reviewRefs,
			ownerId,
			stageCounts
		)
	];
}

function groupByWorkKind(
	projectId: string,
	items: readonly WorkItemSummary[],
	reviewRefs: ReadonlySet<string>,
	ownerId: string,
	stageCounts: Record<DeliveryStage, number>
): ProjectDeliveryTrack[] {
	const kinds: { key: WorkItemSummary['kind']; label: string }[] = [
		{ key: 'task', label: 'Tasks' },
		{ key: 'bug', label: 'Bugs' },
		{ key: 'request', label: 'Requests' },
		{ key: 'unknown', label: 'Other' }
	];
	const tracks: ProjectDeliveryTrack[] = [];
	for (const k of kinds) {
		const bucket = items.filter((i) => i.kind === k.key);
		if (bucket.length > 0) {
			tracks.push(
				summarizeTrack(
					`${projectId}:track:${k.key}`,
					k.label,
					bucket,
					reviewRefs,
					ownerId,
					stageCounts
				)
			);
		}
	}
	if (tracks.length === 0) {
		tracks.push(
			summarizeTrack(`${projectId}:track:task`, 'Tasks', [], reviewRefs, ownerId, stageCounts)
		);
	}
	return tracks;
}

export function deriveProjectDeliverySummary(input: {
	snapshot: OrgSnapshot;
	projectId: string;
	workItems: readonly WorkItemSummary[];
	inboxItems: readonly InboxItem[];
	ownerId?: string;
}): ProjectDeliverySummary {
	const ownerId = input.ownerId ?? owner(input.snapshot)?.id ?? 'owner';
	const stageCounts: Record<DeliveryStage, number> = {
		queue: 0,
		agents: 0,
		ci: 0,
		review: 0,
		merged: 0
	};
	const projectItems = input.workItems.filter(
		(item) => !isExcludedTerminalItem(item) && item.project_id === input.projectId
	);
	const workByRef = buildWorkByRef(input.workItems);
	const reviewRefs = buildReviewInboxSet(input.inboxItems, input.projectId, workByRef);
	const activeAreas = (input.snapshot.areas ?? [])
		.filter((a) => a.project_id === input.projectId && !a.deleted)
		.sort((a, b) => (Boolean(a.general) !== Boolean(b.general) ? (a.general ? 1 : -1) : byName(a, b)));

	const tracks =
		activeAreas.length > 1
			? groupByArea(input.projectId, projectItems, activeAreas, reviewRefs, ownerId, stageCounts)
			: groupByWorkKind(input.projectId, projectItems, reviewRefs, ownerId, stageCounts);

	const totalItems =
		stageCounts.queue + stageCounts.agents + stageCounts.ci + stageCounts.review + stageCounts.merged;
	return { stageCounts, totalItems, tracks };
}
