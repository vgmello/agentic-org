import { badgeInitials, relativeAge } from '$lib/format';
import { memberById } from '$lib/modules/organization/model';
import {
	familiarityInfo,
	type PendingExternalAuthView
} from '$lib/platform/workspace/external-auth';
import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
import type { InboxItem } from './types';
import {
	recRiskLabelFor,
	workItemToSpotlight,
	workSpotlightScore,
	type SpotlightCell,
	type SpotlightDecision,
	type SpotlightPrimaryAction,
	type SpotlightSecondaryAction,
	type SpotlightSignalTone
} from './work-spotlight';

export type {
	SpotlightCell,
	SpotlightDecision,
	SpotlightPrimaryAction,
	SpotlightSecondaryAction,
	SpotlightSignalTone
};

export function isDenyAction(
	action: SpotlightSecondaryAction | undefined
): action is { kind: 'deny_inbox' | 'decline_request'; label: string } {
	return action?.kind === 'deny_inbox' || action?.kind === 'decline_request';
}

function inboxPrimaryLabel(item: InboxItem): string {
	if (item.actions.includes('choose') && item.choices?.[0]) return item.choices[0].label;
	if (item.actions.includes('approve')) return item.approve_label?.trim() || 'Approve';
	if (item.actions.includes('dismiss')) return 'Dismiss';
	return item.open?.label ?? 'Open in Inbox';
}

function isAskReplyCutoffSource(source: string | undefined): boolean {
	return source === 'Chat ask' || source === 'Chat approval';
}

function formatExpiryBadge(iso: string, nowMs: number, source?: string): string {
	const diffMs = Date.parse(iso) - nowMs;
	if (Number.isNaN(diffMs)) return 'Time-sensitive';
	const replyCutoff = isAskReplyCutoffSource(source);
	if (diffMs <= 0) return replyCutoff ? 'Past reply cutoff' : 'Expired';
	const diffMin = Math.max(1, Math.ceil(diffMs / 60_000));
	if (diffMin < 60) return replyCutoff ? `Reply cutoff in ${diffMin}m` : `Expires in ${diffMin}m`;
	const diffH = Math.round(diffMin / 60);
	if (diffH < 24) return replyCutoff ? `Reply cutoff in ${diffH}h` : `Expires in ${diffH}h`;
	return replyCutoff ? 'Reply cutoff tomorrow' : 'Expires tomorrow';
}

const RAW_UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function looksLikeRawId(name: string | undefined, id: string | undefined): boolean {
	const trimmed = name?.trim() ?? '';
	if (!trimmed) return true;
	if (id && trimmed === id) return true;
	return RAW_UUID_RE.test(trimmed);
}

function resolveDisplayLabel(
	rawName: string | undefined,
	rawId: string | undefined,
	resolvedName: string | undefined
): string | undefined {
	if (!rawName && !resolvedName) return undefined;
	return resolvedName && looksLikeRawId(rawName, rawId) ? resolvedName : rawName;
}

function resolveInboxEntityNames(item: InboxItem, snapshot: OrgSnapshot) {
	const resolvedMember = memberById(snapshot, item.who.id);
	const actorName =
		resolveDisplayLabel(item.who.name, item.who.id, resolvedMember?.name) ?? item.who.name;
	const resolvedProject = item.project?.id
		? (snapshot.projects ?? []).find((p) => p.id === item.project!.id)
		: undefined;
	const resolvedTeam = item.team?.id
		? (snapshot.teams ?? []).find((t) => t.id === item.team!.id)
		: undefined;
	const projectLabel = item.project
		? resolveDisplayLabel(item.project.name, item.project.id, resolvedProject?.name)
		: undefined;
	const teamLabel = item.team
		? resolveDisplayLabel(item.team.name, item.team.id, resolvedTeam?.name)
		: undefined;
	return {
		resolvedMember,
		actorName,
		projectLabel,
		teamLabel,
		projectName: projectLabel ?? teamLabel ?? 'Organization'
	};
}

function resolveInboxActions(item: InboxItem): {
	primaryAction: SpotlightPrimaryAction;
	secondaryAction?: SpotlightSecondaryAction;
} {
	if (item.external_auth) {
		return {
			primaryAction: { kind: 'open_inbox', label: 'Review in Inbox' },
			secondaryAction: item.open ? { kind: 'open_work', label: item.open.label } : undefined
		};
	}

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

	return { primaryAction, secondaryAction };
}

function externalAuthSignals(ea: PendingExternalAuthView): {
	recTone: SpotlightDecision['recTone'];
	recRiskLabel: string;
	risk: SpotlightCell;
	proof: SpotlightCell;
} {
	const proof: SpotlightCell = {
		label: 'Code required',
		sub: `Lookup ${ea.code_prefix}`,
		tone: 'warn',
		icon: 'clock'
	};
	if (ea.workspace_familiarity === 'returning_new_key') {
		return {
			recTone: 'bad',
			recRiskLabel: 'Use caution',
			risk: {
				label: 'High risk',
				sub: ea.managed_turn_active
					? 'New key · active managed turn'
					: 'Returning workspace with new key',
				tone: 'bad',
				icon: 'shield-alert'
			},
			proof
		};
	}
	if (ea.managed_turn_active) {
		return {
			recTone: 'warn',
			recRiskLabel: 'Takeover required',
			risk: {
				label: 'Takeover risk',
				sub: 'Active managed turn will be parked',
				tone: 'warn',
				icon: 'shield-alert'
			},
			proof
		};
	}
	if (ea.workspace_familiarity === 'returning_known_key') {
		return {
			recTone: 'neutral',
			recRiskLabel: 'Verify code',
			risk: {
				label: 'Verify code',
				sub: familiarityInfo(ea.workspace_familiarity).label,
				tone: 'muted',
				icon: 'shield-check'
			},
			proof
		};
	}
	return {
		recTone: 'warn',
		recRiskLabel: 'Investigate',
		risk: {
			label: 'Moderate risk',
			sub: familiarityInfo(ea.workspace_familiarity).label,
			tone: 'warn',
			icon: 'shield-alert'
		},
		proof
	};
}

function recToneForInbox(item: InboxItem): SpotlightDecision['recTone'] {
	if (item.external_auth) return externalAuthSignals(item.external_auth).recTone;
	if (item.kind === 'problem') return 'bad';
	if (item.kind === 'decision' || item.kind === 'question') return 'warn';
	return item.recommendation ? 'ok' : 'neutral';
}

function inboxToSpotlight(
	item: InboxItem,
	snapshot: OrgSnapshot,
	nowMs: number
): SpotlightDecision {
	const urgent = Boolean(item.expires_at) || item.kind === 'problem';
	const deadlineLabel = item.expires_at
		? formatExpiryBadge(item.expires_at, nowMs, item.source)
		: undefined;
	const badgeText = deadlineLabel ?? (item.source || 'Decision');
	const { primaryAction, secondaryAction } = resolveInboxActions(item);
	const { resolvedMember, actorName, projectLabel, teamLabel, projectName } =
		resolveInboxEntityNames(item, snapshot);
	const eaSignals = item.external_auth ? externalAuthSignals(item.external_auth) : undefined;
	const risk: SpotlightCell =
		eaSignals?.risk ??
		(item.kind === 'problem' || item.kind === 'decision'
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
				});

	const impact: SpotlightCell = {
		label: urgent || item.kind === 'approval' ? 'High impact' : 'Medium impact',
		sub: `Unblocks ${projectName}`,
		tone: 'sky',
		icon: urgent || item.kind === 'approval' ? 'bolt' : 'scale'
	};

	const proof: SpotlightCell =
		eaSignals?.proof ??
		(item.recommendation
			? { label: 'Reviewed', sub: item.source, tone: 'ok', icon: 'check-circle' }
			: { label: 'Ready to review', sub: item.source, tone: 'muted', icon: 'clock' });

	const auditChecks: string[] = [];
	if (item.source) auditChecks.push(`Source: ${item.source}`);
	if (projectLabel) auditChecks.push(`Project: ${projectLabel}`);
	if (teamLabel) auditChecks.push(`Team: ${teamLabel}`);
	if (item.open?.label) auditChecks.push(item.open.label);
	if (deadlineLabel) auditChecks.push(deadlineLabel);

	const recTone = eaSignals?.recTone ?? recToneForInbox(item);
	const recRiskLabel = eaSignals?.recRiskLabel ?? recRiskLabelFor(recTone);
	const isChiefOfStaff =
		item.who.id === 'tess' ||
		resolvedMember?.agent?.role === 'chief_of_staff' ||
		item.source.toLowerCase().includes('tess') ||
		item.source.toLowerCase().includes('chief of staff');
	const detailHref = item.external_auth
		? `/inbox?item=${encodeURIComponent(item.id)}`
		: item.open?.path && !/^\/work\/[^/?#]+/.test(item.open.path)
			? item.open.path
			: `/inbox?item=${encodeURIComponent(item.id)}`;

	return {
		id: item.id,
		sourceKind: 'inbox',
		category: item.kind,
		inboxItem: item,
		kindLabel: item.source,
		urgent,
		badgeText,
		expiresShort: deadlineLabel,
		actorId: item.who.id,
		actorName,
		actorInitials: badgeInitials(actorName),
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
		detailHref,
		ageLabel: relativeAge(item.created_at, nowMs)
	};
}

function inboxSpotlightScore(item: InboxItem): number {
	let s = 0;
	if (item.expires_at) s += 100;
	if (item.kind === 'approval') s += 60;
	else if (item.kind === 'decision') s += 50;
	else if (item.kind === 'problem') s += 35;
	else if (item.kind === 'question') s += 20;
	if (item.actions.includes('approve') || item.actions.includes('choose')) s += 15;
	return s;
}

export function deriveDecisionSpotlight(
	items: readonly WorkItemSummary[],
	snapshot: OrgSnapshot,
	ownerId: string,
	nowMs = Date.now(),
	inboxItems?: readonly InboxItem[]
): SpotlightDecision[] {
	const openInbox = (inboxItems ?? []).filter((i) => i.state === 'open');
	const inboxSpot = [...openInbox]
		.sort(
			(a, b) =>
				inboxSpotlightScore(b) - inboxSpotlightScore(a) ||
				Date.parse(b.created_at) - Date.parse(a.created_at)
		)
		.map((item) => inboxToSpotlight(item, snapshot, nowMs));

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
		return item.kind === 'task' && item.status === 'blocked' && item.assignee_id === ownerId;
	});

	const workSpot = [...candidates]
		.sort(
			(a, b) =>
				workSpotlightScore(b) - workSpotlightScore(a) ||
				Date.parse(b.updated_at) - Date.parse(a.updated_at)
		)
		.map((item) => workItemToSpotlight(item, snapshot, projects, nowMs));

	return [...inboxSpot, ...workSpot];
}
