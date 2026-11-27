import { badgeInitials as initialsOf, isItemDone, relativeAge } from '$lib/format';
import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import type { ActivityRow } from '$lib/modules/activity/types';
import type { AgentBudgets, OrganizationCost } from '$lib/modules/costs/types';
import type { HomeProject } from '$lib/modules/home/types';
import {
	deriveDecisionSpotlight,
	isDenyAction,
	type SpotlightCell,
	type SpotlightDecision,
	type SpotlightPrimaryAction,
	type SpotlightSecondaryAction
} from '$lib/modules/inbox/spotlight';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { MemberRow } from '$lib/modules/members/types';
import type { ProjectRow } from '$lib/modules/projects/types';
import {
	agentsOf,
	isFormer,
	leadOf,
	memberById,
	nonOwners,
	peopleOf,
	teamsOfProject
} from '$lib/modules/organization/model';
import { needsYou } from '$lib/modules/work/saved';

export {
	deriveDecisionSpotlight,
	initialsOf,
	isDenyAction,
	isItemDone,
	relativeAge,
	type SpotlightCell,
	type SpotlightDecision,
	type SpotlightPrimaryAction,
	type SpotlightSecondaryAction
};

export type SignalTone = 'ok' | 'warn' | 'bad' | 'sky' | 'muted';
export type BriefLens = 'today' | 'week' | 'board';

export type HomeInboxState = 'loading' | 'ready' | 'unavailable';

export interface HomeDomainData {
	orgCost?: OrganizationCost;
	agentBudgets?: AgentBudgets;
	inboxItems?: readonly InboxItem[];
	inboxState?: HomeInboxState;
	memberRows?: readonly MemberRow[];
	projectRows?: readonly ProjectRow[];
	homeProjects?: readonly HomeProject[];
	activityRows?: readonly ActivityRow[];
}

export interface FinancialKpi {
	available: boolean;
	spentMicroUsd: number | null;
	spentLabel: string;
	budgetMicroUsd: number | null;
	budgetLabel: string;
	usagePct: number;
	pillLabel: string;
	pillTone: SignalTone;
	cappedAgent?: { memberId: string; name: string };
	costPerTaskLabel: string;
	budgetedAgentsCount: number;
	recordsCount: number;
}

export interface ExecutiveKpis {
	financials: FinancialKpi;
	openCount: number;
	doneCount: number;
	blockedCount: number;
	urgentCount: number;
	completionPct: number;
	activeAgents: number;
	pausedAgents: number;
	unplacedAgents: number;
	totalAgents: number;
	leadsCount: number;
	specialistsCount: number;
	budgetRemainingPct: number | null;
	peopleCount: number;
	autonomyPct: number;
	onlineOffices: number;
	offlineOffices: number;
	offlineOfficeNames: string[];
	officePillLabel: string;
	officePillTone: SignalTone;
	projectsTotal: number;
	projectsAttention: number;
	offTrackProjects: number;
	atRiskProjects: number;
	openBugs: number;
	criticalBugs: number;
	firstTryPassPct: number;
}

export interface ProjectBentoCard {
	id: string;
	name: string;
	key?: string;
	health: 'On track' | 'At risk' | 'Off track';
	healthTone: 'ok' | 'warn' | 'bad';
	statusPillClass: 't-green' | 't-amber' | 't-red';
	doneCount: number;
	totalCount: number;
	pct: number;
	dueLabel?: string;
	paceLabel?: string;
	paceTone?: 'ok' | 'warn' | 'bad';
	blockerText: string;
	blockerTone: 'ok' | 'warn' | 'bad' | 'sky';
	isClear: boolean;
	clearNote: string;
	actionLabel: string;
	actionHref: string;
	spotDecisionId?: string;
	workHref: string;
	leadName: string;
	memberInitials: string[];
	extraMembers: number;
}

export interface HomeMilestone {
	id: string;
	month: string;
	day: string;
	title: string;
	releaseTag?: string;
	note: string;
	statusLabel: string;
	tone: 'ok' | 'warn' | 'bad' | 'sky';
	spotId?: string;
	href: string;
}

export interface CatchUpRow {
	id: string;
	title: string;
	detail: string;
	timeLabel: string;
	tone: 'ok' | 'sky' | 'warn' | 'live';
	href: string;
}

export interface TaskTimeBreakdown {
	totalOpen: number;
	workingCount: number;
	reviewCount: number;
	waitingYouCount: number;
	queuedCount: number;
	workingPct: number;
	reviewPct: number;
	waitingYouPct: number;
	queuedPct: number;
	/** User rule: coral-rose when waiting-for-agent share exceeds 30%, purple otherwise. */
	queuedIsHigh: boolean;
	summary: string;
	primaryLink: { label: string; href: string };
	secondaryLink?: { label: string; href: string };
}

export interface OrgAiAnswer {
	query: string;
	headline: string;
	bullets: { text: string; href?: string; linkLabel?: string }[];
}

export interface ExecutiveBriefSection {
	id: string;
	kicker: string;
	tone: 'ok' | 'warn' | 'accent';
	body: string;
}

export interface ExecutiveBriefLensContent {
	lens: BriefLens;
	label: string;
	sections: ExecutiveBriefSection[];
	copyText: string;
	updatedLabel: string;
}

export interface ExecutiveBrief {
	sections: ExecutiveBriefSection[];
	copyText: string;
	updatedLabel: string;
	chiefOfStaff?: { id: string; name: string; role: string };
	lenses: Record<BriefLens, ExecutiveBriefLensContent>;
}

const INT_FORMAT = new Intl.NumberFormat('en-US');

/** Compact USD formatting using integer micro-USD arithmetic (never float drift). */
export function formatCompactUsd(microUsd: number | null): string {
	if (microUsd === null || !Number.isFinite(microUsd)) return '—';
	const abs = Math.abs(microUsd);
	const sign = microUsd < 0 ? '−' : '';
	if (abs === 0) return '$0';
	if (abs < 10_000) return `${sign}< $0.01`;
	// Round to whole dollars when exact or >= $100 with whole cents = 0, otherwise 2 decimals.
	const cents = Math.floor((abs + 5_000) / 10_000);
	const whole = Math.floor(cents / 100);
	const rem = cents % 100;
	if (rem === 0) return `${sign}$${INT_FORMAT.format(whole)}`;
	return `${sign}$${INT_FORMAT.format(whole)}.${String(rem).padStart(2, '0')}`;
}

export function greetingForHour(hour: number): string {
	if (hour < 12) return 'Good morning.';
	if (hour < 18) return 'Good afternoon.';
	return 'Good evening.';
}

function formatDueShort(isoDate: string): string {
	const parts = isoDate.split('-').map(Number);
	if (parts.length !== 3 || parts.some((n) => Number.isNaN(n))) return isoDate;
	const dt = new Date(Date.UTC(parts[0], parts[1] - 1, parts[2], 12));
	return new Intl.DateTimeFormat('en-US', {
		month: 'short',
		day: 'numeric',
		timeZone: 'UTC'
	}).format(dt);
}

export function isItemOpen(item: WorkItemSummary): boolean {
	if (item.deleted) return false;
	if (item.kind === 'task') {
		if (item.resolution === 'cancelled' || (item.status as string) === 'cancelled') return false;
		return item.status !== 'done';
	}
	if (item.kind === 'bug') return item.status !== 'closed';
	if (item.kind === 'request') {
		return !['approved', 'declined', 'merged'].includes(item.status);
	}
	return false;
}

export function isOfficeOnline(office: OfficeInfo): boolean {
	return office.status === 'active' || office.status === undefined;
}

export function deriveFinancialKpi(
	orgCost: OrganizationCost | undefined,
	agentBudgets: AgentBudgets | undefined,
	doneCount: number
): FinancialKpi {
	const available = Boolean(orgCost || agentBudgets);
	if (!available) {
		return {
			available: false,
			spentMicroUsd: null,
			spentLabel: '—',
			budgetMicroUsd: null,
			budgetLabel: 'Costs not connected',
			usagePct: 0,
			pillLabel: 'View costs',
			pillTone: 'muted',
			costPerTaskLabel: '—',
			budgetedAgentsCount: 0,
			recordsCount: 0
		};
	}

	const monthlyUsdBudgets = (agentBudgets?.agents ?? [])
		.filter((a) => !a.former && a.monthly?.unit === 'usd')
		.map((a) => ({ agent: a, monthly: a.monthly! }));

	const budgetMicroUsd =
		monthlyUsdBudgets.length > 0
			? monthlyUsdBudgets.reduce((acc, entry) => acc + entry.monthly.limit, 0)
			: null;

	const budgetSpentSum =
		monthlyUsdBudgets.length > 0
			? monthlyUsdBudgets.reduce((acc, entry) => acc + entry.monthly.spent, 0)
			: null;

	const spentMicroUsd = orgCost?.totals.knownCostMicroUsd ?? budgetSpentSum;

	const cappedEntry = (agentBudgets?.agents ?? []).find(
		(a) =>
			!a.former &&
			(a.monthly?.hold !== undefined ||
				a.daily?.hold !== undefined ||
				(a.monthly !== undefined && a.monthly.spent >= a.monthly.limit))
	);
	const cappedAgent = cappedEntry
		? { memberId: cappedEntry.memberId, name: cappedEntry.name }
		: undefined;

	const usagePct =
		spentMicroUsd !== null && budgetMicroUsd !== null && budgetMicroUsd > 0
			? Math.max(0, Math.min(100, Math.round((spentMicroUsd * 100) / budgetMicroUsd)))
			: 0;

	let pillLabel = 'On budget';
	let pillTone: SignalTone = 'ok';
	if (cappedAgent) {
		pillLabel = `${cappedAgent.name.split(/\s+/)[0]} at cap`;
		pillTone = 'warn';
	} else if (spentMicroUsd === null || spentMicroUsd === 0) {
		pillLabel = 'No spend yet';
		pillTone = 'ok';
	} else if (budgetMicroUsd !== null && usagePct >= 95) {
		pillLabel = `${usagePct}% of budget`;
		pillTone = 'bad';
	} else if (budgetMicroUsd !== null && usagePct >= 80) {
		pillLabel = `${usagePct}% of budget`;
		pillTone = 'warn';
	}

	const costPerTaskLabel =
		spentMicroUsd !== null && spentMicroUsd > 0 && doneCount > 0
			? formatCompactUsd(Math.round(spentMicroUsd / doneCount))
			: '—';

	const budgetedAgentsCount = (agentBudgets?.agents ?? []).filter(
		(a) => !a.former && (a.daily || a.monthly)
	).length;

	return {
		available: true,
		spentMicroUsd,
		spentLabel: formatCompactUsd(spentMicroUsd),
		budgetMicroUsd,
		budgetLabel:
			budgetMicroUsd !== null ? `/ ${formatCompactUsd(budgetMicroUsd)} monthly` : 'this month',
		usagePct,
		pillLabel,
		pillTone,
		cappedAgent,
		costPerTaskLabel,
		budgetedAgentsCount,
		recordsCount: orgCost?.totals.records ?? 0
	};
}

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
	const allMembers = nonOwners(snapshot).filter((m) => !isFormer(m));

	// Union of projects from projectsApi + snapshot so nothing is dropped when one source is ahead.
	const projectIds = new Set<string>([
		...rowById.keys(),
		...activeSnapshotProjects.map((p) => p.id)
	]);

	const riskRank: Record<ProjectBentoCard['health'], number> = {
		'Off track': 0,
		'At risk': 1,
		'On track': 2
	};

	const cards: ProjectBentoCard[] = [];
	for (const projectId of projectIds) {
		const snapProj = activeSnapshotProjects.find((p) => p.id === projectId);
		const row = rowById.get(projectId);
		const homeProj = homeById.get(projectId);
		const name = row?.name ?? snapProj?.name ?? homeProj?.name ?? projectId;
		const key = snapProj?.key ?? row?.slug;

		const projItems = items.filter((i) => !i.deleted && i.project_id === projectId);
		const workDone = projItems.filter(isItemDone).length;
		const workTotal = projItems.length;

		const rowTotal = row
			? row.tasks.todo + row.tasks.inProgress + row.tasks.blocked + row.tasks.done
			: 0;
		const doneCount =
			homeProj && homeProj.total > 0
				? homeProj.done
				: row && rowTotal > 0
					? row.tasks.done
					: workDone;
		const totalCount =
			homeProj && homeProj.total > 0 ? homeProj.total : row && rowTotal > 0 ? rowTotal : workTotal;
		const pct = totalCount > 0 ? Math.round((doneCount * 100) / totalCount) : 0;

		const blockedItems = projItems.filter((i) => i.status === 'blocked');
		const blockedCount = Math.max(
			blockedItems.length,
			row?.tasks.blocked ?? 0,
			homeProj?.blocked ?? 0
		);
		const criticalBugs = Math.max(
			projItems.filter(
				(i) =>
					i.kind === 'bug' &&
					i.status !== 'closed' &&
					(i.severity === 'critical' || i.severity === 'major')
			).length,
			row ? row.bugs.critical + row.bugs.major : 0
		);
		const scopedReqs = projItems.filter((i) => i.kind === 'request' && i.status === 'scoped');
		const projInbox = openInbox.filter((i) => i.project?.id === projectId);
		const projProblems = projInbox.filter((i) => i.kind === 'problem');
		const projApprovals = projInbox.filter((i) => i.kind === 'approval' || i.kind === 'decision');

		const teams = teamsOfProject(snapshot, projectId);
		const teamIds = new Set(teams.map((t) => t.id));
		if (row) {
			teamIds.add(row.ownerTeam.id);
			for (const c of row.contributors) teamIds.add(c.id);
		}
		const projMembers = allMembers.filter(
			(m) => Boolean(m.home_team_id) && teamIds.has(m.home_team_id!)
		);
		const lead = homeProj?.lead ?? teams.map((t) => leadOf(snapshot, t)).find(Boolean);

		const nowDate = new Date(nowMs);
		const todayUtc = Date.UTC(
			nowDate.getUTCFullYear(),
			nowDate.getUTCMonth(),
			nowDate.getUTCDate(),
			12
		);
		const dueDt = homeProj?.due ? parseIsoDateUtc(homeProj.due) : null;
		const remainingCount = Math.max(0, totalCount - doneCount);
		const isOverdue = Boolean(
			dueDt && dueDt.getTime() < todayUtc && totalCount > 0 && doneCount < totalCount
		);

		let health: ProjectBentoCard['health'] = 'On track';
		let healthTone: ProjectBentoCard['healthTone'] = 'ok';
		let statusPillClass: ProjectBentoCard['statusPillClass'] = 't-green';
		if (
			criticalBugs > 0 ||
			blockedCount >= 2 ||
			(projProblems.length > 0 && blockedCount > 0) ||
			(row && row.bugs.critical > 0)
		) {
			health = 'Off track';
			healthTone = 'bad';
			statusPillClass = 't-red';
		} else if (
			isOverdue ||
			blockedCount > 0 ||
			scopedReqs.length > 0 ||
			projInbox.length >= 2 ||
			(teams.length === 0 && !row)
		) {
			health = 'At risk';
			healthTone = 'warn';
			statusPillClass = 't-amber';
		}

		const workHref = `/projects/${projectId}/work`;
		let blockerText = '';
		let blockerTone: ProjectBentoCard['blockerTone'] = 'ok';
		let actionLabel = 'Open work';
		let actionHref = workHref;
		let spotDecisionId: string | undefined;
		const clearNote = isOverdue
			? remainingCount > 0
				? `Past due date · ${remainingCount} open`
				: 'Past due date · review remaining work'
			: 'All checks passing · on schedule';

		if (projProblems.length > 0) {
			blockerText = projProblems[0].title;
			blockerTone = 'bad';
			actionLabel = 'Inspect';
			actionHref = `/inbox?item=${encodeURIComponent(projProblems[0].id)}`;
			spotDecisionId = projProblems[0].id;
		} else if (projApprovals.length > 0 && healthTone === 'bad') {
			blockerText = blockedItems[0]?.blocked_reason || projApprovals[0].title;
			blockerTone = 'bad';
			actionLabel = 'Unblock';
			actionHref = `/inbox?item=${encodeURIComponent(projApprovals[0].id)}`;
			spotDecisionId = projApprovals[0].id;
		} else if (criticalBugs > 0) {
			blockerText = `${criticalBugs} ${criticalBugs === 1 ? 'serious bug' : 'serious bugs'} open`;
			blockerTone = 'bad';
			actionLabel = 'Inspect';
			actionHref = workHref;
		} else if (projApprovals.length > 0) {
			blockerText =
				projApprovals.length === 1
					? projApprovals[0].title
					: `${projApprovals.length} sign-offs waiting in Inbox`;
			blockerTone = healthTone === 'warn' ? 'warn' : 'sky';
			actionLabel =
				projApprovals.length > 1
					? `Review (${projApprovals.length})`
					: healthTone === 'ok'
						? 'Sign off'
						: 'Review';
			actionHref = `/inbox?item=${encodeURIComponent(projApprovals[0].id)}`;
			spotDecisionId = projApprovals[0].id;
		} else if (blockedCount > 0) {
			blockerText =
				blockedItems[0]?.blocked_reason ||
				`${blockedCount} ${blockedCount === 1 ? 'task' : 'tasks'} blocked`;
			blockerTone = 'warn';
			actionLabel = 'Unblock';
			actionHref = '/inbox';
			if (blockedItems[0]) spotDecisionId = blockedItems[0].id;
		} else if (scopedReqs.length > 0) {
			blockerText = `${scopedReqs.length} scoped ${scopedReqs.length === 1 ? 'request' : 'requests'} waiting sign-off`;
			blockerTone = 'sky';
			actionLabel = 'Review';
			actionHref = '/inbox';
			spotDecisionId = scopedReqs[0].id;
		} else if (teams.length === 0 && !row) {
			blockerText = 'No team assigned yet';
			blockerTone = 'warn';
			actionLabel = 'Assign';
			actionHref = `/projects/${projectId}/team`;
		}

		const isClear = blockerText === '' && !isOverdue;
		if (blockerText === '') {
			blockerText = clearNote;
			if (isOverdue) {
				blockerTone = 'bad';
			}
		}

		const paceLabel = isOverdue
			? 'Overdue'
			: health === 'Off track'
				? 'Behind pace'
				: health === 'At risk'
					? 'At risk'
					: pct >= 50
						? 'On pace'
						: undefined;
		const paceTone: ProjectBentoCard['paceTone'] = isOverdue
			? 'bad'
			: health === 'Off track'
				? 'bad'
				: health === 'At risk'
					? 'warn'
					: 'ok';

		const shown = projMembers.slice(0, 4).map((m) => initialsOf(m.name));
		const extraMembers = Math.max(0, (homeProj?.members ?? projMembers.length) - shown.length);

		cards.push({
			id: projectId,
			name,
			key,
			health,
			healthTone,
			statusPillClass,
			doneCount,
			totalCount,
			pct,
			dueLabel: homeProj?.due ? formatDueShort(homeProj.due) : undefined,
			paceLabel,
			paceTone,
			blockerText,
			blockerTone,
			isClear,
			clearNote,
			actionLabel,
			actionHref,
			spotDecisionId,
			workHref,
			leadName:
				lead?.name ??
				row?.ownerTeam.name ??
				(teams[0]?.name ? `${teams[0].name} team` : 'Unassigned'),
			memberInitials: shown,
			extraMembers
		});
	}

	return cards.sort(
		(a, b) => riskRank[a.health] - riskRank[b.health] || a.name.localeCompare(b.name)
	);
}

function deriveTeamHeadcount(
	snapshot: OrgSnapshot,
	offices: readonly OfficeInfo[],
	directory: HomeDomainData['memberRows']
): {
	activeAgents: number;
	pausedAgents: number;
	unplacedAgents: number;
	totalAgents: number;
	peopleCount: number;
} {
	if (directory && directory.length > 0) {
		const activeRows = directory.filter((m) => m.status !== 'inactive');
		const agentRows = activeRows.filter((m) => m.kind === 'agent');
		const personRows = activeRows.filter((m) => m.kind === 'person' && !m.owner);
		const hasPresenceTelemetry = agentRows.some((a) => a.presence !== undefined);
		const totalAgents = agentRows.length;
		const activeAgents = agentRows.filter(
			(a) =>
				a.status === 'active' &&
				a.office !== null &&
				(!hasPresenceTelemetry || a.presence?.id === 'working' || a.presence?.id === 'with_you')
		).length;
		const unplacedAgents = agentRows.filter((a) => a.office === null).length;
		return {
			activeAgents,
			pausedAgents: Math.max(0, totalAgents - activeAgents - unplacedAgents),
			unplacedAgents,
			totalAgents,
			peopleCount: personRows.length
		};
	}

	const members = nonOwners(snapshot).filter((m) => !isFormer(m));
	const agents = agentsOf(members);
	const knownOffices = new Set(offices.map((o) => o.id));
	return {
		activeAgents: agents.filter((a) => a.agent?.status === 'active').length,
		pausedAgents: agents.filter((a) => a.agent?.status === 'paused').length,
		unplacedAgents: agents.filter(
			(a) => !a.office_id || (knownOffices.size > 0 && !knownOffices.has(a.office_id))
		).length,
		totalAgents: agents.length,
		peopleCount: peopleOf(members).length
	};
}

export function deriveKpiMetrics(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[],
	offices: readonly OfficeInfo[],
	ownerId: string,
	domain?: HomeDomainData,
	nowMs = Date.now()
): ExecutiveKpis {
	const liveItems = items.filter((i) => !i.deleted);
	const openItems = liveItems.filter(isItemOpen);
	const doneItems = liveItems.filter(isItemDone);
	const projectRows = (domain?.projectRows ?? []).filter((p) => !p.archived);
	const rowOpen = projectRows.reduce(
		(s, p) => s + p.tasks.todo + p.tasks.inProgress + p.tasks.blocked,
		0
	);
	const rowDone = projectRows.reduce((s, p) => s + p.tasks.done, 0);
	const rowBlocked = projectRows.reduce((s, p) => s + p.tasks.blocked, 0);

	const openCount = openItems.length > 0 ? openItems.length : rowOpen;
	const doneCount = doneItems.length > 0 ? doneItems.length : rowDone;
	const blockedCount = Math.max(openItems.filter((i) => i.status === 'blocked').length, rowBlocked);

	const openInboxCount = (domain?.inboxItems ?? []).filter((i) => i.state === 'open').length;
	const workUrgentCount = needsYou(liveItems, ownerId);
	const urgentCount = openInboxCount > 0 ? openInboxCount : workUrgentCount;

	const totalTracked = openCount + doneCount;
	const completionPct = totalTracked > 0 ? Math.round((doneCount * 100) / totalTracked) : 0;

	const { activeAgents, pausedAgents, unplacedAgents, totalAgents, peopleCount } =
		deriveTeamHeadcount(snapshot, offices, domain?.memberRows);

	const autonomyPct =
		openCount > 0
			? Math.max(
					0,
					Math.min(
						100,
						Math.round(((openCount - Math.min(openCount, urgentCount)) * 100) / openCount)
					)
				)
			: activeAgents > 0
				? 100
				: 0;

	const onlineOffices = offices.filter(isOfficeOnline).length;
	const offlineOfficeList = offices.filter((o) => !isOfficeOnline(o));
	const offlineOffices = offlineOfficeList.length;
	const offlineOfficeNames = offlineOfficeList.map((o) => o.name);

	const officePillLabel =
		offlineOffices > 0
			? `${offlineOffices} ${offlineOffices === 1 ? 'office' : 'offices'} offline`
			: offices.length === 1
				? 'HQ online'
				: offices.length > 1
					? 'All offices online'
					: 'No offices';
	const officePillTone: SignalTone =
		offlineOffices > 0 ? 'bad' : offices.length > 0 ? 'ok' : 'muted';

	const projectCards = deriveProjectCards(snapshot, liveItems, domain, nowMs);
	const offTrackProjects = projectCards.filter((p) => p.health === 'Off track').length;
	const atRiskProjects = projectCards.filter((p) => p.health === 'At risk').length;
	const projectsAttention = offTrackProjects + atRiskProjects;

	const leadIds = new Set(
		snapshot.teams.map((t) => t.lead_member_id).filter((id): id is string => Boolean(id))
	);
	const allActiveAgents = agentsOf(nonOwners(snapshot).filter((m) => !isFormer(m)));
	const leadsCount = allActiveAgents.filter(
		(a) =>
			leadIds.has(a.id) ||
			a.agent?.role === 'chief_of_staff' ||
			a.agent?.role === 'engineering_manager'
	).length;
	const specialistsCount = Math.max(0, totalAgents - leadsCount);

	const workOpenBugs = openItems.filter((i) => i.kind === 'bug').length;
	const rowOpenBugs = projectRows.reduce(
		(s, p) => s + p.bugs.critical + p.bugs.major + p.bugs.minor + p.bugs.trivial + p.bugs.unset,
		0
	);
	const openBugs = Math.max(workOpenBugs, rowOpenBugs);

	const workCriticalBugs = openItems.filter(
		(i) => i.kind === 'bug' && (i.severity === 'critical' || i.severity === 'major')
	).length;
	const rowCriticalBugs = projectRows.reduce((s, p) => s + p.bugs.critical + p.bugs.major, 0);
	const criticalBugs = Math.max(workCriticalBugs, rowCriticalBugs);

	const reopened = liveItems.filter((i) => i.reopen_count > 0).length;
	const firstTryPassPct =
		doneCount > 0
			? Math.max(0, Math.round(((doneCount - Math.min(doneCount, reopened)) * 100) / doneCount))
			: totalTracked > 0
				? 100
				: 0;

	const financials = deriveFinancialKpi(domain?.orgCost, domain?.agentBudgets, doneCount);
	const budgetRemainingPct =
		financials.available && financials.budgetMicroUsd !== null
			? Math.max(0, Math.min(100, 100 - financials.usagePct))
			: null;

	return {
		financials,
		openCount,
		doneCount,
		blockedCount,
		urgentCount,
		completionPct,
		activeAgents,
		pausedAgents,
		unplacedAgents,
		totalAgents,
		leadsCount,
		specialistsCount,
		budgetRemainingPct,
		peopleCount,
		autonomyPct,
		onlineOffices,
		offlineOffices,
		offlineOfficeNames,
		officePillLabel,
		officePillTone,
		projectsTotal: projectCards.length,
		projectsAttention,
		offTrackProjects,
		atRiskProjects,
		openBugs,
		criticalBugs,
		firstTryPassPct
	};
}

function parseIsoDateUtc(isoDate: string): Date | null {
	const parts = isoDate.split('-').map(Number);
	if (parts.length !== 3 || parts.some((n) => Number.isNaN(n))) return null;
	return new Date(Date.UTC(parts[0], parts[1] - 1, parts[2], 12));
}

export function deriveUpcomingMilestones(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[],
	nowMs = Date.now(),
	domain?: Pick<HomeDomainData, 'projectRows' | 'homeProjects' | 'inboxItems'>
): HomeMilestone[] {
	const homeProjects = domain?.homeProjects ?? [];
	const dated = homeProjects.filter((p): p is HomeProject & { due: string } => Boolean(p.due));
	if (dated.length === 0) return [];

	const cardsById = new Map(
		deriveProjectCards(snapshot, items, domain, nowMs).map((card) => [card.id, card])
	);
	const monthFmt = new Intl.DateTimeFormat('en-US', { month: 'short', timeZone: 'UTC' });
	const nowDate = new Date(nowMs);
	const todayUtc = Date.UTC(
		nowDate.getUTCFullYear(),
		nowDate.getUTCMonth(),
		nowDate.getUTCDate(),
		12
	);

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
			const month = monthFmt.format(dt);
			const day = String(dt.getUTCDate()).padStart(2, '0');
			const statusLabel =
				diffDays === 0 ? 'Today' : diffDays === 1 ? 'Tomorrow' : `In ${diffDays} days`;

			const card = cardsById.get(hp.id);
			const tone: HomeMilestone['tone'] =
				card?.healthTone === 'bad'
					? 'bad'
					: card?.healthTone === 'warn'
						? 'warn'
						: card?.spotDecisionId
							? 'sky'
							: 'ok';
			const leadLabel = hp.lead?.name ?? card?.leadName ?? 'Team';
			const releaseTag = card?.key;
			const progressNote = hp.total > 0 ? `${hp.done}/${hp.total} done` : 'Scheduled';

			return {
				id: `ms-${hp.id}`,
				month,
				day,
				title: hp.name,
				releaseTag,
				note: `${leadLabel} · ${progressNote}`,
				statusLabel,
				tone,
				spotId: card?.spotDecisionId,
				href: card?.actionHref ?? `/projects/${hp.id}/work`
			};
		});
}

export function deriveTaskTimeBreakdown(
	items: readonly WorkItemSummary[],
	ownerId: string,
	offlineOffices = 0,
	domain?: Pick<HomeDomainData, 'inboxItems' | 'memberRows' | 'projectRows'>
): TaskTimeBreakdown {
	const open = items.filter(isItemOpen);
	const openInboxCount = (domain?.inboxItems ?? []).filter((i) => i.state === 'open').length;
	const projectRows = (domain?.projectRows ?? []).filter((p) => !p.archived);
	const rowWorking = projectRows.reduce((s, p) => s + p.tasks.inProgress, 0);
	const rowBlocked = projectRows.reduce((s, p) => s + p.tasks.blocked, 0);
	const rowTodo = projectRows.reduce((s, p) => s + p.tasks.todo, 0);
	const totalOpen = open.length;
	if (totalOpen === 0 && openInboxCount === 0 && rowWorking + rowBlocked + rowTodo === 0) {
		return {
			totalOpen: 0,
			workingCount: 0,
			reviewCount: 0,
			waitingYouCount: 0,
			queuedCount: 0,
			workingPct: 0,
			reviewPct: 0,
			waitingYouPct: 0,
			queuedPct: 0,
			queuedIsHigh: false,
			summary: 'No open tasks in the queue right now.',
			primaryLink: { label: 'Open Work', href: '/work' }
		};
	}

	let workingCount = 0;
	let reviewCount = 0;
	let waitingYouCount = 0;
	let queuedCount = 0;

	if (totalOpen > 0) {
		for (const item of open) {
			const needsOwner =
				(item.kind === 'request' && item.status === 'scoped') ||
				(item.kind === 'bug' && (item.status === 'new' || item.status === 'fixed')) ||
				(item.kind === 'task' && item.status === 'blocked' && item.assignee_id === ownerId);

			if (needsOwner) {
				waitingYouCount += 1;
			} else if (item.status === 'in_progress' || item.status === 'scoping') {
				workingCount += 1;
			} else if (item.status === 'triaged' || item.status === 'routed') {
				reviewCount += 1;
			} else {
				queuedCount += 1;
			}
		}
	} else {
		workingCount = rowWorking;
		waitingYouCount = Math.max(openInboxCount, rowBlocked);
		queuedCount = rowTodo;
	}
	const denom = Math.max(1, workingCount + reviewCount + waitingYouCount + queuedCount);
	const workingPct = Math.round((workingCount * 100) / denom);
	const reviewPct = Math.round((reviewCount * 100) / denom);
	const waitingYouPct = Math.round((waitingYouCount * 100) / denom);
	const queuedPct = Math.max(0, 100 - workingPct - reviewPct - waitingYouPct);
	// Explicit user threshold: >30% waiting for an agent uses coral-rose; <=30% uses purple.
	const queuedIsHigh = queuedPct > 30;

	const idleAgents = (domain?.memberRows ?? []).filter(
		(m) => m.kind === 'agent' && m.status !== 'inactive' && m.presence?.id !== 'working'
	).length;

	let summary = 'Work is flowing smoothly across active agents and your decisions.';
	let primaryLink = { label: 'Open Work', href: '/work' };
	let secondaryLink: { label: string; href: string } | undefined;

	if (queuedIsHigh) {
		summary = `${queuedPct}% of open work is waiting for an agent.${offlineOffices > 0 ? ' Reconnecting offline offices or activating idle agents will speed up delivery.' : idleAgents > 0 ? ` Assigning ${idleAgents} idle specialists will clear the backlog faster.` : ' Assigning specialists will clear the backlog faster.'}`;
		primaryLink =
			offlineOffices > 0
				? { label: 'Check offices', href: '/settings/offices' }
				: { label: 'Open Work queue', href: '/work' };
		secondaryLink = { label: 'Review members', href: '/teams/members' };
	} else if (waitingYouPct >= 25) {
		summary = `${waitingYouPct}% of open work is waiting on your review. Clearing the Decision Spotlight will unblock downstream tasks.`;
		primaryLink = { label: 'Open Inbox', href: '/inbox' };
		secondaryLink = { label: 'Open Work', href: '/work' };
	}

	return {
		totalOpen: denom,
		workingCount,
		reviewCount,
		waitingYouCount,
		queuedCount,
		workingPct,
		reviewPct,
		waitingYouPct,
		queuedPct,
		queuedIsHigh,
		summary,
		primaryLink,
		secondaryLink
	};
}

export function deriveCatchUpDigest(
	items: readonly WorkItemSummary[],
	snapshot: OrgSnapshot,
	nowMs = Date.now(),
	activityRows?: readonly ActivityRow[]
): CatchUpRow[] {
	if (activityRows && activityRows.length > 0) {
		const top = activityRows.slice(0, 3).map((row): CatchUpRow => ({
			id: row.id,
			title: `${row.actor.name} ${row.text}`,
			detail: row.preview ?? row.run?.outcome ?? row.run?.trigger ?? 'Recorded in Activity',
			timeLabel: relativeAge(row.at, nowMs),
			tone:
				row.action === 'task-done' || row.action === 'merge' || row.run?.state === 'succeeded'
					? 'ok'
					: row.run?.state === 'failed' || row.kind === 'decision'
						? 'warn'
						: 'sky',
			href: row.href?.path ?? '/activity'
		}));
		return top;
	}

	const recent = [...items]
		.filter((i) => !i.deleted)
		.sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at))
		.slice(0, 3);

	const rows: CatchUpRow[] = recent.map((item) => {
		const actor = memberById(snapshot, item.assignee_id) ?? memberById(snapshot, item.created_by);
		const actorName = actor?.name ?? 'Team';
		const done = isItemDone(item);
		const blocked = item.status === 'blocked';
		return {
			id: item.id,
			title: item.title,
			detail: done
				? `${actorName} completed ${item.short_id}.`
				: blocked
					? `${actorName} flagged ${item.short_id} as blocked.`
					: `${actorName} updated ${item.short_id} (${item.status.replace('_', ' ')}).`,
			timeLabel: relativeAge(item.updated_at, nowMs),
			tone: done ? 'ok' : blocked ? 'warn' : 'sky',
			href:
				item.kind === 'request'
					? `/work?request=${encodeURIComponent(item.id)}`
					: `/work/${encodeURIComponent(item.short_id)}`
		};
	});

	const activeAgents = agentsOf(nonOwners(snapshot).filter((m) => !isFormer(m))).filter(
		(a) => a.agent?.status === 'active'
	);
	rows.push({
		id: 'live-agents',
		title:
			activeAgents.length > 0
				? `${activeAgents.length} ${activeAgents.length === 1 ? 'agent' : 'agents'} active now`
				: 'Ready for your first agent',
		detail:
			activeAgents.length > 0
				? `${activeAgents
						.slice(0, 3)
						.map((a) => a.name)
						.join(
							', '
						)}${activeAgents.length > 3 ? ` +${activeAgents.length - 3} more` : ''} standing by across ${(snapshot.projects ?? []).length} projects.`
				: 'Hire or place an agent in Offices to start autonomous delivery.',
		timeLabel: 'Live',
		tone: 'live',
		href: '/teams/members'
	});

	return rows;
}

export function deriveExecutiveBrief(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[],
	offices: readonly OfficeInfo[],
	ownerId: string,
	organizationName = 'Organization',
	nowMs = Date.now(),
	domain?: HomeDomainData
): ExecutiveBrief {
	const kpis = deriveKpiMetrics(snapshot, items, offices, ownerId, domain);
	const spotlight = deriveDecisionSpotlight(items, snapshot, ownerId, nowMs, domain?.inboxItems);
	const projects = deriveProjectCards(snapshot, items, domain);
	const attentionProjects = projects.filter((p) => p.health !== 'On track');
	const recentActivity = domain?.activityRows ?? [];

	const spendPhrase = kpis.financials.available
		? kpis.financials.budgetMicroUsd !== null
			? `Month-to-date spend is ${kpis.financials.spentLabel} of ${formatCompactUsd(kpis.financials.budgetMicroUsd)} (${kpis.financials.usagePct}%).`
			: `Month-to-date spend is ${kpis.financials.spentLabel}.`
		: 'Cost reporting will appear once the billing ledger connects.';

	const latestEventPhrase =
		recentActivity.length > 0
			? ` Latest: ${recentActivity[0].actor.name} ${recentActivity[0].text}.`
			: '';

	const liveBody =
		kpis.totalAgents > 0
			? `${kpis.activeAgents} of ${kpis.totalAgents} agents are active across ${kpis.projectsTotal} ${kpis.projectsTotal === 1 ? 'project' : 'projects'} (${kpis.autonomyPct}% autonomous execution). ${kpis.doneCount} ${kpis.doneCount === 1 ? 'item' : 'items'} completed with ${kpis.completionPct}% overall progress and ${kpis.firstTryPassPct}% clean delivery.${latestEventPhrase}`
			: `${kpis.openCount} open ${kpis.openCount === 1 ? 'item' : 'items'} and ${kpis.doneCount} completed across ${kpis.projectsTotal} ${kpis.projectsTotal === 1 ? 'project' : 'projects'}. Add or activate agents to increase autonomous delivery.`;

	const riskParts: string[] = [];
	if (kpis.offlineOffices > 0) {
		riskParts.push(
			`${kpis.offlineOffices} ${kpis.offlineOffices === 1 ? 'office is' : 'offices are'} offline (${kpis.offlineOfficeNames.join(', ')}).`
		);
	}
	if (kpis.financials.cappedAgent) {
		riskParts.push(`${kpis.financials.cappedAgent.name} reached its monthly budget cap.`);
	}
	if (kpis.blockedCount > 0) {
		riskParts.push(
			`${kpis.blockedCount} ${kpis.blockedCount === 1 ? 'item is' : 'items are'} blocked.`
		);
	}
	if (attentionProjects.length > 0) {
		riskParts.push(
			`${attentionProjects.length} ${attentionProjects.length === 1 ? 'project needs' : 'projects need'} attention: ${attentionProjects
				.slice(0, 2)
				.map((p) => `${p.name} (${p.health})`)
				.join(', ')}.`
		);
	}
	const problemsBody =
		riskParts.length > 0
			? riskParts.join(' ')
			: 'All connected offices are online, no work items are blocked, and every active project is tracking cleanly.';

	const inboxState = domain?.inboxState ?? 'ready';
	const inboxLoading = inboxState === 'loading' && spotlight.length === 0;
	const inboxUnavailable = inboxState === 'unavailable' && spotlight.length === 0;
	const queueBody =
		spotlight.length > 0
			? `${spotlight.length} ${spotlight.length === 1 ? 'decision is' : 'decisions are'} queued for your review, led by “${spotlight[0].title}” (${spotlight[0].kindLabel} · ${spotlight[0].projectName}).`
			: inboxLoading
				? 'Checking Inbox for pending decisions…'
				: inboxUnavailable
					? 'Inbox status unavailable — check Inbox directly for pending decisions.'
					: 'Your decision queue is clear — nothing is waiting on owner sign-off right now.';

	const todaySections: ExecutiveBriefSection[] = [
		{ id: 'live', kicker: 'Live Status & Highlights', tone: 'ok', body: liveBody },
		{
			id: 'problems',
			kicker: 'Problems Needing Attention',
			tone: riskParts.length > 0 ? 'warn' : 'ok',
			body: problemsBody
		},
		{
			id: 'queue',
			kicker: 'Queued For You',
			tone: inboxUnavailable ? 'warn' : 'accent',
			body: queueBody
		}
	];

	const weekSections: ExecutiveBriefSection[] = [
		{
			id: 'velocity',
			kicker: 'Velocity & Delivery',
			tone: 'ok',
			body: `${kpis.doneCount} completed and ${kpis.openCount} open items across ${kpis.projectsTotal} active ${kpis.projectsTotal === 1 ? 'project' : 'projects'} (${kpis.completionPct}% complete).${projects.find((p) => p.health === 'On track') ? ` ${projects.find((p) => p.health === 'On track')!.name} is tracking cleanly.` : ''}`
		},
		{
			id: 'autonomy',
			kicker: 'Autonomy & Budget',
			tone: kpis.financials.cappedAgent ? 'warn' : 'accent',
			body: `${kpis.autonomyPct}% of open work is running autonomously across ${kpis.totalAgents} agents (${kpis.activeAgents} active, ${kpis.pausedAgents} idle). ${spendPhrase}`
		},
		{
			id: 'quality',
			kicker: 'Quality Bar',
			tone: kpis.criticalBugs > 0 ? 'warn' : 'ok',
			body: `${kpis.firstTryPassPct}% clean first-try delivery with ${kpis.openBugs} open ${kpis.openBugs === 1 ? 'bug' : 'bugs'}${kpis.criticalBugs > 0 ? ` (${kpis.criticalBugs} serious)` : ''}.`
		}
	];

	const boardSections: ExecutiveBriefSection[] = [
		{
			id: 'summary',
			kicker: 'Executive Summary',
			tone: 'ok',
			body: `${organizationName} is at ${kpis.completionPct}% portfolio completion (${kpis.doneCount} delivered, ${kpis.firstTryPassPct}% clean delivery) with ${kpis.autonomyPct}% autonomous execution. ${spendPhrase}`
		},
		{
			id: 'readiness',
			kicker: 'Delivery Readiness',
			tone: attentionProjects.length > 0 ? 'accent' : 'ok',
			body:
				projects.length > 0
					? `${projects.length - attentionProjects.length} of ${projects.length} active projects are on track; ${
							inboxLoading
								? 'Checking Inbox for pending decisions…'
								: inboxUnavailable
									? 'Inbox status unavailable — check Inbox directly for pending decisions.'
									: `${spotlight.length} owner ${spotlight.length === 1 ? 'sign-off remains' : 'sign-offs remain'} in the decision queue.`
						}`
					: 'No active projects registered yet.'
		},
		{
			id: 'watchpoints',
			kicker: 'Operational Watchpoints',
			tone: riskParts.length > 0 ? 'warn' : 'ok',
			body: problemsBody
		}
	];

	const buildLens = (
		lens: BriefLens,
		label: string,
		sections: ExecutiveBriefSection[],
		updatedLabel: string
	): ExecutiveBriefLensContent => ({
		lens,
		label,
		sections,
		updatedLabel,
		copyText: [
			`Executive Brief (${label}) — ${organizationName}`,
			...sections.map((s) => `• ${s.kicker}: ${s.body}`)
		].join('\n')
	});

	const lenses: Record<BriefLens, ExecutiveBriefLensContent> = {
		today: buildLens('today', 'Today', todaySections, 'Live summary'),
		week: buildLens('week', '7-Day', weekSections, '7-day rollup'),
		board: buildLens('board', 'Board', boardSections, 'Board ready')
	};

	const cosMember = snapshot.members.find(
		(m) => !isFormer(m) && m.agent?.role === 'chief_of_staff'
	);

	return {
		sections: lenses.today.sections,
		copyText: lenses.today.copyText,
		updatedLabel: lenses.today.updatedLabel,
		chiefOfStaff: cosMember
			? {
					id: cosMember.id,
					name: cosMember.name,
					role: cosMember.agent?.title ?? 'Chief of Staff'
				}
			: undefined,
		lenses
	};
}

export interface DecisionAiContext {
	id: string;
	title: string;
	recommendation: string;
	risk: { label: string; sub: string };
	impact: { label: string; sub: string };
	backgroundContext: string;
	detailHref: string;
	projectId?: string;
	projectName: string;
}

function formatSpendPhrase(kpis: ExecutiveKpis): string | null {
	if (!kpis.financials.available) return null;
	const capDetail =
		kpis.budgetRemainingPct !== null ? `${kpis.financials.usagePct}%` : 'no monthly cap set';
	return `${kpis.financials.spentLabel} ${kpis.financials.budgetLabel} (${capDetail})`;
}

function answerBudgetAndTeam(query: string, kpis: ExecutiveKpis): OrgAiAnswer {
	const spendPhrase = formatSpendPhrase(kpis);
	const teamHeadline = `${kpis.activeAgents} of ${kpis.totalAgents} agents active · ${kpis.onlineOffices} online, ${kpis.offlineOffices} offline offices`;
	return {
		query,
		headline: spendPhrase ? `${teamHeadline} · spend ${spendPhrase}.` : `${teamHeadline}.`,
		bullets: [
			{
				text: spendPhrase
					? `Monthly spend is ${spendPhrase}. Average per completed item: ${kpis.financials.costPerTaskLabel}.`
					: `Cost metrics are not connected on this server yet (average per completed item: ${kpis.financials.costPerTaskLabel}).`,
				href: '/settings/costs',
				linkLabel: 'Open Costs'
			},
			...(kpis.financials.cappedAgent
				? [
						{
							text: `${kpis.financials.cappedAgent.name} reached its budget cap.`,
							href: '/settings/costs',
							linkLabel: 'Review budgets'
						}
					]
				: []),
			{
				text: `${kpis.activeAgents} active, ${kpis.pausedAgents} idle, ${kpis.unplacedAgents} unplaced across ${kpis.peopleCount} people.`,
				href: '/teams/members',
				linkLabel: 'Open Members'
			},
			{
				text:
					kpis.offlineOffices > 0
						? `Offline offices: ${kpis.offlineOfficeNames.join(', ')}.`
						: 'All connected offices are online.',
				href: '/settings/offices',
				linkLabel: 'Check Offices'
			}
		]
	};
}

function answerBudgetOnly(query: string, kpis: ExecutiveKpis): OrgAiAnswer {
	const spendPhrase = formatSpendPhrase(kpis);
	return {
		query,
		headline: spendPhrase
			? `Monthly spend is ${spendPhrase}.`
			: 'Cost metrics are not connected on this server yet.',
		bullets: [
			...(kpis.financials.cappedAgent
				? [
						{
							text: `${kpis.financials.cappedAgent.name} reached its budget cap.`,
							href: '/settings/costs',
							linkLabel: 'Review budgets'
						}
					]
				: []),
			{
				text: `Average cost per completed item: ${kpis.financials.costPerTaskLabel}.`,
				href: '/settings/costs',
				linkLabel: 'Open Costs'
			}
		]
	};
}

function answerUrgentInbox(query: string, spotlight: readonly SpotlightDecision[]): OrgAiAnswer {
	return {
		query,
		headline:
			spotlight.length > 0
				? `${spotlight.length} ${spotlight.length === 1 ? 'decision needs' : 'decisions need'} your attention right now.`
				: 'All clear — nothing is waiting on your decision.',
		bullets:
			spotlight.length > 0
				? spotlight.slice(0, 3).map((d) => ({
						text: `${d.title} (${d.kindLabel} · ${d.projectName})`,
						href: '/inbox',
						linkLabel: 'Review in Inbox'
					}))
				: [
						{
							text: 'Your team is running autonomously.',
							href: '/work',
							linkLabel: 'View all work'
						}
					]
	};
}

function answerProjectsHealth(query: string, projects: readonly ProjectBentoCard[]): OrgAiAnswer {
	const attn = projects.filter((p) => p.health !== 'On track');
	return {
		query,
		headline:
			attn.length > 0
				? `${attn.length} of ${projects.length} projects need attention.`
				: `All ${projects.length} active projects are on track.`,
		bullets: (attn.length > 0 ? attn : projects).slice(0, 4).map((p) => ({
			text: `${p.name} — ${p.health} (${p.pct}% done): ${p.blockerText}`,
			href: p.workHref,
			linkLabel: 'Open project'
		}))
	};
}

function answerTeamAndOffices(query: string, kpis: ExecutiveKpis): OrgAiAnswer {
	return {
		query,
		headline: `${kpis.activeAgents} of ${kpis.totalAgents} agents active · ${kpis.onlineOffices} online, ${kpis.offlineOffices} offline offices.`,
		bullets: [
			{
				text: `${kpis.activeAgents} active, ${kpis.pausedAgents} idle, ${kpis.unplacedAgents} unplaced across ${kpis.peopleCount} people.`,
				href: '/teams/members',
				linkLabel: 'Open Members'
			},
			{
				text:
					kpis.offlineOffices > 0
						? `Offline offices: ${kpis.offlineOfficeNames.join(', ')}.`
						: 'All connected offices are online.',
				href: '/settings/offices',
				linkLabel: 'Check Offices'
			}
		]
	};
}

function findMatchingDecision(
	query: string,
	spotlight: readonly SpotlightDecision[],
	decisionId?: string
): SpotlightDecision | undefined {
	if (decisionId) {
		const byId = spotlight.find((d) => d.id === decisionId);
		if (byId) return byId;
	}
	const explainMatch = query.match(/^explain the risk, impact, and recommendation for:\s*(.+)$/i);
	if (!explainMatch) return undefined;
	const targetTitle = explainMatch[1].trim().toLowerCase();
	return spotlight.find((d) => d.title.toLowerCase() === targetTitle);
}

export function answerExecutiveQuestion(
	rawQuery: string,
	{
		snapshot,
		items,
		offices,
		ownerId,
		domain,
		decisionId,
		decision
	}: {
		snapshot?: OrgSnapshot;
		items: readonly WorkItemSummary[];
		offices: readonly OfficeInfo[];
		ownerId: string;
		domain?: HomeDomainData;
		decisionId?: string;
		decision?: DecisionAiContext;
	}
): OrgAiAnswer {
	const query = rawQuery.trim();
	const q = query.toLowerCase();

	const formatDecisionAnswer = (decisionMatch: DecisionAiContext): OrgAiAnswer => ({
		query,
		headline: `${decisionMatch.title} — ${decisionMatch.risk.label} · ${decisionMatch.impact.label}`,
		bullets: [
			{
				text: `Recommendation: ${decisionMatch.recommendation}`,
				href: decisionMatch.detailHref,
				linkLabel: 'Open details'
			},
			{
				text: `Risk & impact: ${decisionMatch.risk.label} (${decisionMatch.risk.sub}); ${decisionMatch.impact.label} (${decisionMatch.impact.sub}).`
			},
			{
				text: `Background: ${decisionMatch.backgroundContext}`,
				href: decisionMatch.projectId ? `/projects/${decisionMatch.projectId}/work` : '/inbox',
				linkLabel: decisionMatch.projectId ? `Open ${decisionMatch.projectName}` : 'Open Inbox'
			}
		]
	});

	if (decision) return formatDecisionAnswer(decision);
	if (!snapshot) {
		return {
			query,
			headline: 'Workspace context is still loading.',
			bullets: [
				{
					text: 'Try asking again once the organization snapshot finishes loading.',
					href: '/home',
					linkLabel: 'Open Home'
				}
			]
		};
	}

	const kpis = deriveKpiMetrics(snapshot, items, offices, ownerId, domain);
	const spotlight = deriveDecisionSpotlight(
		items,
		snapshot,
		ownerId,
		Date.now(),
		domain?.inboxItems
	);
	const decisionMatch = findMatchingDecision(query, spotlight, decisionId);
	if (decisionMatch) return formatDecisionAnswer(decisionMatch);

	const asksBudget = /spend|cost|budget|money|cap|dollar/.test(q);
	const asksTeam = /office|agent|team|online|offline|who/.test(q);
	if (asksBudget && asksTeam) return answerBudgetAndTeam(query, kpis);
	if (asksBudget) return answerBudgetOnly(query, kpis);
	if (/need|first|urgent|inbox|decision|approv|wait/.test(q)) {
		return answerUrgentInbox(query, spotlight);
	}
	if (/project|slow|risk|block|health|track/.test(q)) {
		return answerProjectsHealth(query, deriveProjectCards(snapshot, items, domain));
	}
	if (asksTeam) return answerTeamAndOffices(query, kpis);

	const time = deriveTaskTimeBreakdown(items, ownerId, kpis.offlineOffices, domain);
	return {
		query,
		headline: `${kpis.openCount} open items · ${kpis.completionPct}% complete · ${spotlight.length} waiting on you.`,
		bullets: [
			{
				text: time.summary,
				href: time.primaryLink.href,
				linkLabel: time.primaryLink.label
			},
			{
				text: `${kpis.activeAgents} active agents across ${kpis.projectsTotal} projects.`,
				href: '/projects',
				linkLabel: 'View Projects'
			}
		]
	};
}
