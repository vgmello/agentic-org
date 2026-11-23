import type { Member, OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import type { ActivityRow } from '$lib/modules/activity/types';
import type { AgentBudgets, OrganizationCost } from '$lib/modules/costs/types';
import type { HomeProject } from '$lib/modules/home/types';
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
} from '$lib/org/model';
import { needsYou } from '$lib/work/saved';

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

export interface SpotlightCell {
	label: string;
	sub: string;
	tone: SignalTone;
	icon: 'shield-check' | 'shield-alert' | 'bolt' | 'check-circle' | 'clock' | 'scale';
}

export type SpotlightPrimaryAction =
	| { kind: 'approve_inbox'; label: string }
	| { kind: 'choose_inbox'; label: string; choiceId: string }
	| { kind: 'dismiss_inbox'; label: string }
	| { kind: 'open_inbox'; label: string }
	| { kind: 'approve_request'; label: string }
	| { kind: 'verify_bug'; label: string }
	| { kind: 'triage_bug'; label: string }
	| { kind: 'unblock_task'; label: string };

export type SpotlightSecondaryAction =
	| { kind: 'deny_inbox'; label: string }
	| { kind: 'choose_inbox'; label: string; choiceId: string }
	| { kind: 'decline_request'; label: string }
	| { kind: 'open_work'; label: string };

export interface SpotlightDecision {
	id: string;
	sourceKind: 'inbox' | 'work';
	category: 'approval' | 'decision' | 'problem' | 'question' | 'request' | 'bug' | 'task';
	item?: WorkItemSummary;
	inboxItem?: InboxItem;
	kindLabel: string;
	urgent: boolean;
	badgeText: string;
	expiresShort?: string;
	actorId?: string;
	actorName: string;
	actorInitials: string;
	isChiefOfStaff?: boolean;
	projectName: string;
	projectId?: string;
	title: string;
	recommendation: string;
	recTone: 'ok' | 'warn' | 'bad' | 'neutral';
	recRiskLabel?: string;
	backgroundContext: string;
	auditChecks: string[];
	risk: SpotlightCell;
	impact: SpotlightCell;
	proof: SpotlightCell;
	primaryAction: SpotlightPrimaryAction;
	secondaryAction?: SpotlightSecondaryAction;
	detailHref: string;
	ageLabel: string;
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

export function initialsOf(name: string): string {
	const parts = name.trim().split(/\s+/).filter(Boolean);
	if (parts.length === 0) return '?';
	if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
	return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

export function greetingForHour(hour: number): string {
	if (hour < 12) return 'Good morning.';
	if (hour < 18) return 'Good afternoon.';
	return 'Good evening.';
}

export function relativeAge(iso: string, nowMs: number): string {
	const ts = Date.parse(iso);
	if (Number.isNaN(ts)) return 'recently';
	const diffMin = Math.max(0, Math.round((nowMs - ts) / 60_000));
	if (diffMin < 2) return 'Just now';
	if (diffMin < 60) return `${diffMin}m ago`;
	const diffH = Math.round(diffMin / 60);
	if (diffH < 24) return `${diffH}h ago`;
	const diffD = Math.round(diffH / 24);
	return `${diffD}d ago`;
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
	if (item.kind === 'task') return item.status !== 'done';
	if (item.kind === 'bug') return item.status !== 'closed';
	if (item.kind === 'request') {
		return !['approved', 'declined', 'merged'].includes(item.status);
	}
	return false;
}

export function isItemDone(item: WorkItemSummary): boolean {
	if (item.deleted) return false;
	return (
		(item.kind === 'task' && item.status === 'done') ||
		(item.kind === 'bug' && item.status === 'closed') ||
		(item.kind === 'request' && item.status === 'approved')
	);
}

export function isOfficeOnline(office: OfficeInfo): boolean {
	return office.status === 'active' || office.status === undefined;
}

export function isDenyAction(
	action: SpotlightSecondaryAction | undefined
): action is { kind: 'deny_inbox' | 'decline_request'; label: string } {
	return action?.kind === 'deny_inbox' || action?.kind === 'decline_request';
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

function inboxPrimaryLabel(item: InboxItem): string {
	if (item.actions.includes('choose') && item.choices?.[0]) return item.choices[0].label;
	if (item.actions.includes('approve')) return item.approve_label?.trim() || 'Approve';
	if (item.actions.includes('dismiss')) return 'Dismiss';
	return item.open?.label ?? 'Open in Inbox';
}

function formatExpiryBadge(iso: string, nowMs: number): string {
	const diffMin = Math.round((Date.parse(iso) - nowMs) / 60_000);
	if (Number.isNaN(diffMin)) return 'Time-sensitive';
	if (diffMin <= 0) return 'Expired';
	if (diffMin < 60) return `Expires in ${diffMin}m`;
	const diffH = Math.round(diffMin / 60);
	if (diffH < 24) return `Expires in ${diffH}h`;
	return 'Expires tomorrow';
}

function inboxToSpotlight(item: InboxItem, nowMs: number): SpotlightDecision {
	const urgent = Boolean(item.expires_at) || item.kind === 'problem';
	const badgeText = item.expires_at
		? formatExpiryBadge(item.expires_at, nowMs)
		: item.source || 'Decision';

	const hasApprove = item.actions.includes('approve');
	const hasDeny = item.actions.includes('deny');
	const hasChoose = item.actions.includes('choose') && (item.choices?.length ?? 0) > 0;
	const hasDismiss = item.actions.includes('dismiss');

	const primaryAction: SpotlightPrimaryAction = hasApprove
		? { kind: 'approve_inbox', label: inboxPrimaryLabel(item) }
		: hasChoose
			? { kind: 'choose_inbox', label: item.choices![0].label, choiceId: item.choices![0].id }
			: hasDismiss
				? { kind: 'dismiss_inbox', label: 'Dismiss' }
				: { kind: 'open_inbox', label: item.open?.label ?? 'Open in Inbox' };

	const secondaryAction: SpotlightSecondaryAction | undefined = hasDeny
		? { kind: 'deny_inbox', label: 'Deny' }
		: hasChoose && item.choices && item.choices.length > 1
			? { kind: 'choose_inbox', label: item.choices[1].label, choiceId: item.choices[1].id }
			: item.open
				? { kind: 'open_work', label: item.open.label }
				: undefined;

	const projectName = item.project?.name ?? item.team?.name ?? 'Organization';
	const risk: SpotlightCell =
		item.kind === 'problem' || item.kind === 'decision'
			? {
					label: 'Moderate risk',
					sub: item.expires_at ? 'Time-sensitive gate' : 'Needs owner ruling',
					tone: 'warn',
					icon: 'shield-alert'
				}
			: {
					label: 'Low risk',
					sub: 'Reversible gate',
					tone: 'ok',
					icon: 'shield-check'
				};

	const impact: SpotlightCell = {
		label: urgent || item.kind === 'approval' ? 'High impact' : 'Medium impact',
		sub: `Unblocks ${projectName}`,
		tone: 'sky',
		icon: urgent || item.kind === 'approval' ? 'bolt' : 'scale'
	};

	const proof: SpotlightCell = item.recommendation
		? {
				label: 'Reviewed',
				sub: item.source,
				tone: 'ok',
				icon: 'check-circle'
			}
		: {
				label: 'Ready to review',
				sub: item.source,
				tone: 'muted',
				icon: 'clock'
			};

	const auditChecks: string[] = [];
	if (item.source) auditChecks.push(`Source: ${item.source}`);
	if (item.project?.name) auditChecks.push(`Project: ${item.project.name}`);
	if (item.team?.name) auditChecks.push(`Team: ${item.team.name}`);
	if (item.open?.label) auditChecks.push(item.open.label);
	if (item.expires_at) auditChecks.push(formatExpiryBadge(item.expires_at, nowMs));

	const recTone: SpotlightDecision['recTone'] =
		item.kind === 'problem'
			? 'bad'
			: item.kind === 'decision' || item.kind === 'question'
				? 'warn'
				: item.recommendation
					? 'ok'
					: 'neutral';
	const recRiskLabel =
		recTone === 'ok'
			? 'Low risk'
			: recTone === 'warn'
				? 'Investigate'
				: recTone === 'bad'
					? 'Use caution'
					: undefined;
	const isChiefOfStaff =
		item.who.id === 'tess' ||
		item.source.toLowerCase().includes('tess') ||
		item.source.toLowerCase().includes('chief of staff');

	return {
		id: item.id,
		sourceKind: 'inbox',
		category: item.kind,
		inboxItem: item,
		kindLabel: item.source,
		urgent,
		badgeText,
		expiresShort: item.expires_at ? formatExpiryBadge(item.expires_at, nowMs) : undefined,
		actorId: item.who.id,
		actorName: item.who.name,
		actorInitials: initialsOf(item.who.name),
		isChiefOfStaff,
		projectName,
		projectId: item.project?.id,
		title: item.title,
		recommendation: item.recommendation ?? item.why,
		recTone,
		recRiskLabel,
		backgroundContext: item.why,
		auditChecks,
		risk,
		impact,
		proof,
		primaryAction,
		secondaryAction,
		detailHref: item.open?.path ?? `/inbox?item=${encodeURIComponent(item.id)}`,
		ageLabel: relativeAge(item.created_at, nowMs)
	};
}

export function deriveDecisionSpotlight(
	items: readonly WorkItemSummary[],
	snapshot: OrgSnapshot,
	ownerId: string,
	nowMs = Date.now(),
	inboxItems?: readonly InboxItem[]
): SpotlightDecision[] {
	const openInbox = (inboxItems ?? []).filter((i) => i.state === 'open');
	const inboxScore = (item: InboxItem): number => {
		let s = 0;
		if (item.expires_at) s += 100;
		if (item.kind === 'approval') s += 60;
		else if (item.kind === 'decision') s += 50;
		else if (item.kind === 'problem') s += 35;
		else if (item.kind === 'question') s += 20;
		if (item.actions.includes('approve') || item.actions.includes('choose')) s += 15;
		return s;
	};
	const inboxSpot = [...openInbox]
		.sort(
			(a, b) => inboxScore(b) - inboxScore(a) || Date.parse(b.created_at) - Date.parse(a.created_at)
		)
		.map((item) => inboxToSpotlight(item, nowMs));

	// Track referenced work refs/titles from inbox so we don't show duplicates.
	const seenRefs = new Set<string>();
	const seenTitles = new Set<string>();
	for (const it of openInbox) {
		seenTitles.add(it.title.trim().toLowerCase());
		const match = it.open?.path.match(/\/work\/([^/?#]+)/);
		if (match) seenRefs.add(decodeURIComponent(match[1]).toUpperCase());
		const titleRef = it.title.match(/\b([A-Z]+-\d+)\b/);
		if (titleRef) seenRefs.add(titleRef[1].toUpperCase());
	}

	const projects = new Map((snapshot.projects ?? []).map((p) => [p.id, p]));
	const candidates = items.filter((item) => {
		if (item.deleted) return false;
		if (seenRefs.has(item.short_id.toUpperCase())) return false;
		if (seenTitles.has(item.title.trim().toLowerCase())) return false;
		if (item.kind === 'request' && item.status === 'scoped') return true;
		if (item.kind === 'bug' && (item.status === 'new' || item.status === 'fixed')) return true;
		if (item.kind === 'task' && item.status === 'blocked' && item.assignee_id === ownerId)
			return true;
		return false;
	});

	const score = (item: WorkItemSummary): number => {
		let s = 0;
		if (item.priority === 'urgent' || item.severity === 'critical') s += 100;
		else if (item.priority === 'high' || item.severity === 'major') s += 50;
		if (item.kind === 'request' && item.status === 'scoped') s += 40;
		if (item.kind === 'bug' && item.status === 'fixed') s += 30;
		if (item.kind === 'task' && item.status === 'blocked') s += 25;
		return s;
	};

	const workSpot = [...candidates]
		.sort((a, b) => score(b) - score(a) || Date.parse(b.updated_at) - Date.parse(a.updated_at))
		.map((item): SpotlightDecision => {
			const actor: Member | undefined =
				memberById(snapshot, item.assignee_id) ?? memberById(snapshot, item.created_by);
			const actorName = actor?.name ?? 'Team member';
			const project = item.project_id ? projects.get(item.project_id) : undefined;
			const projectName = project?.name ?? 'Organization';
			const urgent =
				item.priority === 'urgent' ||
				item.severity === 'critical' ||
				(item.kind === 'task' && item.status === 'blocked');

			const isRequest = item.kind === 'request';
			const isFixedBug = item.kind === 'bug' && item.status === 'fixed';
			const isNewBug = item.kind === 'bug' && item.status === 'new';

			const kindLabel = isRequest
				? 'Request sign-off'
				: isFixedBug
					? 'Bug verification'
					: isNewBug
						? 'Bug triage'
						: 'Blocked task';

			const badgeText = urgent
				? item.severity === 'critical'
					? 'Critical · needs attention'
					: 'Priority decision'
				: kindLabel;

			const recommendation = isRequest
				? `${actorName} scoped this request. Approving turns it into active work for the team.`
				: isFixedBug
					? `${actorName} marked this fix complete. Verify and close if it meets expectations.`
					: isNewBug
						? `New ${item.severity ?? 'reported'} bug waiting for triage and owner confirmation.`
						: item.blocked_reason
							? `Blocked: ${item.blocked_reason}`
							: `${actorName} is waiting on your input to unblock this task.`;

			const risk: SpotlightCell =
				item.severity === 'critical' || item.priority === 'urgent'
					? {
							label: 'Moderate risk',
							sub: 'Time-sensitive item',
							tone: 'warn',
							icon: 'shield-alert'
						}
					: {
							label: 'Low risk',
							sub: 'Reversible decision',
							tone: 'ok',
							icon: 'shield-check'
						};

			const impact: SpotlightCell =
				urgent || isRequest
					? {
							label: 'High impact',
							sub: `Unblocks ${projectName}`,
							tone: 'sky',
							icon: 'bolt'
						}
					: {
							label: 'Medium impact',
							sub: `Keeps ${projectName} moving`,
							tone: 'sky',
							icon: 'scale'
						};

			const checklistDone = item.checklist.total > 0;
			const proof: SpotlightCell = checklistDone
				? {
						label:
							item.checklist.required_left === 0
								? 'Checks passed'
								: `${item.checklist.done}/${item.checklist.total} steps`,
						sub:
							item.checklist.required_left === 0
								? `${item.checklist.done} of ${item.checklist.total} complete`
								: `${item.checklist.required_left} required left`,
						tone: item.checklist.required_left === 0 ? 'ok' : 'warn',
						icon: 'check-circle'
					}
				: {
						label: isFixedBug ? 'Fix ready' : 'Ready to review',
						sub: item.short_id,
						tone: isFixedBug ? 'ok' : 'muted',
						icon: 'clock'
					};

			const primaryAction: SpotlightPrimaryAction = isRequest
				? { kind: 'approve_request', label: 'Approve request' }
				: isFixedBug
					? { kind: 'verify_bug', label: 'Verify fix' }
					: isNewBug
						? { kind: 'triage_bug', label: 'Triage bug' }
						: { kind: 'unblock_task', label: 'Resume task' };

			const secondaryAction: SpotlightSecondaryAction = isRequest
				? { kind: 'decline_request', label: 'Deny' }
				: { kind: 'open_work', label: 'Inspect' };

			const backgroundContext = isRequest
				? `${item.short_id} in ${projectName} is scoped and ready for approval. Approving converts the scoped outcome into executable work items assigned to the team; denying returns it with your note.`
				: isFixedBug
					? `${item.short_id} (${item.severity ?? 'reported'} severity) in ${projectName} was marked fixed by ${actorName}. Verifying closes the bug and clears it from the delivery queue.`
					: isNewBug
						? `${item.short_id} was reported in ${projectName}${item.severity ? ` with ${item.severity} severity` : ''}. Triaging routes it to the responsible area and sets priority for an agent or owner.`
						: `${item.short_id} in ${projectName} is currently blocked${item.blocked_reason ? ` (${item.blocked_reason})` : ''}. Resuming moves it back to in-progress so ${actorName} can continue.`;

			const auditChecks: string[] = [];
			if (item.checklist.total > 0) {
				auditChecks.push(
					`${item.checklist.done}/${item.checklist.total} checklist steps completed`
				);
			}
			auditChecks.push(`Ref ${item.short_id}`);
			if (item.priority) auditChecks.push(`Priority: ${item.priority}`);
			if (item.severity) auditChecks.push(`Severity: ${item.severity}`);

			const detailHref = isRequest
				? `/work?request=${encodeURIComponent(item.id)}`
				: `/work/${encodeURIComponent(item.short_id)}`;

			const workRecTone: SpotlightDecision['recTone'] =
				item.severity === 'critical'
					? 'bad'
					: urgent || isNewBug
						? 'warn'
						: isFixedBug || isRequest
							? 'ok'
							: 'neutral';
			const workRecRiskLabel =
				workRecTone === 'ok'
					? 'Low risk'
					: workRecTone === 'warn'
						? 'Investigate'
						: workRecTone === 'bad'
							? 'Use caution'
							: undefined;

			return {
				id: item.id,
				sourceKind: 'work',
				category: item.kind === 'unknown' ? 'task' : item.kind,
				item,
				kindLabel,
				urgent,
				badgeText,
				actorId: actor?.id,
				actorName,
				actorInitials: initialsOf(actorName),
				isChiefOfStaff: actor?.agent?.role === 'chief_of_staff',
				projectName,
				projectId: item.project_id,
				title: item.title,
				recommendation,
				recTone: workRecTone,
				recRiskLabel: workRecRiskLabel,
				backgroundContext,
				auditChecks,
				risk,
				impact,
				proof,
				primaryAction,
				secondaryAction,
				detailHref,
				ageLabel: relativeAge(item.updated_at, nowMs)
			};
		});

	return [...inboxSpot, ...workSpot];
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
		const memberIds = new Set(
			snapshot.memberships.filter((m) => teamIds.has(m.team_id)).map((m) => m.member_id)
		);
		const projMembers = allMembers.filter((m) => memberIds.has(m.id));
		const lead = homeProj?.lead ?? teams.map((t) => leadOf(snapshot, t)).find(Boolean);

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
			blockedCount > 0 ||
			scopedReqs.length > 0 ||
			projInbox.length >= 2 ||
			(teams.length === 0 && !row)
		) {
			health = 'At risk';
			healthTone = 'warn';
			statusPillClass = 't-amber';
		}

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
			dueDt && dueDt.getTime() < todayUtc && (totalCount === 0 || doneCount < totalCount)
		);

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

export function deriveKpiMetrics(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[],
	offices: readonly OfficeInfo[],
	ownerId: string,
	domain?: HomeDomainData
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

	const directory = domain?.memberRows;
	let activeAgents: number;
	let pausedAgents: number;
	let unplacedAgents: number;
	let totalAgents: number;
	let peopleCount: number;

	if (directory && directory.length > 0) {
		const activeRows = directory.filter((m) => m.status !== 'inactive');
		const agentRows = activeRows.filter((m) => m.kind === 'agent');
		const personRows = activeRows.filter((m) => m.kind === 'person' && !m.owner);
		totalAgents = agentRows.length;
		peopleCount = personRows.length;
		activeAgents = agentRows.filter(
			(a) =>
				a.status === 'active' &&
				a.office !== null &&
				(a.presence?.id === 'working' || a.presence?.id === 'with_you')
		).length;
		unplacedAgents = agentRows.filter((a) => a.office === null).length;
		pausedAgents = Math.max(0, totalAgents - activeAgents - unplacedAgents);
	} else {
		const members = nonOwners(snapshot).filter((m) => !isFormer(m));
		const agents = agentsOf(members);
		const people = peopleOf(members);
		const knownOffices = new Set(offices.map((o) => o.id));
		totalAgents = agents.length;
		peopleCount = people.length;
		activeAgents = agents.filter((a) => a.agent?.status === 'active').length;
		pausedAgents = agents.filter((a) => a.agent?.status === 'paused').length;
		unplacedAgents = agents.filter(
			(a) => !a.office_id || (knownOffices.size > 0 && !knownOffices.has(a.office_id))
		).length;
	}

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

	const projectCards = deriveProjectCards(snapshot, liveItems, domain);
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
		snapshot: OrgSnapshot;
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
	const kpis = deriveKpiMetrics(snapshot, items, offices, ownerId, domain);
	const spotlight = deriveDecisionSpotlight(
		items,
		snapshot,
		ownerId,
		Date.now(),
		domain?.inboxItems
	);
	const projects = deriveProjectCards(snapshot, items, domain);
	const time = deriveTaskTimeBreakdown(items, ownerId, kpis.offlineOffices, domain);

	const explainMatch = query.match(/^explain the risk, impact, and recommendation for:\s*(.+)$/i);
	const decisionMatch =
		(decisionId ? spotlight.find((d) => d.id === decisionId) : undefined) ??
		decision ??
		(explainMatch
			? spotlight.find((d) => d.title.toLowerCase() === explainMatch[1].trim().toLowerCase())
			: undefined);
	if (decisionMatch) {
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

	if (/spend|cost|budget|money|cap|dollar/.test(q)) {
		return {
			query,
			headline: kpis.financials.available
				? `Monthly spend is ${kpis.financials.spentLabel} ${kpis.financials.budgetLabel} (${kpis.financials.usagePct}%).`
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

	if (/need|first|urgent|inbox|decision|approv|wait/.test(q)) {
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

	if (/project|slow|risk|block|health|track/.test(q)) {
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

	if (/office|agent|team|online|offline|who/.test(q)) {
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
