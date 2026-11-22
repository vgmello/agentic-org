import type { Member, OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import { agentsOf, isFormer, leadOf, memberById, nonOwners, peopleOf, teamsOfProject } from '$lib/org/model';
import { needsYou } from '$lib/work/saved';

export type SignalTone = 'ok' | 'warn' | 'bad' | 'sky' | 'muted';

export interface SpotlightCell {
	label: string;
	sub: string;
	tone: SignalTone;
	icon: 'shield-check' | 'shield-alert' | 'bolt' | 'check-circle' | 'clock' | 'scale';
}

export interface SpotlightDecision {
	id: string;
	item: WorkItemSummary;
	kindLabel: string;
	urgent: boolean;
	badgeText: string;
	actorName: string;
	actorInitials: string;
	projectName: string;
	projectId?: string;
	title: string;
	recommendation: string;
	backgroundContext: string;
	auditChecks: string[];
	risk: SpotlightCell;
	impact: SpotlightCell;
	proof: SpotlightCell;
	primaryAction: {
		kind: 'approve_request' | 'verify_bug' | 'triage_bug' | 'unblock_task';
		label: string;
	};
	secondaryAction?: {
		kind: 'decline_request' | 'open_work';
		label: string;
	};
	detailHref: string;
	ageLabel: string;
}

export interface ExecutiveKpis {
	openCount: number;
	doneCount: number;
	blockedCount: number;
	urgentCount: number;
	completionPct: number;
	activeAgents: number;
	pausedAgents: number;
	unplacedAgents: number;
	totalAgents: number;
	peopleCount: number;
	autonomyPct: number;
	onlineOffices: number;
	offlineOffices: number;
	offlineOfficeNames: string[];
	officePillLabel: string;
	officePillTone: SignalTone;
	projectsTotal: number;
	projectsAttention: number;
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
	doneCount: number;
	totalCount: number;
	pct: number;
	blockerText: string;
	blockerTone: 'ok' | 'warn' | 'bad' | 'sky';
	actionLabel: string;
	actionHref: string;
	leadName: string;
	memberInitials: string[];
	extraMembers: number;
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

export function deriveDecisionSpotlight(
	items: readonly WorkItemSummary[],
	snapshot: OrgSnapshot,
	ownerId: string,
	nowMs = Date.now()
): SpotlightDecision[] {
	const projects = new Map((snapshot.projects ?? []).map((p) => [p.id, p]));
	const candidates = items.filter((item) => {
		if (item.deleted) return false;
		if (item.kind === 'request' && item.status === 'scoped') return true;
		if (item.kind === 'bug' && (item.status === 'new' || item.status === 'fixed')) return true;
		if (item.kind === 'task' && item.status === 'blocked' && item.assignee_id === ownerId) return true;
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

	return [...candidates]
		.sort((a, b) => score(b) - score(a) || Date.parse(b.updated_at) - Date.parse(a.updated_at))
		.map((item) => {
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

			const primaryAction: SpotlightDecision['primaryAction'] = isRequest
				? { kind: 'approve_request', label: 'Approve request' }
				: isFixedBug
					? { kind: 'verify_bug', label: 'Verify fix' }
					: isNewBug
						? { kind: 'triage_bug', label: 'Triage bug' }
						: { kind: 'unblock_task', label: 'Resume task' };

			const secondaryAction: SpotlightDecision['secondaryAction'] = isRequest
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

			const detailHref = isRequest ? `/work?request=${encodeURIComponent(item.id)}` : '/work';

			return {
				id: item.id,
				item,
				kindLabel,
				urgent,
				badgeText,
				actorName,
				actorInitials: initialsOf(actorName),
				projectName,
				projectId: item.project_id,
				title: item.title,
				recommendation,
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
}

export function deriveProjectCards(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[]
): ProjectBentoCard[] {
	const activeProjects = (snapshot.projects ?? []).filter((p) => !p.archived);
	const allMembers = nonOwners(snapshot).filter((m) => !isFormer(m));

	return activeProjects.map((project) => {
		const projItems = items.filter((i) => !i.deleted && i.project_id === project.id);
		const doneCount = projItems.filter(isItemDone).length;
		const totalCount = projItems.length;
		const pct = totalCount > 0 ? Math.round((doneCount * 100) / totalCount) : 0;

		const blocked = projItems.filter((i) => i.status === 'blocked');
		const criticalBugs = projItems.filter(
			(i) => i.kind === 'bug' && i.status !== 'closed' && (i.severity === 'critical' || i.severity === 'major')
		);
		const scopedReqs = projItems.filter((i) => i.kind === 'request' && i.status === 'scoped');

		const teams = teamsOfProject(snapshot, project.id);
		const teamIds = new Set(teams.map((t) => t.id));
		const memberIds = new Set(
			snapshot.memberships.filter((m) => teamIds.has(m.team_id)).map((m) => m.member_id)
		);
		const projMembers = allMembers.filter((m) => memberIds.has(m.id));
		const lead = teams.map((t) => leadOf(snapshot, t)).find(Boolean);

		let health: ProjectBentoCard['health'] = 'On track';
		let healthTone: ProjectBentoCard['healthTone'] = 'ok';
		if (criticalBugs.length > 0 || blocked.length >= 2) {
			health = 'Off track';
			healthTone = 'bad';
		} else if (blocked.length > 0 || scopedReqs.length > 0 || teams.length === 0) {
			health = 'At risk';
			healthTone = 'warn';
		}

		let blockerText = 'All active work flowing cleanly';
		let blockerTone: ProjectBentoCard['blockerTone'] = 'ok';
		let actionLabel = 'Open';
		let actionHref = `/projects/${project.id}`;

		if (criticalBugs.length > 0) {
			blockerText = `${criticalBugs.length} ${criticalBugs.length === 1 ? 'serious bug' : 'serious bugs'} open`;
			blockerTone = 'bad';
			actionLabel = 'Inspect';
			actionHref = '/work';
		} else if (blocked.length > 0) {
			blockerText =
				blocked[0].blocked_reason ||
				`${blocked.length} ${blocked.length === 1 ? 'task' : 'tasks'} blocked`;
			blockerTone = 'warn';
			actionLabel = 'Unblock';
			actionHref = '/inbox';
		} else if (scopedReqs.length > 0) {
			blockerText = `${scopedReqs.length} scoped ${scopedReqs.length === 1 ? 'request' : 'requests'} waiting sign-off`;
			blockerTone = 'sky';
			actionLabel = 'Review';
			actionHref = '/inbox';
		} else if (teams.length === 0) {
			blockerText = 'No team assigned yet';
			blockerTone = 'warn';
			actionLabel = 'Assign';
			actionHref = `/projects/${project.id}`;
		}

		const shown = projMembers.slice(0, 4).map((m) => initialsOf(m.name));
		const extraMembers = Math.max(0, projMembers.length - shown.length);

		return {
			id: project.id,
			name: project.name,
			key: project.key,
			health,
			healthTone,
			doneCount,
			totalCount,
			pct,
			blockerText,
			blockerTone,
			actionLabel,
			actionHref,
			leadName: lead?.name ?? (teams[0]?.name ? `${teams[0].name} team` : 'Unassigned'),
			memberInitials: shown,
			extraMembers
		};
	});
}

export function deriveKpiMetrics(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[],
	offices: readonly OfficeInfo[],
	ownerId: string
): ExecutiveKpis {
	const liveItems = items.filter((i) => !i.deleted);
	const openItems = liveItems.filter(isItemOpen);
	const doneItems = liveItems.filter(isItemDone);
	const blockedCount = openItems.filter((i) => i.status === 'blocked').length;
	const urgentCount = needsYou(liveItems, ownerId);

	const totalTracked = openItems.length + doneItems.length;
	const completionPct = totalTracked > 0 ? Math.round((doneItems.length * 100) / totalTracked) : 0;

	const members = nonOwners(snapshot).filter((m) => !isFormer(m));
	const agents = agentsOf(members);
	const people = peopleOf(members);
	const knownOffices = new Set(offices.map((o) => o.id));
	const activeAgents = agents.filter((a) => a.agent?.status === 'active').length;
	const pausedAgents = agents.filter((a) => a.agent?.status === 'paused').length;
	const unplacedAgents = agents.filter(
		(a) => !a.office_id || (knownOffices.size > 0 && !knownOffices.has(a.office_id))
	).length;

	const autonomyPct =
		openItems.length > 0
			? Math.max(0, Math.min(100, Math.round(((openItems.length - urgentCount) * 100) / openItems.length)))
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

	const projectCards = deriveProjectCards(snapshot, liveItems);
	const projectsAttention = projectCards.filter((p) => p.health !== 'On track').length;

	const openBugs = openItems.filter((i) => i.kind === 'bug').length;
	const criticalBugs = openItems.filter(
		(i) => i.kind === 'bug' && (i.severity === 'critical' || i.severity === 'major')
	).length;
	const reopened = liveItems.filter((i) => i.reopen_count > 0).length;
	const firstTryPassPct =
		doneItems.length > 0
			? Math.max(0, Math.round(((doneItems.length - Math.min(doneItems.length, reopened)) * 100) / doneItems.length))
			: totalTracked > 0
				? 100
				: 0;

	return {
		openCount: openItems.length,
		doneCount: doneItems.length,
		blockedCount,
		urgentCount,
		completionPct,
		activeAgents,
		pausedAgents,
		unplacedAgents,
		totalAgents: agents.length,
		peopleCount: people.length,
		autonomyPct,
		onlineOffices,
		offlineOffices,
		offlineOfficeNames,
		officePillLabel,
		officePillTone,
		projectsTotal: projectCards.length,
		projectsAttention,
		openBugs,
		criticalBugs,
		firstTryPassPct
	};
}

export function deriveTaskTimeBreakdown(
	items: readonly WorkItemSummary[],
	ownerId: string,
	offlineOffices = 0
): TaskTimeBreakdown {
	const open = items.filter(isItemOpen);
	const totalOpen = open.length;
	if (totalOpen === 0) {
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

	const workingPct = Math.round((workingCount * 100) / totalOpen);
	const reviewPct = Math.round((reviewCount * 100) / totalOpen);
	const waitingYouPct = Math.round((waitingYouCount * 100) / totalOpen);
	const queuedPct = Math.max(0, 100 - workingPct - reviewPct - waitingYouPct);
	// Explicit user threshold: >30% waiting for an agent uses soft coral-rose; <=30% uses purple.
	const queuedIsHigh = queuedPct > 30;

	let summary = 'Work is flowing smoothly across active agents and your decisions.';
	let primaryLink = { label: 'Open Work', href: '/work' };
	let secondaryLink: { label: string; href: string } | undefined;

	if (queuedIsHigh) {
		summary = `${queuedPct}% of open work is waiting for an agent.${offlineOffices > 0 ? ' Reconnecting offline offices or activating idle agents will speed up delivery.' : ' Assigning idle specialists will clear the backlog faster.'}`;
		primaryLink =
			offlineOffices > 0
				? { label: 'Check offices', href: '/offices' }
				: { label: 'Open Work queue', href: '/work' };
		secondaryLink = { label: 'Review agents', href: '/agents' };
	} else if (waitingYouPct >= 25) {
		summary = `${waitingYouPct}% of open work is waiting on your review. Clearing the Decision Spotlight will unblock downstream tasks.`;
		primaryLink = { label: 'Open Inbox', href: '/inbox' };
		secondaryLink = { label: 'Open Work', href: '/work' };
	}

	return {
		totalOpen,
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
	nowMs = Date.now()
): CatchUpRow[] {
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
			href: item.kind === 'request' ? `/work?request=${encodeURIComponent(item.id)}` : '/work'
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
				? `${activeAgents.slice(0, 3).map((a) => a.name).join(', ')}${activeAgents.length > 3 ? ` +${activeAgents.length - 3} more` : ''} standing by across ${(snapshot.projects ?? []).length} projects.`
				: 'Hire or place an agent in Offices to start autonomous delivery.',
		timeLabel: 'Live',
		tone: 'live',
		href: '/agents'
	});

	return rows;
}

export interface ExecutiveBriefSection {
	id: string;
	kicker: string;
	tone: 'ok' | 'warn' | 'accent';
	body: string;
}

export interface ExecutiveBrief {
	sections: ExecutiveBriefSection[];
	copyText: string;
	updatedLabel: string;
}

export function deriveExecutiveBrief(
	snapshot: OrgSnapshot,
	items: readonly WorkItemSummary[],
	offices: readonly OfficeInfo[],
	ownerId: string,
	organizationName = 'Organization',
	nowMs = Date.now()
): ExecutiveBrief {
	const kpis = deriveKpiMetrics(snapshot, items, offices, ownerId);
	const spotlight = deriveDecisionSpotlight(items, snapshot, ownerId, nowMs);
	const projects = deriveProjectCards(snapshot, items);
	const attentionProjects = projects.filter((p) => p.health !== 'On track');

	const liveBody =
		kpis.totalAgents > 0
			? `${kpis.activeAgents} of ${kpis.totalAgents} agents are active across ${kpis.projectsTotal} ${kpis.projectsTotal === 1 ? 'project' : 'projects'} (${kpis.autonomyPct}% autonomous execution). ${kpis.doneCount} ${kpis.doneCount === 1 ? 'item' : 'items'} completed with ${kpis.completionPct}% overall progress and ${kpis.firstTryPassPct}% clean delivery.`
			: `${kpis.openCount} open ${kpis.openCount === 1 ? 'item' : 'items'} and ${kpis.doneCount} completed across ${kpis.projectsTotal} ${kpis.projectsTotal === 1 ? 'project' : 'projects'}. Add or activate agents to increase autonomous delivery.`;

	const riskParts: string[] = [];
	if (kpis.offlineOffices > 0) {
		riskParts.push(
			`${kpis.offlineOffices} ${kpis.offlineOffices === 1 ? 'office is' : 'offices are'} offline (${kpis.offlineOfficeNames.join(', ')}).`
		);
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

	const queueBody =
		spotlight.length > 0
			? `${spotlight.length} ${spotlight.length === 1 ? 'decision is' : 'decisions are'} queued for your review, led by “${spotlight[0].title}” (${spotlight[0].kindLabel} · ${spotlight[0].projectName}).`
			: 'Your decision queue is clear — nothing is waiting on owner sign-off right now.';

	const sections: ExecutiveBriefSection[] = [
		{
			id: 'live',
			kicker: 'Live Status & Highlights',
			tone: 'ok',
			body: liveBody
		},
		{
			id: 'problems',
			kicker: 'Problems Needing Attention',
			tone: riskParts.length > 0 ? 'warn' : 'ok',
			body: problemsBody
		},
		{
			id: 'queue',
			kicker: 'Queued For You',
			tone: 'accent',
			body: queueBody
		}
	];

	const copyText = [
		`Executive Brief — ${organizationName}`,
		...sections.map((s) => `• ${s.kicker}: ${s.body}`)
	].join('\n');

	return {
		sections,
		copyText,
		updatedLabel: 'Live summary'
	};
}

export function answerExecutiveQuestion(
	rawQuery: string,
	{
		snapshot,
		items,
		offices,
		ownerId
	}: {
		snapshot: OrgSnapshot;
		items: readonly WorkItemSummary[];
		offices: readonly OfficeInfo[];
		ownerId: string;
	}
): OrgAiAnswer {
	const query = rawQuery.trim();
	const q = query.toLowerCase();
	const kpis = deriveKpiMetrics(snapshot, items, offices, ownerId);
	const spotlight = deriveDecisionSpotlight(items, snapshot, ownerId);
	const projects = deriveProjectCards(snapshot, items);
	const time = deriveTaskTimeBreakdown(items, ownerId, kpis.offlineOffices);

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
					: [{ text: 'Your team is running autonomously.', href: '/work', linkLabel: 'View all work' }]
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
				href: `/projects/${p.id}`,
				linkLabel: 'Open project'
			}))
		};
	}

	if (/office|team|agent|idle|offline|online|capacity/.test(q)) {
		return {
			query,
			headline: `${kpis.activeAgents} of ${kpis.totalAgents} agents active · ${kpis.onlineOffices} online, ${kpis.offlineOffices} offline offices.`,
			bullets: [
				{
					text: `Agents: ${kpis.activeAgents} active, ${kpis.pausedAgents} paused, ${kpis.unplacedAgents} unplaced.`,
					href: '/agents',
					linkLabel: 'Open Agents'
				},
				{
					text:
						kpis.offlineOffices > 0
							? `Offline offices: ${kpis.offlineOfficeNames.join(', ')}.`
							: 'All connected offices are online.',
					href: '/offices',
					linkLabel: 'Open Offices'
				},
				{
					text: `${kpis.peopleCount} human teammates across ${snapshot.teams.length} teams.`,
					href: '/org-chart',
					linkLabel: 'Open Org chart'
				}
			]
		};
	}

	return {
		query,
		headline: `${kpis.openCount} open items (${kpis.urgentCount} needing you), ${kpis.projectsTotal} projects, ${kpis.onlineOffices} online office${kpis.onlineOffices === 1 ? '' : 's'}.`,
		bullets: [
			{
				text: `Task time: ${time.workingPct}% active, ${time.waitingYouPct}% waiting on you, ${time.queuedPct}% waiting for an agent.`,
				href: '/work',
				linkLabel: 'Open Work'
			},
			{
				text:
					kpis.projectsAttention > 0
						? `${kpis.projectsAttention} project${kpis.projectsAttention === 1 ? '' : 's'} need attention.`
						: 'All active projects are on track.',
				href: '/projects',
				linkLabel: 'Open Projects'
			}
		]
	};
}
