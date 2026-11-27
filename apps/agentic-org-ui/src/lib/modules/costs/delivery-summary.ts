import type { InboxItem } from '$lib/modules/inbox/types';
import { byName } from '$lib/modules/organization/model';
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
	if (item.kind === 'request' && (item.status === 'merged' || item.status === 'approved')) {
		return true;
	}
	return (
		(item.kind === 'task' && item.status === 'done') ||
		(item.kind === 'bug' && item.status === 'closed')
	);
}

function isActionable(item: WorkItemSummary, ownerId: string): boolean {
	if (isExcludedTerminalItem(item) || isMergedItem(item)) return false;
	return (
		(item.kind === 'request' && item.status === 'scoped') ||
		(item.kind === 'bug' && item.status === 'fixed') ||
		(item.kind === 'task' && item.status === 'blocked' && item.assignee_id === ownerId)
	);
}

function extractInboxWorkRefs(item: InboxItem): string[] {
	const path = item.open?.path;
	if (path) {
		const workMatch = path.match(/\/work\/([^/?#]+)/);
		if (workMatch) {
			try {
				return [decodeURIComponent(workMatch[1]).toUpperCase()];
			} catch {
				return [workMatch[1].toUpperCase()];
			}
		}
		const reqMatch = path.match(/[?&]request=([^&#]+)/);
		if (reqMatch) {
			try {
				return [decodeURIComponent(reqMatch[1]).toUpperCase()];
			} catch {
				return [reqMatch[1].toUpperCase()];
			}
		}
	}
	const matches = item.title.match(/\b[A-Z]+-[A-Z]{0,3}\d+\b/g);
	return matches ? [...new Set(matches.map((m) => m.toUpperCase()))] : [];
}

function buildReviewInboxSet(inboxItems: readonly InboxItem[]): Set<string> {
	const refs = new Set<string>();
	for (const inbox of inboxItems) {
		if (inbox.state !== 'open' || inbox.kind === 'problem') continue;
		for (const ref of extractInboxWorkRefs(inbox)) refs.add(ref);
	}
	return refs;
}

function classifyStage(
	item: WorkItemSummary,
	reviewRefs: ReadonlySet<string>,
	ownerId: string
): DeliveryStage | null {
	if (isExcludedTerminalItem(item)) return null;
	if (isMergedItem(item)) return 'merged';
	if (
		reviewRefs.has(item.short_id.toUpperCase()) ||
		reviewRefs.has(item.id.toUpperCase()) ||
		isActionable(item, ownerId)
	) {
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

export function deriveProjectDeliverySummary(input: {
	snapshot: OrgSnapshot;
	projectId: string;
	workItems: readonly WorkItemSummary[];
	inboxItems: readonly InboxItem[];
	ownerId?: string;
}): ProjectDeliverySummary {
	const ownerId = input.ownerId ?? 'owner';
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
	const reviewRefs = buildReviewInboxSet(input.inboxItems);
	const activeAreas = (input.snapshot.areas ?? [])
		.filter((a) => a.project_id === input.projectId && !a.deleted)
		.sort((a, b) => (Boolean(a.general) !== Boolean(b.general) ? (a.general ? 1 : -1) : byName(a, b)));

	let tracks: ProjectDeliveryTrack[];
	if (activeAreas.length > 1) {
		tracks = groupByArea(input.projectId, projectItems, activeAreas, reviewRefs, ownerId, stageCounts);
	} else {
		const kinds: { key: WorkItemSummary['kind']; label: string }[] = [
			{ key: 'task', label: 'Tasks' },
			{ key: 'bug', label: 'Bugs' },
			{ key: 'request', label: 'Requests' },
			{ key: 'unknown', label: 'Other' }
		];
		tracks = [];
		for (const k of kinds) {
			const bucket = projectItems.filter((i) => i.kind === k.key);
			if (bucket.length > 0) {
				tracks.push(
					summarizeTrack(
						`${input.projectId}:track:${k.key}`,
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
			tracks.push(summarizeTrack(`${input.projectId}:track:task`, 'Tasks', [], reviewRefs, ownerId, stageCounts));
		}
	}

	const totalItems =
		stageCounts.queue + stageCounts.agents + stageCounts.ci + stageCounts.review + stageCounts.merged;
	return { stageCounts, totalItems, tracks };
}
