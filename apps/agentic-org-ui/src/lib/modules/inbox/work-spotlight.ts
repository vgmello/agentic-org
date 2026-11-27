import { badgeInitials, relativeAge } from '$lib/format';
import { memberById } from '$lib/modules/organization/model';
import type { Member, OrgSnapshot, WorkItemSummary } from '$lib/proto';
import type { InboxItem } from './types';

export type SpotlightSignalTone = 'ok' | 'warn' | 'bad' | 'sky' | 'muted';

export interface SpotlightCell {
	label: string;
	sub: string;
	tone: SpotlightSignalTone;
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

export function recRiskLabelFor(tone: SpotlightDecision['recTone']): string | undefined {
	if (tone === 'ok') return 'Low risk';
	if (tone === 'warn') return 'Investigate';
	if (tone === 'bad') return 'Use caution';
	return undefined;
}

type WorkSpotlightKind = 'request' | 'fixed_bug' | 'new_bug' | 'blocked_task';

function classifyWorkSpotlightKind(item: WorkItemSummary): WorkSpotlightKind {
	if (item.kind === 'request') return 'request';
	if (item.kind === 'bug') return item.status === 'fixed' ? 'fixed_bug' : 'new_bug';
	return 'blocked_task';
}

function workSpotlightCopy(
	kind: WorkSpotlightKind,
	item: WorkItemSummary,
	actorName: string,
	projectName: string,
	urgent: boolean
): { kindLabel: string; badgeText: string; recommendation: string; backgroundContext: string } {
	const kindLabels: Record<WorkSpotlightKind, string> = {
		request: 'Request sign-off',
		fixed_bug: 'Bug verification',
		new_bug: 'Bug triage',
		blocked_task: 'Blocked task'
	};
	const kindLabel = kindLabels[kind];
	const badgeText = urgent
		? item.severity === 'critical'
			? 'Critical · needs attention'
			: 'Priority decision'
		: kindLabel;

	if (kind === 'request') {
		return {
			kindLabel,
			badgeText,
			recommendation: `${actorName} scoped this request. Approving turns it into active work for the team.`,
			backgroundContext: `${item.short_id} in ${projectName} is scoped and ready for approval. Approving converts the scoped outcome into executable work items assigned to the team; denying returns it with your note.`
		};
	}
	if (kind === 'fixed_bug') {
		return {
			kindLabel,
			badgeText,
			recommendation: `${actorName} marked this fix complete. Verify whether it resolves the bug; close it separately on the bug page.`,
			backgroundContext: `${item.short_id} (${item.severity ?? 'reported'} severity) in ${projectName} was marked fixed by ${actorName}. Verifying records your check and clears pending verification; close the bug on its page once confirmed.`
		};
	}
	if (kind === 'new_bug') {
		return {
			kindLabel,
			badgeText,
			recommendation: `New ${item.severity ?? 'reported'} bug waiting for triage and owner confirmation.`,
			backgroundContext: `${item.short_id} was reported in ${projectName}${item.severity ? ` with ${item.severity} severity` : ''}. Triaging routes it to the responsible area and sets priority for an agent or owner.`
		};
	}
	return {
		kindLabel,
		badgeText,
		recommendation: item.blocked_reason
			? `Blocked: ${item.blocked_reason}`
			: `${actorName} is waiting on your input to unblock this task.`,
		backgroundContext: `${item.short_id} in ${projectName} is currently blocked${item.blocked_reason ? ` (${item.blocked_reason})` : ''}. Resuming moves it back to in-progress so ${actorName} can continue.`
	};
}

function workSpotlightProof(kind: WorkSpotlightKind, item: WorkItemSummary): SpotlightCell {
	if (item.checklist.total === 0) {
		return {
			label: kind === 'fixed_bug' ? 'Fix ready' : 'Ready to review',
			sub: item.short_id,
			tone: kind === 'fixed_bug' ? 'ok' : 'muted',
			icon: 'clock'
		};
	}
	const allRequiredDone = item.checklist.required_left === 0;
	return {
		label: allRequiredDone
			? 'Checks passed'
			: `${item.checklist.done}/${item.checklist.total} steps`,
		sub: allRequiredDone
			? `${item.checklist.done} of ${item.checklist.total} complete`
			: `${item.checklist.required_left} required left`,
		tone: allRequiredDone ? 'ok' : 'warn',
		icon: 'check-circle'
	};
}

function workSpotlightRecTone(
	kind: WorkSpotlightKind,
	item: WorkItemSummary,
	urgent: boolean
): SpotlightDecision['recTone'] {
	if (item.severity === 'critical') return 'bad';
	if (urgent || kind === 'new_bug') return 'warn';
	if (kind === 'fixed_bug' || kind === 'request') return 'ok';
	return 'neutral';
}

function workSpotlightSignals(
	kind: WorkSpotlightKind,
	item: WorkItemSummary,
	projectName: string,
	urgent: boolean
): {
	risk: SpotlightCell;
	impact: SpotlightCell;
	proof: SpotlightCell;
	recTone: SpotlightDecision['recTone'];
} {
	const highRisk = item.severity === 'critical' || item.priority === 'urgent';
	const risk: SpotlightCell = highRisk
		? { label: 'Moderate risk', sub: 'Time-sensitive item', tone: 'warn', icon: 'shield-alert' }
		: { label: 'Low risk', sub: 'Reversible decision', tone: 'ok', icon: 'shield-check' };

	const impact: SpotlightCell =
		urgent || kind === 'request'
			? { label: 'High impact', sub: `Unblocks ${projectName}`, tone: 'sky', icon: 'bolt' }
			: { label: 'Medium impact', sub: `Keeps ${projectName} moving`, tone: 'sky', icon: 'scale' };

	return {
		risk,
		impact,
		proof: workSpotlightProof(kind, item),
		recTone: workSpotlightRecTone(kind, item, urgent)
	};
}

function workSpotlightActions(kind: WorkSpotlightKind): {
	primaryAction: SpotlightPrimaryAction;
	secondaryAction: SpotlightSecondaryAction;
} {
	const primaryByKind: Record<WorkSpotlightKind, SpotlightPrimaryAction> = {
		request: { kind: 'approve_request', label: 'Approve request' },
		fixed_bug: { kind: 'verify_bug', label: 'Verify fix' },
		new_bug: { kind: 'triage_bug', label: 'Triage bug' },
		blocked_task: { kind: 'unblock_task', label: 'Resume task' }
	};
	return {
		primaryAction: primaryByKind[kind],
		secondaryAction:
			kind === 'request'
				? { kind: 'decline_request', label: 'Deny' }
				: { kind: 'open_work', label: 'Inspect' }
	};
}

function workSpotlightAudit(item: WorkItemSummary): string[] {
	const checks: string[] = [];
	if (item.checklist.total > 0) {
		checks.push(`${item.checklist.done}/${item.checklist.total} checklist steps completed`);
	}
	checks.push(`Ref ${item.short_id}`);
	if (item.priority) checks.push(`Priority: ${item.priority}`);
	if (item.severity) checks.push(`Severity: ${item.severity}`);
	return checks;
}

export function workItemToSpotlight(
	item: WorkItemSummary,
	snapshot: OrgSnapshot,
	projects: ReadonlyMap<string, { name: string }>,
	nowMs: number
): SpotlightDecision {
	const actor: Member | undefined =
		memberById(snapshot, item.assignee_id) ?? memberById(snapshot, item.created_by);
	const actorName = actor?.name ?? 'Team member';
	const projectName =
		(item.project_id ? projects.get(item.project_id)?.name : undefined) ?? 'Organization';
	const kind = classifyWorkSpotlightKind(item);
	const urgent =
		item.priority === 'urgent' || item.severity === 'critical' || kind === 'blocked_task';

	const copy = workSpotlightCopy(kind, item, actorName, projectName, urgent);
	const signals = workSpotlightSignals(kind, item, projectName, urgent);
	const actions = workSpotlightActions(kind);
	const detailHref =
		kind === 'request'
			? `/work?request=${encodeURIComponent(item.id)}`
			: item.project_id
				? `/projects/${encodeURIComponent(item.project_id)}/work`
				: '/work';

	return {
		id: item.id,
		sourceKind: 'work',
		category: item.kind === 'unknown' ? 'task' : item.kind,
		item,
		kindLabel: copy.kindLabel,
		urgent,
		badgeText: copy.badgeText,
		actorId: actor?.id,
		actorName,
		actorInitials: badgeInitials(actorName),
		isChiefOfStaff: actor?.agent?.role === 'chief_of_staff',
		projectName,
		projectId: item.project_id,
		title: item.title,
		recommendation: copy.recommendation,
		recTone: signals.recTone,
		recRiskLabel: recRiskLabelFor(signals.recTone),
		backgroundContext: copy.backgroundContext,
		auditChecks: workSpotlightAudit(item),
		risk: signals.risk,
		impact: signals.impact,
		proof: signals.proof,
		primaryAction: actions.primaryAction,
		secondaryAction: actions.secondaryAction,
		detailHref,
		ageLabel: relativeAge(item.updated_at, nowMs)
	};
}

export function workSpotlightScore(item: WorkItemSummary): number {
	let s = 0;
	if (item.priority === 'urgent' || item.severity === 'critical') s += 100;
	else if (item.priority === 'high' || item.severity === 'major') s += 50;
	if (item.kind === 'request' && item.status === 'scoped') s += 40;
	if (item.kind === 'bug' && item.status === 'fixed') s += 30;
	if (item.kind === 'task' && item.status === 'blocked') s += 25;
	return s;
}
