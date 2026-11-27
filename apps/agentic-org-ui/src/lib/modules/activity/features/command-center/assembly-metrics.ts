import { badgeInitials as initialsOf } from '$lib/format';
import type { ActivityRow } from '$lib/modules/activity/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import { agentsOf, byName, isFormer, nonOwners } from '$lib/modules/organization/model';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import {
	classifyAssemblyStage,
	compareItems,
	inboxItemHref,
	isPipelineMerged,
	workItemHref
} from './assembly-stages';
import type {
	AssemblyStage,
	BottleneckSummary,
	FeedPhase,
	RailAgentRow,
	ReleaseGateRow,
	StationAgentSlot,
	ThroughputDay,
	ThroughputSource
} from './assembly-types';

const DAY_MS = 86_400_000;
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'] as const;

function bottleneckNote(stageCounts: Readonly<Record<AssemblyStage, number>>): string {
	if (stageCounts.review > 0) {
		return `Reviewing ${stageCounts.review} ${stageCounts.review === 1 ? 'item' : 'items'} unblocks the line`;
	}
	if (stageCounts.queue > stageCounts.agents && stageCounts.queue > 0) {
		return `${stageCounts.queue} ${stageCounts.queue === 1 ? 'item' : 'items'} queued for agent pickup`;
	}
	if (stageCounts.ci > 0) {
		return `${stageCounts.ci} ${stageCounts.ci === 1 ? 'item' : 'items'} ready for checks`;
	}
	if (stageCounts.agents > 0) {
		return `${stageCounts.agents} ${stageCounts.agents === 1 ? 'item' : 'items'} actively in progress`;
	}
	return 'Pipeline flowing smoothly';
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
	for (const entry of [...floored].sort((a, b) => b.rem - a.rem)) {
		if (remaining <= 0) break;
		entry.pct += 1;
		remaining -= 1;
	}

	const pctMap = Object.fromEntries(floored.map((f) => [f.key, f.pct])) as Record<
		'agents' | 'ci' | 'review' | 'queue',
		number
	>;

	return {
		totalOpen,
		agentsPct: pctMap.agents,
		ciPct: pctMap.ci,
		reviewPct: pctMap.review,
		queuePct: pctMap.queue,
		legend: `${pctMap.agents}% agents · ${pctMap.ci}% ready for checks · ${pctMap.review}% waiting on you · ${pctMap.queue}% queued`,
		note: bottleneckNote(stageCounts)
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

function selectThroughputSource(input: {
	scopedProjectId: string | undefined;
	scopedActivityRows: readonly ActivityRow[];
	scopedWork: readonly WorkItemSummary[];
	feedPhase: FeedPhase;
	feedMore: number;
	workPhase: FeedPhase;
}): { source: ThroughputSource; timestamps: string[] } {
	const throughputRows = input.scopedProjectId
		? input.scopedActivityRows.filter((r) => r.kind === 'work')
		: input.scopedActivityRows;

	if (input.feedPhase === 'loading') return { source: 'loading', timestamps: [] };
	if (input.feedPhase === 'error') return { source: 'unavailable', timestamps: [] };
	if (throughputRows.length > 0) {
		return {
			source: input.feedMore > 0 ? 'events_capped' : 'events',
			timestamps: throughputRows.map((r) => r.at)
		};
	}
	if (input.workPhase === 'ready') {
		return { source: 'work_updates', timestamps: input.scopedWork.map((i) => i.updated_at) };
	}
	if (input.workPhase === 'loading') return { source: 'loading', timestamps: [] };
	return { source: 'unavailable', timestamps: [] };
}

export function deriveThroughput(input: {
	scopedProjectId: string | undefined;
	scopedActivityRows: readonly ActivityRow[];
	scopedWork: readonly WorkItemSummary[];
	feedPhase: FeedPhase;
	feedMore: number;
	workPhase: FeedPhase;
	nowMs: number;
}): {
	throughput7d: ThroughputDay[];
	throughputSource: ThroughputSource;
	throughputMeta: string;
	throughputAriaLabel: string;
	throughputUnavailableMessage?: string;
} {
	const { source, timestamps } = selectThroughputSource(input);
	const throughput7d = computeThroughput7d(timestamps, input.nowMs);
	const total7dCount = throughput7d.reduce((sum, d) => sum + d.count, 0);
	const eventNoun = input.scopedProjectId ? 'work event' : 'event';
	let throughputMeta = `${total7dCount} ${total7dCount === 1 ? eventNoun : `${eventNoun}s`}`;
	let throughputAriaLabel = input.scopedProjectId
		? '7-day work delivery events'
		: '7-day activity events';
	let throughputUnavailableMessage: string | undefined;

	if (source === 'loading') {
		throughputMeta = 'Loading…';
	} else if (source === 'unavailable') {
		const isFeedErr = input.feedPhase === 'error';
		throughputMeta = isFeedErr ? 'Feed unavailable' : 'Work data unavailable';
		throughputUnavailableMessage = isFeedErr
			? 'Activity feed is unavailable for 7-day throughput.'
			: 'Work data is unavailable for 7-day throughput.';
	} else if (source === 'events_capped') {
		throughputMeta = 'Recent events (capped)';
		throughputAriaLabel = 'Recent 7-day activity events (capped)';
	} else if (source === 'work_updates') {
		throughputMeta = 'Items last updated (7d)';
		throughputAriaLabel = '7-day items last updated';
	}

	return {
		throughput7d,
		throughputSource: source,
		throughputMeta,
		throughputAriaLabel,
		throughputUnavailableMessage
	};
}

function buildApprovalGate(openCount: number, inboxPhase: FeedPhase): ReleaseGateRow {
	if (inboxPhase === 'loading') {
		return {
			id: 'approvals',
			label: 'Human approvals',
			statusLabel: 'Loading…',
			tone: 'neutral',
			count: 0,
			href: '/inbox'
		};
	}
	if (inboxPhase === 'error') {
		return {
			id: 'approvals',
			label: 'Human approvals',
			statusLabel: 'Unavailable',
			tone: 'warn',
			count: 0,
			href: '/inbox'
		};
	}
	return {
		id: 'approvals',
		label: 'Human approvals',
		statusLabel: openCount === 0 ? 'Clear' : `${openCount} pending`,
		tone: openCount === 0 ? 'ok' : 'warn',
		count: openCount,
		href: '/inbox'
	};
}

export function deriveReleaseGates(
	scopedOpenInbox: readonly InboxItem[],
	scopedWork: readonly WorkItemSummary[],
	inboxPhase: FeedPhase
): {
	releaseGates: ReleaseGateRow[];
	pendingApprovals: { id: string; title: string; href: string }[];
} {
	const openApprovals = scopedOpenInbox.filter((i) => i.kind === 'approval');
	const openDecisions = scopedOpenInbox.filter((i) => i.kind === 'decision');
	const criticalOpenBugs = scopedWork.filter(
		(i) => i.kind === 'bug' && i.status !== 'closed' && i.severity === 'critical'
	).length;
	const blockedTasks = scopedWork.filter((i) => i.kind === 'task' && i.status === 'blocked').length;

	const releaseGates: ReleaseGateRow[] = [buildApprovalGate(openApprovals.length, inboxPhase)];
	if (inboxPhase === 'ready' && openDecisions.length > 0) {
		releaseGates.push({
			id: 'decisions',
			label: 'Open decisions',
			statusLabel: `${openDecisions.length} pending`,
			tone: 'warn',
			count: openDecisions.length,
			href: '/inbox'
		});
	}
	releaseGates.push(
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
	);

	const pendingApprovals =
		inboxPhase === 'ready'
			? openApprovals.slice(0, 3).map((item) => ({
					id: item.id,
					title: item.title,
					href: inboxItemHref(item.id)
				}))
			: [];

	return { releaseGates, pendingApprovals };
}

const STAGE_PRIORITY: Record<AssemblyStage, number> = {
	agents: 0,
	ci: 1,
	review: 2,
	queue: 3,
	merged: 4
};

const AGENT_STATUS_RANK: Record<RailAgentRow['status'], number> = {
	building: 0,
	waiting: 1,
	paused: 2,
	idle: 3
};

function agentStatusFromLead(
	agentStatus: string | undefined,
	leadStage: AssemblyStage | null
): { status: RailAgentRow['status']; statusLabel: string } {
	if (agentStatus === 'paused') return { status: 'paused', statusLabel: 'Paused' };
	if (leadStage === 'agents' || leadStage === 'ci') {
		return { status: 'building', statusLabel: leadStage === 'ci' ? 'Checks ready' : 'Building' };
	}
	if (leadStage === 'review') return { status: 'waiting', statusLabel: 'Needs input' };
	return { status: 'idle', statusLabel: 'Idle' };
}

function collectProjectTeamIds(
	snapshot: OrgSnapshot,
	scopedProjectId: string | undefined,
	scopedTeamIds?: readonly string[]
): Set<string> {
	const teamIds = new Set<string>(scopedTeamIds ?? []);
	if (!scopedProjectId || teamIds.size > 0) return teamIds;
	for (const pt of snapshot.project_teams ?? []) {
		if (pt.project_id === scopedProjectId) teamIds.add(pt.team_id);
	}
	return teamIds;
}

function indexOpenWorkByAssignee(scopedWork: readonly WorkItemSummary[]): {
	scopedAssigneeIds: Set<string>;
	workByAssignee: Map<string, WorkItemSummary[]>;
} {
	const scopedAssigneeIds = new Set<string>();
	const workByAssignee = new Map<string, WorkItemSummary[]>();
	for (const item of scopedWork) {
		if (!item.assignee_id) continue;
		scopedAssigneeIds.add(item.assignee_id);
		if (isPipelineMerged(item)) continue;
		const list = workByAssignee.get(item.assignee_id) ?? [];
		list.push(item);
		workByAssignee.set(item.assignee_id, list);
	}
	return { scopedAssigneeIds, workByAssignee };
}

function buildRailAgentRow(
	member: OrgSnapshot['members'][number],
	assignedItems: readonly WorkItemSummary[],
	reviewInboxByRef: ReadonlyMap<string, InboxItem>,
	ownerId: string
): RailAgentRow {
	const sorted = [...assignedItems].sort((a, b) => {
		const sa = classifyAssemblyStage(a, reviewInboxByRef, ownerId) ?? 'merged';
		const sb = classifyAssemblyStage(b, reviewInboxByRef, ownerId) ?? 'merged';
		return STAGE_PRIORITY[sa] - STAGE_PRIORITY[sb] || compareItems(a, b);
	});
	const leadItem = sorted[0];
	const leadStage = leadItem ? classifyAssemblyStage(leadItem, reviewInboxByRef, ownerId) : null;
	const { status, statusLabel } = agentStatusFromLead(member.agent?.status, leadStage);
	return {
		id: member.id,
		name: member.name,
		initials: initialsOf(member.name),
		title: member.agent?.title || member.agent?.role || 'Agent',
		status,
		statusLabel,
		currentWork: leadItem
			? { shortId: leadItem.short_id, title: leadItem.title, href: workItemHref(leadItem) }
			: undefined,
		href: `/members/${encodeURIComponent(member.id)}`
	};
}

export function deriveRailAndStationAgents(input: {
	snapshot: OrgSnapshot;
	scopedWork: readonly WorkItemSummary[];
	scopedProjectId: string | undefined;
	scopedTeamIds?: readonly string[];
	busyAssigneeIds: ReadonlySet<string>;
	reviewInboxByRef: ReadonlyMap<string, InboxItem>;
	ownerId: string;
}): {
	activeAgentsCount: number;
	busyAgentsCount: number;
	stationAgents: StationAgentSlot[];
	railAgents: RailAgentRow[];
	extraAgentsCount: number;
} {
	const nonFormerAgents = agentsOf(nonOwners(input.snapshot)).filter((m) => !isFormer(m));
	const allActiveAgents = nonFormerAgents.filter((m) => m.agent?.status === 'active');
	const projectTeamSet = collectProjectTeamIds(
		input.snapshot,
		input.scopedProjectId,
		input.scopedTeamIds
	);
	const { scopedAssigneeIds, workByAssignee } = indexOpenWorkByAssignee(input.scopedWork);
	const filterByScope =
		Boolean(input.scopedProjectId) && (projectTeamSet.size > 0 || scopedAssigneeIds.size > 0);
	const activeAgents = filterByScope
		? allActiveAgents.filter(
				(a) =>
					(a.home_team_id && projectTeamSet.has(a.home_team_id)) ||
					scopedAssigneeIds.has(a.id) ||
					input.busyAssigneeIds.has(a.id)
			)
		: allActiveAgents;

	const stationAgents: StationAgentSlot[] = activeAgents.map((a) => ({
		id: a.id,
		name: a.name,
		busy: input.busyAssigneeIds.has(a.id)
	}));

	const allRailAgents = nonFormerAgents
		.map((member) =>
			buildRailAgentRow(
				member,
				workByAssignee.get(member.id) ?? [],
				input.reviewInboxByRef,
				input.ownerId
			)
		)
		.sort((a, b) => AGENT_STATUS_RANK[a.status] - AGENT_STATUS_RANK[b.status] || byName(a, b));
	const railAgents = allRailAgents.slice(0, 6);

	return {
		activeAgentsCount: activeAgents.length,
		busyAgentsCount: activeAgents.filter((a) => input.busyAssigneeIds.has(a.id)).length,
		stationAgents,
		railAgents,
		extraAgentsCount: Math.max(0, allRailAgents.length - railAgents.length)
	};
}
