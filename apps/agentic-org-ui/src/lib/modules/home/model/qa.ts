import { deriveDecisionSpotlight, type SpotlightDecision } from '$lib/modules/inbox/spotlight';
import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import { deriveKpiMetrics, deriveTaskTimeBreakdown } from './kpis';
import { deriveProjectCards } from './projects';
import type { ExecutiveKpis, HomeDomainData, OrgAiAnswer, ProjectBentoCard } from './types';

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

function formatDecisionAnswer(query: string, decisionMatch: DecisionAiContext): OrgAiAnswer {
	return {
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
	};
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

	if (decision) return formatDecisionAnswer(query, decision);
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
	if (decisionMatch) return formatDecisionAnswer(query, decisionMatch);

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
