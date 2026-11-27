import { isItemDone, relativeAge } from '$lib/format';
import type { ActivityRow } from '$lib/modules/activity/types';
import { deriveDecisionSpotlight, type SpotlightDecision } from '$lib/modules/inbox/spotlight';
import { agentsOf, isFormer, memberById, nonOwners } from '$lib/modules/organization/model';
import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import { deriveKpiMetrics } from './kpis';
import { deriveProjectCards } from './projects';
import {
	formatCompactUsd,
	type BriefLens,
	type CatchUpRow,
	type ExecutiveBrief,
	type ExecutiveBriefLensContent,
	type ExecutiveBriefSection,
	type ExecutiveKpis,
	type HomeDomainData,
	type ProjectBentoCard
} from './types';

function activityRowTone(row: ActivityRow): CatchUpRow['tone'] {
	if (row.action === 'task-done' || row.action === 'merge' || row.run?.state === 'succeeded') {
		return 'ok';
	}
	if (row.run?.state === 'failed' || row.kind === 'decision') return 'warn';
	return 'sky';
}

export function deriveCatchUpDigest(
	items: readonly WorkItemSummary[],
	snapshot: OrgSnapshot,
	nowMs = Date.now(),
	activityRows?: readonly ActivityRow[]
): CatchUpRow[] {
	if (activityRows && activityRows.length > 0) {
		return activityRows.slice(0, 3).map((row): CatchUpRow => ({
			id: row.id,
			title: `${row.actor.name} ${row.text}`,
			detail: row.preview ?? row.run?.outcome ?? row.run?.trigger ?? 'Recorded in Activity',
			timeLabel: relativeAge(row.at, nowMs),
			tone: activityRowTone(row),
			href: row.href?.path ?? '/activity'
		}));
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
						.join(', ')}${activeAgents.length > 3 ? ` +${activeAgents.length - 3} more` : ''} standing by across ${(snapshot.projects ?? []).length} projects.`
				: 'Hire or place an agent in Offices to start autonomous delivery.',
		timeLabel: 'Live',
		tone: 'live',
		href: '/teams/members'
	});

	return rows;
}

function buildRiskParts(
	kpis: ExecutiveKpis,
	attentionProjects: readonly ProjectBentoCard[]
): string[] {
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
		const summary = attentionProjects
			.slice(0, 2)
			.map((p) => `${p.name} (${p.health})`)
			.join(', ');
		riskParts.push(
			`${attentionProjects.length} ${attentionProjects.length === 1 ? 'project needs' : 'projects need'} attention: ${summary}.`
		);
	}
	return riskParts;
}

function buildQueueSummary(
	spotlight: readonly SpotlightDecision[],
	inboxLoading: boolean,
	inboxUnavailable: boolean
): string {
	if (spotlight.length > 0) {
		return `${spotlight.length} ${spotlight.length === 1 ? 'decision is' : 'decisions are'} queued for your review, led by “${spotlight[0].title}” (${spotlight[0].kindLabel} · ${spotlight[0].projectName}).`;
	}
	if (inboxLoading) return 'Checking Inbox for pending decisions…';
	if (inboxUnavailable) return 'Inbox status unavailable — check Inbox directly for pending decisions.';
	return 'Your decision queue is clear — nothing is waiting on owner sign-off right now.';
}

function buildSpendSentence(kpis: ExecutiveKpis): string {
	if (!kpis.financials.available) {
		return 'Cost reporting will appear once the billing ledger connects.';
	}
	if (kpis.financials.budgetMicroUsd !== null) {
		return `Month-to-date spend is ${kpis.financials.spentLabel} of ${formatCompactUsd(kpis.financials.budgetMicroUsd)} (${kpis.financials.usagePct}%).`;
	}
	return `Month-to-date spend is ${kpis.financials.spentLabel}.`;
}

function buildLens(
	organizationName: string,
	lens: BriefLens,
	label: string,
	sections: ExecutiveBriefSection[],
	updatedLabel: string
): ExecutiveBriefLensContent {
	return {
		lens,
		label,
		sections,
		updatedLabel,
		copyText: [
			`Executive Brief (${label}) — ${organizationName}`,
			...sections.map((s) => `• ${s.kicker}: ${s.body}`)
		].join('\n')
	};
}

function buildWeekSections(
	kpis: ExecutiveKpis,
	projects: readonly ProjectBentoCard[],
	spendPhrase: string
): ExecutiveBriefSection[] {
	const healthy = projects.find((p) => p.health === 'On track');
	return [
		{
			id: 'velocity',
			kicker: 'Velocity & Delivery',
			tone: 'ok',
			body: `${kpis.doneCount} completed and ${kpis.openCount} open items across ${kpis.projectsTotal} active ${kpis.projectsTotal === 1 ? 'project' : 'projects'} (${kpis.completionPct}% complete).${healthy ? ` ${healthy.name} is tracking cleanly.` : ''}`
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
}

function buildBoardSections(input: {
	organizationName: string;
	kpis: ExecutiveKpis;
	projects: readonly ProjectBentoCard[];
	attentionCount: number;
	spotlightCount: number;
	inboxLoading: boolean;
	inboxUnavailable: boolean;
	spendPhrase: string;
	problemsBody: string;
	hasRisks: boolean;
}): ExecutiveBriefSection[] {
	const readinessTail = input.inboxLoading
		? 'Checking Inbox for pending decisions…'
		: input.inboxUnavailable
			? 'Inbox status unavailable — check Inbox directly for pending decisions.'
			: `${input.spotlightCount} owner ${input.spotlightCount === 1 ? 'sign-off remains' : 'sign-offs remain'} in the decision queue.`;
	return [
		{
			id: 'summary',
			kicker: 'Executive Summary',
			tone: 'ok',
			body: `${input.organizationName} is at ${input.kpis.completionPct}% portfolio completion (${input.kpis.doneCount} delivered, ${input.kpis.firstTryPassPct}% clean delivery) with ${input.kpis.autonomyPct}% autonomous execution. ${input.spendPhrase}`
		},
		{
			id: 'readiness',
			kicker: 'Delivery Readiness',
			tone: input.attentionCount > 0 ? 'accent' : 'ok',
			body:
				input.projects.length > 0
					? `${input.projects.length - input.attentionCount} of ${input.projects.length} active projects are on track; ${readinessTail}`
					: 'No active projects registered yet.'
		},
		{
			id: 'watchpoints',
			kicker: 'Operational Watchpoints',
			tone: input.hasRisks ? 'warn' : 'ok',
			body: input.problemsBody
		}
	];
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
	const spendPhrase = buildSpendSentence(kpis);
	const latestEventPhrase =
		recentActivity.length > 0
			? ` Latest: ${recentActivity[0].actor.name} ${recentActivity[0].text}.`
			: '';

	const liveBody =
		kpis.totalAgents > 0
			? `${kpis.activeAgents} of ${kpis.totalAgents} agents are active across ${kpis.projectsTotal} ${kpis.projectsTotal === 1 ? 'project' : 'projects'} (${kpis.autonomyPct}% autonomous execution). ${kpis.doneCount} ${kpis.doneCount === 1 ? 'item' : 'items'} completed with ${kpis.completionPct}% overall progress and ${kpis.firstTryPassPct}% clean delivery.${latestEventPhrase}`
			: `${kpis.openCount} open ${kpis.openCount === 1 ? 'item' : 'items'} and ${kpis.doneCount} completed across ${kpis.projectsTotal} ${kpis.projectsTotal === 1 ? 'project' : 'projects'}. Add or activate agents to increase autonomous delivery.`;

	const riskParts = buildRiskParts(kpis, attentionProjects);
	const problemsBody =
		riskParts.length > 0
			? riskParts.join(' ')
			: 'All connected offices are online, no work items are blocked, and every active project is tracking cleanly.';

	const inboxState = domain?.inboxState ?? 'ready';
	const inboxLoading = inboxState === 'loading' && spotlight.length === 0;
	const inboxUnavailable = inboxState === 'unavailable' && spotlight.length === 0;

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
			body: buildQueueSummary(spotlight, inboxLoading, inboxUnavailable)
		}
	];

	const lenses: Record<BriefLens, ExecutiveBriefLensContent> = {
		today: buildLens(organizationName, 'today', 'Today', todaySections, 'Live summary'),
		week: buildLens(
			organizationName,
			'week',
			'7-Day',
			buildWeekSections(kpis, projects, spendPhrase),
			'7-day rollup'
		),
		board: buildLens(
			organizationName,
			'board',
			'Board',
			buildBoardSections({
				organizationName,
				kpis,
				projects,
				attentionCount: attentionProjects.length,
				spotlightCount: spotlight.length,
				inboxLoading,
				inboxUnavailable,
				spendPhrase,
				problemsBody,
				hasRisks: riskParts.length > 0
			}),
			'Board ready'
		)
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
