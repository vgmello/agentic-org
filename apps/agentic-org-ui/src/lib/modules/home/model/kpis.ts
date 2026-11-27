import { isItemDone } from '$lib/format';
import type { AgentBudgets, OrganizationCost } from '$lib/modules/costs/types';
import { agentsOf, isFormer, nonOwners, peopleOf } from '$lib/modules/organization/model';
import type { ProjectRow } from '$lib/modules/projects/types';
import { needsYou } from '$lib/modules/work/saved';
import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import { deriveProjectCards } from './projects';
import {
	formatCompactUsd,
	isItemOpen,
	isOfficeOnline,
	type ExecutiveKpis,
	type FinancialKpi,
	type HomeDomainData,
	type SignalTone,
	type TaskTimeBreakdown
} from './types';

function financialPillState(
	cappedAgent: { memberId: string; name: string } | undefined,
	spentMicroUsd: number | null,
	budgetMicroUsd: number | null,
	usagePct: number
): { pillLabel: string; pillTone: SignalTone } {
	if (cappedAgent) {
		return { pillLabel: `${cappedAgent.name.split(/\s+/)[0]} at cap`, pillTone: 'warn' };
	}
	if (spentMicroUsd === null || spentMicroUsd === 0) {
		return { pillLabel: 'No spend yet', pillTone: 'ok' };
	}
	if (budgetMicroUsd !== null && usagePct >= 95) {
		return { pillLabel: `${usagePct}% of budget`, pillTone: 'bad' };
	}
	if (budgetMicroUsd !== null && usagePct >= 80) {
		return { pillLabel: `${usagePct}% of budget`, pillTone: 'warn' };
	}
	return { pillLabel: 'On budget', pillTone: 'ok' };
}

export function deriveFinancialKpi(
	orgCost: OrganizationCost | undefined,
	agentBudgets: AgentBudgets | undefined,
	doneCount: number
): FinancialKpi {
	if (!orgCost && !agentBudgets) {
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
	const { pillLabel, pillTone } = financialPillState(
		cappedAgent,
		spentMicroUsd,
		budgetMicroUsd,
		usagePct
	);
	const costPerTaskLabel =
		spentMicroUsd !== null && spentMicroUsd > 0 && doneCount > 0
			? formatCompactUsd(Math.round(spentMicroUsd / doneCount))
			: '—';

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
		budgetedAgentsCount: (agentBudgets?.agents ?? []).filter(
			(a) => !a.former && (a.daily || a.monthly)
		).length,
		recordsCount: orgCost?.totals.records ?? 0
	};
}

function deriveTeamHeadcount(
	snapshot: OrgSnapshot,
	offices: readonly OfficeInfo[],
	directory: HomeDomainData['memberRows']
) {
	if (directory && directory.length > 0) {
		const activeRows = directory.filter((m) => m.status !== 'inactive');
		const agentRows = activeRows.filter((m) => m.kind === 'agent');
		const personRows = activeRows.filter((m) => m.kind === 'person' && !m.owner);
		const hasPresence = agentRows.some((a) => a.presence !== undefined);
		const totalAgents = agentRows.length;
		const activeAgents = agentRows.filter(
			(a) =>
				a.status === 'active' &&
				a.office !== null &&
				(!hasPresence || a.presence?.id === 'working' || a.presence?.id === 'with_you')
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

function officeStatusPill(
	offices: readonly OfficeInfo[],
	offlineOffices: number
): {
	officePillLabel: string;
	officePillTone: SignalTone;
} {
	if (offlineOffices > 0) {
		return {
			officePillLabel: `${offlineOffices} ${offlineOffices === 1 ? 'office' : 'offices'} offline`,
			officePillTone: 'bad'
		};
	}
	if (offices.length === 1) return { officePillLabel: 'HQ online', officePillTone: 'ok' };
	if (offices.length > 1) return { officePillLabel: 'All offices online', officePillTone: 'ok' };
	return { officePillLabel: 'No offices', officePillTone: 'muted' };
}

function countOpenAndCriticalBugs(
	openItems: readonly WorkItemSummary[],
	projectRows: readonly ProjectRow[]
): { openBugs: number; criticalBugs: number } {
	const workOpenBugs = openItems.filter((i) => i.kind === 'bug').length;
	const rowOpenBugs = projectRows.reduce(
		(s, p) => s + p.bugs.critical + p.bugs.major + p.bugs.minor + p.bugs.trivial + p.bugs.unset,
		0
	);
	const workCritical = openItems.filter(
		(i) => i.kind === 'bug' && (i.severity === 'critical' || i.severity === 'major')
	).length;
	const rowCritical = projectRows.reduce((s, p) => s + p.bugs.critical + p.bugs.major, 0);
	return {
		openBugs: Math.max(workOpenBugs, rowOpenBugs),
		criticalBugs: Math.max(workCritical, rowCritical)
	};
}

export const KPI_PILLAR_ORDER = ['projects', 'delivery', 'team', 'financials'] as const;

function itemCycleHours(item: WorkItemSummary): number | null {
	const doneTs = Date.parse(item.updated_at ?? item.created_at ?? '');
	const createdTs = Date.parse(item.created_at ?? '');
	if (!Number.isFinite(doneTs) || !Number.isFinite(createdTs) || doneTs <= createdTs) return null;
	return (doneTs - createdTs) / 3_600_000;
}

function medianOf(values: readonly number[]): number | null {
	if (values.length === 0) return null;
	const sorted = [...values].sort((a, b) => a - b);
	const mid = Math.floor(sorted.length / 2);
	return sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function deriveDeliveryWindowStats(
	doneItems: readonly WorkItemSummary[],
	nowMs: number
): {
	throughputPoints: number[];
	closedLast7d: number;
	closedPrior7d: number | null;
	medianCycleHours: number | null;
} {
	const DAYS = 7;
	const DAY_MS = 24 * 3600 * 1000;
	const buckets = Array.from({ length: DAYS }, () => 0);
	let closedLast7d = 0;
	let closedPrior7d = 0;
	const recentCycles: number[] = [];
	const allCycles: number[] = [];

	for (const item of doneItems) {
		const cycleH = itemCycleHours(item);
		if (cycleH !== null) allCycles.push(cycleH);
		const doneTs = Date.parse(item.updated_at ?? item.created_at ?? '');
		if (!Number.isFinite(doneTs)) continue;
		const daysAgo = Math.floor((nowMs - doneTs) / DAY_MS);
		if (daysAgo >= 0 && daysAgo < DAYS) {
			buckets[DAYS - 1 - daysAgo] += 1;
			closedLast7d += 1;
			if (cycleH !== null) recentCycles.push(cycleH);
		} else if (daysAgo >= DAYS && daysAgo < DAYS * 2) {
			closedPrior7d += 1;
		}
	}

	return {
		throughputPoints: buckets,
		closedLast7d,
		closedPrior7d: closedPrior7d > 0 ? closedPrior7d : null,
		medianCycleHours: medianOf(recentCycles.length > 0 ? recentCycles : allCycles)
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
	const urgentCount = openInboxCount > 0 ? openInboxCount : needsYou(liveItems, ownerId);
	const totalTracked = openCount + doneCount;
	const completionPct = totalTracked > 0 ? Math.round((doneCount * 100) / totalTracked) : 0;

	const headcount = deriveTeamHeadcount(snapshot, offices, domain?.memberRows);
	const autonomyPct =
		openCount > 0
			? Math.max(
					0,
					Math.min(
						100,
						Math.round(((openCount - Math.min(openCount, urgentCount)) * 100) / openCount)
					)
				)
			: headcount.activeAgents > 0
				? 100
				: 0;

	const onlineOffices = offices.filter(isOfficeOnline).length;
	const offlineOfficeList = offices.filter((o) => !isOfficeOnline(o));
	const offlineOffices = offlineOfficeList.length;
	const { officePillLabel, officePillTone } = officeStatusPill(offices, offlineOffices);

	const projectCards = deriveProjectCards(snapshot, liveItems, domain, nowMs);
	const offTrackProjects = projectCards.filter((p) => p.health === 'Off track').length;
	const atRiskProjects = projectCards.filter((p) => p.health === 'At risk').length;

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
	const { openBugs, criticalBugs } = countOpenAndCriticalBugs(openItems, projectRows);

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
	const deliveryStats = deriveDeliveryWindowStats(doneItems, nowMs);

	return {
		financials,
		openCount,
		doneCount,
		blockedCount,
		urgentCount,
		completionPct,
		...headcount,
		leadsCount,
		specialistsCount: Math.max(0, headcount.totalAgents - leadsCount),
		budgetRemainingPct,
		autonomyPct,
		onlineOffices,
		offlineOffices,
		offlineOfficeNames: offlineOfficeList.map((o) => o.name),
		officePillLabel,
		officePillTone,
		projectsTotal: projectCards.length,
		projectsAttention: offTrackProjects + atRiskProjects,
		offTrackProjects,
		atRiskProjects,
		openBugs,
		criticalBugs,
		firstTryPassPct,
		...deliveryStats
	};
}

function classifyWorkBucket(
	item: WorkItemSummary,
	ownerId: string
): 'waitingYou' | 'working' | 'review' | 'queued' {
	const needsOwner =
		(item.kind === 'request' && item.status === 'scoped') ||
		(item.kind === 'bug' && (item.status === 'new' || item.status === 'fixed')) ||
		(item.kind === 'task' && item.status === 'blocked' && item.assignee_id === ownerId);
	if (needsOwner) return 'waitingYou';
	if (item.status === 'in_progress' || item.status === 'scoping') return 'working';
	if (item.status === 'triaged' || item.status === 'routed') return 'review';
	return 'queued';
}

function taskTimeNarrative(
	queuedIsHigh: boolean,
	queuedPct: number,
	waitingYouPct: number,
	offlineOffices: number,
	idleAgents: number
): {
	summary: string;
	primaryLink: { label: string; href: string };
	secondaryLink?: { label: string; href: string };
} {
	if (queuedIsHigh) {
		const advice =
			offlineOffices > 0
				? ' Reconnecting offline offices or activating idle agents will speed up delivery.'
				: idleAgents > 0
					? ` Assigning ${idleAgents} idle specialists will clear the backlog faster.`
					: ' Assigning specialists will clear the backlog faster.';
		return {
			summary: `${queuedPct}% of open work is waiting for an agent.${advice}`,
			primaryLink:
				offlineOffices > 0
					? { label: 'Check offices', href: '/settings/offices' }
					: { label: 'Open Work queue', href: '/work' },
			secondaryLink: { label: 'Review members', href: '/teams/members' }
		};
	}
	if (waitingYouPct >= 25) {
		return {
			summary: `${waitingYouPct}% of open work is waiting on your review. Clearing the Decision Spotlight will unblock downstream tasks.`,
			primaryLink: { label: 'Open Inbox', href: '/inbox' },
			secondaryLink: { label: 'Open Work', href: '/work' }
		};
	}
	return {
		summary: 'Work is flowing smoothly across active agents and your decisions.',
		primaryLink: { label: 'Open Work', href: '/work' }
	};
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

	if (open.length === 0 && openInboxCount === 0 && rowWorking + rowBlocked + rowTodo === 0) {
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

	const counts = { working: 0, review: 0, waitingYou: 0, queued: 0 };
	if (open.length > 0) {
		for (const item of open) counts[classifyWorkBucket(item, ownerId)] += 1;
	} else {
		counts.working = rowWorking;
		counts.waitingYou = Math.max(openInboxCount, rowBlocked);
		counts.queued = rowTodo;
	}

	const denom = Math.max(1, counts.working + counts.review + counts.waitingYou + counts.queued);
	const workingPct = Math.round((counts.working * 100) / denom);
	const reviewPct = Math.round((counts.review * 100) / denom);
	const waitingYouPct = Math.round((counts.waitingYou * 100) / denom);
	const queuedPct = Math.max(0, 100 - workingPct - reviewPct - waitingYouPct);
	const queuedIsHigh = queuedPct > 30;

	const idleAgents = (domain?.memberRows ?? []).filter(
		(m) => m.kind === 'agent' && m.status !== 'inactive' && m.presence?.id !== 'working'
	).length;
	const narrative = taskTimeNarrative(
		queuedIsHigh,
		queuedPct,
		waitingYouPct,
		offlineOffices,
		idleAgents
	);

	return {
		totalOpen: denom,
		workingCount: counts.working,
		reviewCount: counts.review,
		waitingYouCount: counts.waitingYou,
		queuedCount: counts.queued,
		workingPct,
		reviewPct,
		waitingYouPct,
		queuedPct,
		queuedIsHigh,
		...narrative
	};
}
