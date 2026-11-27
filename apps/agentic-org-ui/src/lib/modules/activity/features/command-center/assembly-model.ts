import type { InboxItem } from '$lib/modules/inbox/types';
import { byName, owner } from '$lib/modules/organization/model';
import type { ProjectSummary, WorkItemSummary } from '$lib/proto';
import {
	computeBottleneck,
	computeThroughput7d,
	deriveRailAndStationAgents,
	deriveReleaseGates,
	deriveThroughput
} from './assembly-metrics';
import {
	belongsOnlyToArchivedProjects,
	buildMilestones,
	buildReviewInboxLookup,
	classifyAssemblyStage,
	computeCompletionPct,
	extractInboxWorkRefs,
	extractWorkRefFromPath,
	extractWorkRefsFromText,
	formatTelemetryHours,
	inboxItemHref,
	isActionableWorkItem,
	isExcludedTerminalItem,
	isPipelineMerged,
	resolveInboxProjectId,
	workItemHref
} from './assembly-stages';
import {
	deriveDecisionStreamEntries,
	deriveStreamEntries,
	filterStreamEntries,
	groupStreamEntriesByDay,
	mergeLiveActivityRows
} from './assembly-stream';
import type {
	ActivityCommandCenterModel,
	AssemblyStage,
	CommandCenterInput,
	FeedPhase,
	LineEventEntry
} from './assembly-types';

export * from './assembly-types';
export {
	classifyAssemblyStage,
	computeBottleneck,
	computeCompletionPct,
	computeThroughput7d,
	deriveDecisionStreamEntries,
	deriveStreamEntries,
	extractInboxWorkRefs,
	extractWorkRefFromPath,
	extractWorkRefsFromText,
	filterStreamEntries,
	formatTelemetryHours,
	groupStreamEntriesByDay,
	inboxItemHref,
	isActionableWorkItem,
	isExcludedTerminalItem,
	isPipelineMerged,
	mergeLiveActivityRows,
	workItemHref
};

function tallyStageMetrics(
	scopedWork: readonly WorkItemSummary[],
	reviewInboxByRef: ReadonlyMap<string, InboxItem>,
	ownerId: string
): {
	stageCounts: Record<AssemblyStage, number>;
	actionableWorkRefs: Set<string>;
	busyAssigneeIds: Set<string>;
	linkedInboxIds: Set<string>;
	actionableWorkCount: number;
	humanWaitingItems: { id: string; title: string; href: string }[];
} {
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
	const humanWaitingItems: { id: string; title: string; href: string }[] = [];
	let actionableWorkCount = 0;

	for (const item of scopedWork) {
		const stage = classifyAssemblyStage(item, reviewInboxByRef, ownerId);
		if (!stage) continue;
		stageCounts[stage] += 1;
		if (isActionableWorkItem(item, ownerId)) {
			actionableWorkCount += 1;
			actionableWorkRefs.add(item.short_id.toUpperCase());
			actionableWorkRefs.add(item.id.toUpperCase());
		}
		if (stage === 'review') {
			const linked =
				reviewInboxByRef.get(item.short_id.toUpperCase()) ??
				reviewInboxByRef.get(item.id.toUpperCase());
			if (linked) linkedInboxIds.add(linked.id);
			humanWaitingItems.push({
				id: item.id,
				title: `${item.short_id} · ${item.title}`,
				href: linked ? inboxItemHref(linked.id) : workItemHref(item)
			});
		}
		if ((stage === 'agents' || stage === 'ci') && item.assignee_id) {
			busyAssigneeIds.add(item.assignee_id);
		}
	}

	return {
		stageCounts,
		actionableWorkRefs,
		busyAssigneeIds,
		linkedInboxIds,
		actionableWorkCount,
		humanWaitingItems
	};
}

function buildLiveStatusText(
	stageCounts: Readonly<Record<AssemblyStage, number>>,
	isComplete: boolean
): string {
	if (isComplete) return 'All items complete';
	if (stageCounts.review > 0) return `${stageCounts.review} awaiting human review`;
	if (stageCounts.agents > 0 || stageCounts.ci > 0) {
		return `Assembling · ${stageCounts.agents + stageCounts.ci} active`;
	}
	if (stageCounts.queue > 0) return `${stageCounts.queue} queued`;
	return 'Ready for work';
}

function buildStationsNote(inboxPhase: FeedPhase, needsYouCount: number): string {
	if (inboxPhase === 'error') return 'Inbox unavailable; approval wait counts may be incomplete';
	if (inboxPhase === 'loading') return 'Loading Inbox review items…';
	if (needsYouCount > 0) {
		return `${needsYouCount} ${needsYouCount === 1 ? 'item' : 'items'} waiting on human review`;
	}
	return 'No human review bottlenecks right now';
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

	const scopedProjectId = input.projectScope !== 'all' ? input.projectScope : undefined;
	const selectedProject = scopedProjectId
		? (snapshot.projects ?? []).find((p) => p.id === scopedProjectId)
		: undefined;
	const displayScopedProject: ProjectSummary | undefined = scopedProjectId
		? (selectedProject ?? { id: scopedProjectId, name: organizationName })
		: undefined;

	const workByRef = new Map<string, WorkItemSummary>();
	for (const item of workItems) {
		if (item.deleted) continue;
		workByRef.set(item.short_id.toUpperCase(), item);
		workByRef.set(item.id.toUpperCase(), item);
	}

	const pipelineWork = workItems.filter((i) => !isExcludedTerminalItem(i));
	const scopedWork = scopedProjectId
		? pipelineWork.filter((i) => i.project_id === scopedProjectId)
		: pipelineWork.filter((i) => !i.project_id || !archivedProjectIds.has(i.project_id));

	const allOpenInbox = inboxPhase === 'ready' ? inboxItems.filter((i) => i.state === 'open') : [];
	const scopedOpenInbox = scopedProjectId
		? allOpenInbox.filter((i) => resolveInboxProjectId(i, workByRef) === scopedProjectId)
		: allOpenInbox.filter((i) => {
				const pid = resolveInboxProjectId(i, workByRef);
				return !pid || !archivedProjectIds.has(pid);
			});

	const reviewInboxByRef = buildReviewInboxLookup(scopedOpenInbox);
	const openInboxWorkRefs = new Set<string>(reviewInboxByRef.keys());

	const scopedActivityRows = scopedProjectId
		? activityRows.filter((row) => row.projectIds.includes(scopedProjectId))
		: activityRows.filter(
				(row) => !belongsOnlyToArchivedProjects(row.projectIds, archivedProjectIds)
			);

	const milestones = buildMilestones({
		projectsToRender: displayScopedProject ? [displayScopedProject] : activeProjects,
		scopedProjectId,
		activeProjectIds,
		scopedWork,
		areas: snapshot.areas ?? [],
		snapshot,
		reviewInboxByRef,
		ownerId: resolvedOwnerId
	});

	const {
		stageCounts,
		actionableWorkRefs,
		busyAssigneeIds,
		linkedInboxIds,
		actionableWorkCount,
		humanWaitingItems
	} = tallyStageMetrics(scopedWork, reviewInboxByRef, resolvedOwnerId);

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

	const { activeAgentsCount, busyAgentsCount, stationAgents, railAgents, extraAgentsCount } =
		deriveRailAndStationAgents({
			snapshot,
			scopedWork,
			scopedProjectId,
			scopedTeamIds: input.scopedTeamIds,
			busyAssigneeIds,
			reviewInboxByRef,
			ownerId: resolvedOwnerId
		});

	const streamEntries = deriveStreamEntries(
		snapshot,
		scopedActivityRows,
		scopedWork,
		scopedOpenInbox,
		reviewInboxByRef,
		actionableWorkRefs,
		resolvedOwnerId,
		feedPhase,
		workPhase
	);
	const throughput = deriveThroughput({
		scopedProjectId,
		scopedActivityRows,
		scopedWork,
		feedPhase,
		feedMore,
		workPhase,
		nowMs
	});
	const { releaseGates, pendingApprovals } = deriveReleaseGates(
		scopedOpenInbox,
		scopedWork,
		inboxPhase
	);

	const ciColumnSubLabel = `${stageCounts.ci} ready for checks`;
	const scopeEyebrow = displayScopedProject
		? `${displayScopedProject.key ? `${displayScopedProject.key.toUpperCase()} · ` : ''}PRODUCTION LINE`
		: 'LIVE PIPELINE';
	const scopeTitle = displayScopedProject ? displayScopedProject.name : 'All projects';
	const scopeSubtitle = displayScopedProject
		? `${displayScopedProject.name} · ${openItems} open, ${mergedItems} done`
		: `Assembly line and live stream across ${activeProjects.length} active ${activeProjects.length === 1 ? 'project' : 'projects'} in ${organizationName}`;

	const lineEvents: LineEventEntry[] =
		feedPhase === 'ready' && scopedActivityRows.length > 0
			? streamEntries.slice(0, 30).map((entry) => ({
					id: entry.id,
					at: entry.at,
					tone:
						entry.tone === 'ok' || entry.tone === 'warn' || entry.tone === 'bad' ? entry.tone : '',
					text: `${entry.actorName} ${entry.text}`.trim()
				}))
			: [];

	return {
		activeProjects,
		selectedProject,
		scopeTitle,
		scopeEyebrow,
		scopeSubtitle,
		liveStatusText: buildLiveStatusText(stageCounts, isComplete),
		stageCounts,
		ciColumnSubLabel,
		ciStationLabel: ciColumnSubLabel,
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
		avgCycleHours: null,
		waitingOnPeopleHours: humanWaitingItems.length === 0 ? 0 : null,
		actionableWorkRefs,
		openInboxWorkRefs,
		stationAgents,
		humanWaitingItems,
		lineEvents,
		milestones,
		bottleneck: computeBottleneck(stageCounts),
		releaseGates,
		pendingApprovals,
		stationsNote: buildStationsNote(inboxPhase, needsYouCount),
		...throughput,
		railAgents,
		extraAgentsCount,
		streamEntries,
		streamNeedsYouCount: streamEntries.filter((e) => e.needsYou).length
	};
}
