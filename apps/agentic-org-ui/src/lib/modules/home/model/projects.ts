import { isItemDone } from '$lib/format';
import type { HomeProject } from '$lib/modules/home/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import { leadOf, teamsOfProject } from '$lib/modules/organization/model';
import type { ProjectRow } from '$lib/modules/projects/types';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import {
	formatDueShort,
	parseIsoDateUtc,
	type HomeDomainData,
	type HomeMilestone,
	type ProjectBentoCard
} from './types';

interface ProjectWorkSignals {
	doneCount: number;
	totalCount: number;
	pct: number;
	blockedItems: WorkItemSummary[];
	blockedCount: number;
	criticalBugs: number;
	scopedReqs: WorkItemSummary[];
}

function resolveProjectDoneTotal(
	projItems: readonly WorkItemSummary[],
	row: ProjectRow | undefined,
	homeProj: HomeProject | undefined
): { doneCount: number; totalCount: number } {
	if (homeProj && homeProj.total > 0)
		return { doneCount: homeProj.done, totalCount: homeProj.total };
	const rowTotal = row
		? row.tasks.todo + row.tasks.inProgress + row.tasks.blocked + row.tasks.done
		: 0;
	if (row && rowTotal > 0) return { doneCount: row.tasks.done, totalCount: rowTotal };
	return { doneCount: projItems.filter(isItemDone).length, totalCount: projItems.length };
}

function isSeriousOpenBug(item: WorkItemSummary): boolean {
	return (
		item.kind === 'bug' &&
		item.status !== 'closed' &&
		(item.severity === 'critical' || item.severity === 'major')
	);
}

function computeProjectSignals(
	projItems: readonly WorkItemSummary[],
	row: ProjectRow | undefined,
	homeProj: HomeProject | undefined
): ProjectWorkSignals {
	const { doneCount, totalCount } = resolveProjectDoneTotal(projItems, row, homeProj);
	const pct = totalCount > 0 ? Math.round((doneCount * 100) / totalCount) : 0;
	const blockedItems = projItems.filter((i) => i.status === 'blocked');
	const blockedCount = Math.max(
		blockedItems.length,
		row?.tasks.blocked ?? 0,
		homeProj?.blocked ?? 0
	);
	const criticalBugs = Math.max(
		projItems.filter(isSeriousOpenBug).length,
		row ? row.bugs.critical + row.bugs.major : 0
	);
	const scopedReqs = projItems.filter((i) => i.kind === 'request' && i.status === 'scoped');
	return { doneCount, totalCount, pct, blockedItems, blockedCount, criticalBugs, scopedReqs };
}

function classifyProjectHealth(input: {
	criticalBugs: number;
	blockedCount: number;
	projProblemsCount: number;
	hasRowCriticalBug: boolean;
	isOverdue: boolean;
	scopedReqsCount: number;
	projInboxCount: number;
	unassignedTeam: boolean;
}): {
	health: ProjectBentoCard['health'];
	healthTone: ProjectBentoCard['healthTone'];
	statusPillClass: ProjectBentoCard['statusPillClass'];
} {
	const offTrack =
		input.criticalBugs > 0 ||
		input.blockedCount >= 2 ||
		(input.projProblemsCount > 0 && input.blockedCount > 0) ||
		input.hasRowCriticalBug;
	if (offTrack) return { health: 'Off track', healthTone: 'bad', statusPillClass: 't-red' };

	const atRisk =
		input.isOverdue ||
		input.blockedCount > 0 ||
		input.scopedReqsCount > 0 ||
		input.projInboxCount >= 2 ||
		input.unassignedTeam;
	if (atRisk) return { health: 'At risk', healthTone: 'warn', statusPillClass: 't-amber' };

	return { health: 'On track', healthTone: 'ok', statusPillClass: 't-green' };
}

interface BlockerPresentation {
	blockerText: string;
	blockerTone: ProjectBentoCard['blockerTone'];
	actionLabel: string;
	actionHref: string;
	spotDecisionId?: string;
	isClear: boolean;
	clearNote: string;
}

function deriveApprovalBlocker(
	projApprovals: readonly InboxItem[],
	healthTone: ProjectBentoCard['healthTone'],
	clearNote: string
): BlockerPresentation {
	const n = projApprovals.length;
	const first = projApprovals[0];
	const actionLabel = n > 1 ? `Review (${n})` : healthTone === 'ok' ? 'Sign off' : 'Review';
	return {
		blockerText: n === 1 ? first.title : `${n} sign-offs waiting in Inbox`,
		blockerTone: healthTone === 'warn' ? 'warn' : 'sky',
		actionLabel,
		actionHref: `/inbox?item=${encodeURIComponent(first.id)}`,
		spotDecisionId: first.id,
		isClear: false,
		clearNote
	};
}

function deriveWorkQueueBlocker(
	projectId: string,
	signals: ProjectWorkSignals,
	unassignedTeam: boolean,
	clearNote: string
): BlockerPresentation | undefined {
	if (signals.blockedCount > 0) {
		const n = signals.blockedCount;
		return {
			blockerText:
				signals.blockedItems[0]?.blocked_reason || `${n} ${n === 1 ? 'task' : 'tasks'} blocked`,
			blockerTone: 'warn',
			actionLabel: 'Unblock',
			actionHref: '/inbox',
			spotDecisionId: signals.blockedItems[0]?.id,
			isClear: false,
			clearNote
		};
	}
	if (signals.scopedReqs.length > 0) {
		const n = signals.scopedReqs.length;
		return {
			blockerText: `${n} scoped ${n === 1 ? 'request' : 'requests'} waiting sign-off`,
			blockerTone: 'sky',
			actionLabel: 'Review',
			actionHref: '/inbox',
			spotDecisionId: signals.scopedReqs[0].id,
			isClear: false,
			clearNote
		};
	}
	if (unassignedTeam) {
		return {
			blockerText: 'No team assigned yet',
			blockerTone: 'warn',
			actionLabel: 'Assign',
			actionHref: `/projects/${projectId}/team`,
			isClear: false,
			clearNote
		};
	}
	return undefined;
}

function deriveProjectBlocker(input: {
	projectId: string;
	workHref: string;
	healthTone: ProjectBentoCard['healthTone'];
	signals: ProjectWorkSignals;
	projProblems: readonly InboxItem[];
	projApprovals: readonly InboxItem[];
	unassignedTeam: boolean;
	isOverdue: boolean;
}): BlockerPresentation {
	const remainingCount = Math.max(0, input.signals.totalCount - input.signals.doneCount);
	const clearNote = input.isOverdue
		? remainingCount > 0
			? `Past due date · ${remainingCount} open`
			: 'Past due date · review remaining work'
		: 'All checks passing · on schedule';

	if (input.projProblems.length > 0) {
		const first = input.projProblems[0];
		return {
			blockerText: first.title,
			blockerTone: 'bad',
			actionLabel: 'Inspect',
			actionHref: `/inbox?item=${encodeURIComponent(first.id)}`,
			spotDecisionId: first.id,
			isClear: false,
			clearNote
		};
	}
	if (input.projApprovals.length > 0 && input.healthTone === 'bad') {
		const first = input.projApprovals[0];
		return {
			blockerText: input.signals.blockedItems[0]?.blocked_reason || first.title,
			blockerTone: 'bad',
			actionLabel: 'Unblock',
			actionHref: `/inbox?item=${encodeURIComponent(first.id)}`,
			spotDecisionId: first.id,
			isClear: false,
			clearNote
		};
	}
	if (input.signals.criticalBugs > 0) {
		const n = input.signals.criticalBugs;
		return {
			blockerText: `${n} ${n === 1 ? 'serious bug' : 'serious bugs'} open`,
			blockerTone: 'bad',
			actionLabel: 'Inspect',
			actionHref: input.workHref,
			isClear: false,
			clearNote
		};
	}
	if (input.projApprovals.length > 0) {
		return deriveApprovalBlocker(input.projApprovals, input.healthTone, clearNote);
	}
	const workBlocker = deriveWorkQueueBlocker(
		input.projectId,
		input.signals,
		input.unassignedTeam,
		clearNote
	);
	if (workBlocker) return workBlocker;

	return {
		blockerText: clearNote,
		blockerTone: input.isOverdue ? 'bad' : 'ok',
		actionLabel: 'Open work',
		actionHref: input.workHref,
		isClear: !input.isOverdue,
		clearNote
	};
}

function deriveProjectPace(
	health: ProjectBentoCard['health'],
	pct: number,
	isOverdue: boolean
): { paceLabel?: string; paceTone: ProjectBentoCard['paceTone'] } {
	if (isOverdue) return { paceLabel: 'Overdue', paceTone: 'bad' };
	if (health === 'Off track') return { paceLabel: 'Behind pace', paceTone: 'bad' };
	if (health === 'At risk') return { paceLabel: 'At risk', paceTone: 'warn' };
	return { paceLabel: pct >= 50 ? 'On pace' : undefined, paceTone: 'ok' };
}

function utcNoonMs(nowMs: number): number {
	const d = new Date(nowMs);
	return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate(), 12);
}

const RISK_RANK: Record<ProjectBentoCard['health'], number> = {
	'Off track': 0,
	'At risk': 1,
	'On track': 2
};

export function deriveProjectCards(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[],
	domain?: Pick<HomeDomainData, 'projectRows' | 'homeProjects' | 'inboxItems'>,
	nowMs = Date.now()
): ProjectBentoCard[] {
	const activeSnapshotProjects = (snapshot.projects ?? []).filter((p) => !p.archived);
	const rowById = new Map(
		(domain?.projectRows ?? []).filter((p) => !p.archived).map((p) => [p.id, p])
	);
	const homeById = new Map((domain?.homeProjects ?? []).map((p) => [p.id, p]));
	const openInbox = (domain?.inboxItems ?? []).filter((i) => i.state === 'open');
	const todayUtc = utcNoonMs(nowMs);

	const projectIds = new Set<string>([
		...rowById.keys(),
		...activeSnapshotProjects.map((p) => p.id)
	]);

	const cards: ProjectBentoCard[] = [];
	for (const projectId of projectIds) {
		const snapProj = activeSnapshotProjects.find((p) => p.id === projectId);
		const row = rowById.get(projectId);
		const homeProj = homeById.get(projectId);
		const projItems = items.filter((i) => !i.deleted && i.project_id === projectId);
		const signals = computeProjectSignals(projItems, row, homeProj);

		const projInbox = openInbox.filter((i) => i.project?.id === projectId);
		const projProblems = projInbox.filter((i) => i.kind === 'problem');
		const projApprovals = projInbox.filter((i) => i.kind === 'approval' || i.kind === 'decision');

		const teams = teamsOfProject(snapshot, projectId);
		const lead = homeProj?.lead ?? teams.map((t) => leadOf(snapshot, t)).find(Boolean);

		const dueDt = homeProj?.due ? parseIsoDateUtc(homeProj.due) : null;
		const isOverdue = Boolean(
			dueDt &&
			dueDt.getTime() < todayUtc &&
			signals.totalCount > 0 &&
			signals.doneCount < signals.totalCount
		);
		const unassignedTeam = teams.length === 0 && !row;

		const { health, healthTone, statusPillClass } = classifyProjectHealth({
			criticalBugs: signals.criticalBugs,
			blockedCount: signals.blockedCount,
			projProblemsCount: projProblems.length,
			hasRowCriticalBug: Boolean(row && row.bugs.critical > 0),
			isOverdue,
			scopedReqsCount: signals.scopedReqs.length,
			projInboxCount: projInbox.length,
			unassignedTeam
		});

		const workHref = `/projects/${projectId}/work`;
		const blocker = deriveProjectBlocker({
			projectId,
			workHref,
			healthTone,
			signals,
			projProblems,
			projApprovals,
			unassignedTeam,
			isOverdue
		});
		const pace = deriveProjectPace(health, signals.pct, isOverdue);

		cards.push({
			id: projectId,
			name: row?.name ?? snapProj?.name ?? homeProj?.name ?? projectId,
			key: snapProj?.key ?? row?.slug,
			health,
			healthTone,
			statusPillClass,
			doneCount: signals.doneCount,
			totalCount: signals.totalCount,
			pct: signals.pct,
			dueLabel: homeProj?.due ? formatDueShort(homeProj.due) : undefined,
			paceLabel: pace.paceLabel,
			paceTone: pace.paceTone,
			blockerText: blocker.blockerText,
			blockerTone: blocker.blockerTone,
			isClear: blocker.isClear,
			clearNote: blocker.clearNote,
			actionLabel: blocker.actionLabel,
			actionHref: blocker.actionHref,
			spotDecisionId: blocker.spotDecisionId,
			workHref,
			leadId: lead?.id,
			leadName:
				lead?.name ??
				row?.ownerTeam.name ??
				(teams[0]?.name ? `${teams[0].name} team` : 'Unassigned')
		});
	}

	return cards.sort(
		(a, b) => RISK_RANK[a.health] - RISK_RANK[b.health] || a.name.localeCompare(b.name)
	);
}

export function deriveUpcomingMilestones(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[],
	nowMs = Date.now(),
	domain?: Pick<HomeDomainData, 'projectRows' | 'homeProjects' | 'inboxItems'>
): HomeMilestone[] {
	const dated = (domain?.homeProjects ?? []).filter((p): p is HomeProject & { due: string } =>
		Boolean(p.due)
	);
	if (dated.length === 0) return [];

	const cardsById = new Map(
		deriveProjectCards(snapshot, items, domain, nowMs).map((card) => [card.id, card])
	);
	const monthFmt = new Intl.DateTimeFormat('en-US', { month: 'short', timeZone: 'UTC' });
	const todayUtc = utcNoonMs(nowMs);

	return dated
		.map((hp) => {
			const dt = parseIsoDateUtc(hp.due);
			const diffDays = dt ? Math.round((dt.getTime() - todayUtc) / 86_400_000) : null;
			return { hp, dt, diffDays };
		})
		.filter(
			(entry): entry is { hp: HomeProject & { due: string }; dt: Date; diffDays: number } =>
				entry.dt !== null && entry.diffDays !== null && entry.diffDays >= 0 && entry.diffDays <= 14
		)
		.sort((a, b) => a.diffDays - b.diffDays || a.hp.name.localeCompare(b.hp.name))
		.slice(0, 3)
		.map(({ hp, dt, diffDays }): HomeMilestone => {
			const card = cardsById.get(hp.id);
			const tone: HomeMilestone['tone'] =
				card?.healthTone === 'bad'
					? 'bad'
					: card?.healthTone === 'warn'
						? 'warn'
						: card?.spotDecisionId
							? 'sky'
							: 'ok';
			const statusLabel =
				diffDays === 0 ? 'Today' : diffDays === 1 ? 'Tomorrow' : `In ${diffDays} days`;
			return {
				id: `ms-${hp.id}`,
				month: monthFmt.format(dt),
				day: String(dt.getUTCDate()).padStart(2, '0'),
				title: hp.name,
				releaseTag: card?.key,
				note: `${hp.lead?.name ?? card?.leadName ?? 'Team'} · ${hp.total > 0 ? `${hp.done}/${hp.total} done` : 'Scheduled'}`,
				statusLabel,
				tone,
				spotId: card?.spotDecisionId,
				href: card?.actionHref ?? `/projects/${hp.id}/work`
			};
		});
}
