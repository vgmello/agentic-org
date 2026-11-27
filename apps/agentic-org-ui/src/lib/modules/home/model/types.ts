import type { OfficeInfo, WorkItemSummary } from '$lib/proto';
import type { ActivityRow } from '$lib/modules/activity/types';
import type { AgentBudgets, OrganizationCost } from '$lib/modules/costs/types';
import type { HomeProject } from '$lib/modules/home/types';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { MemberRow } from '$lib/modules/members/types';
import type { ProjectRow } from '$lib/modules/projects/types';

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

export function parseIsoDateUtc(isoDate: string): Date | null {
	const parts = isoDate.split('-').map(Number);
	if (parts.length !== 3 || parts.some((n) => Number.isNaN(n))) return null;
	return new Date(Date.UTC(parts[0], parts[1] - 1, parts[2], 12));
}

export function formatDueShort(isoDate: string): string {
	const dt = parseIsoDateUtc(isoDate);
	if (!dt) return isoDate;
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
