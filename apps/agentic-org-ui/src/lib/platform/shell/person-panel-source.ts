import { membersApi } from '$lib/modules/members/api';
import { PRESENCE, headerPresence, kindWord, presenceLabel } from '$lib/modules/members/present';
import type { MemberProfile } from '$lib/modules/members/types';
import { workItemsApi } from '$lib/modules/workitems/api';
import { taskHref } from '$lib/modules/workitems/present';
import type { TaskRow } from '$lib/modules/workitems/types';
import {
	isBuiltInHandle,
	toAppError,
	type ItemStatus,
	type OrgSnapshot,
	type WorkItemSummary
} from '$lib/proto';
import type {
	PersonPanelDataSource,
	PersonPanelLoadedData,
	PersonPanelPresenceTone,
	PersonPanelProfile,
	PersonPanelWorkItem
} from '$lib/modules/shell/features/person-panel/types';

const MAX_PANEL_WORK_ITEMS = 3;

const TASK_STATUS_LABEL: Record<TaskRow['status'], string> = {
	todo: 'To do',
	in_progress: 'In progress',
	blocked: 'Blocked',
	done: 'Done',
	cancelled: 'Cancelled'
};

const CLOSED_SUMMARY_STATUSES = new Set<ItemStatus>([
	'done',
	'closed',
	'approved',
	'declined',
	'merged'
]);

function summaryStatusLabel(status: ItemStatus): string {
	if (status === 'in_progress') return 'In progress';
	if (status === 'blocked') return 'Blocked';
	if (status === 'scoped') return 'Scoped';
	if (status === 'scoping') return 'Scoping';
	if (status === 'triaged') return 'Triaged';
	if (status === 'fixed') return 'Fixed';
	return 'To do';
}

function mapPresenceTone(
	tone: 'amber' | 'indigo' | 'green' | 'gray' | 'sky'
): PersonPanelPresenceTone {
	if (tone === 'green') return 'ok';
	if (tone === 'amber') return 'warn';
	if (tone === 'indigo' || tone === 'sky') return 'accent';
	return 'muted';
}

function mapProfile(profile: MemberProfile, snapshot?: OrgSnapshot): PersonPanelProfile {
	const presence = headerPresence(profile);
	const presenceMeta = presence ? PRESENCE[presence.id] : undefined;
	const snapshotMember = snapshot?.members.find((m) => m.id === profile.id);
	const builtinKey = profile.builtinKey ?? snapshotMember?.builtin_key;
	return {
		id: profile.id,
		name: profile.name,
		kind: profile.kind,
		kindLabel: kindWord(profile.kind),
		owner: profile.owner,
		isBuiltIn: isBuiltInHandle(profile.kind, profile.handle, profile.role, builtinKey),
		role: profile.role,
		status: profile.status,
		isSelf: profile.viewer.isSelf,
		presenceLabel: presence ? presenceLabel(presence) : undefined,
		presenceTone: presenceMeta ? mapPresenceTone(presenceMeta.tone) : undefined,
		currentTaskTitle: profile.currentTask?.title,
		homeTeam: profile.homeTeam
			? { id: profile.homeTeam.id, name: profile.homeTeam.name }
			: undefined,
		profileHref: `/members/${encodeURIComponent(profile.id)}`,
		workTabHref: `/members/${encodeURIComponent(profile.id)}/work`
	};
}

function mapWorkItem(item: {
	id: string;
	ref: string;
	title: string;
	status: TaskRow['status'];
}): PersonPanelWorkItem {
	return {
		id: item.id,
		ref: item.ref,
		title: item.title,
		statusLabel: TASK_STATUS_LABEL[item.status],
		href: taskHref(item.ref)
	};
}

function mapShellSummaryWork(
	workItems: readonly WorkItemSummary[],
	memberId: string
): { openWork: readonly PersonPanelWorkItem[]; totalOpenWork: number } {
	const openRows = workItems.filter(
		(item) =>
			!item.deleted && item.assignee_id === memberId && !CLOSED_SUMMARY_STATUSES.has(item.status)
	);
	return {
		totalOpenWork: openRows.length,
		openWork: openRows.slice(0, MAX_PANEL_WORK_ITEMS).map((item) => ({
			id: item.id,
			ref: item.short_id,
			title: item.title,
			statusLabel: summaryStatusLabel(item.status),
			href: taskHref(item.short_id)
		}))
	};
}

function isAbortError(error: unknown): boolean {
	return (
		(error instanceof DOMException && error.name === 'AbortError') ||
		(error instanceof Error && error.name === 'AbortError')
	);
}

function abortException(): DOMException {
	return new DOMException('Aborted', 'AbortError');
}

export interface PersonPanelShellContext {
	organizationId?: string;
	snapshot?: OrgSnapshot;
	workItems?: readonly WorkItemSummary[];
	workLoaded?: boolean;
}

interface OpenWorkResult {
	openWork: readonly PersonPanelWorkItem[];
	totalOpenWork: number;
}

async function fetchFallbackOpenWork(
	organizationId: string,
	memberId: string,
	signal: AbortSignal
): Promise<OpenWorkResult> {
	try {
		const queue = await workItemsApi.queue(organizationId, { kind: 'organization' }, signal);
		if (signal.aborted) throw abortException();
		const openRows = queue.rows.filter(
			(item) =>
				item.assignee?.id === memberId && item.status !== 'done' && item.status !== 'cancelled'
		);
		return {
			totalOpenWork: openRows.length,
			openWork: openRows.slice(0, MAX_PANEL_WORK_ITEMS).map(mapWorkItem)
		};
	} catch (workErr) {
		if (signal.aborted || isAbortError(workErr)) throw workErr;
		// Open work list is supplementary; keep profile visible if work queue fails.
		return { openWork: [], totalOpenWork: 0 };
	}
}

async function resolveOpenWork(
	organizationId: string,
	memberId: string,
	profile: MemberProfile,
	ctx: PersonPanelShellContext | undefined,
	signal: AbortSignal
): Promise<OpenWorkResult> {
	if (profile.openWork) {
		return {
			totalOpenWork: profile.openWork.total,
			openWork: profile.openWork.items.slice(0, MAX_PANEL_WORK_ITEMS).map(mapWorkItem)
		};
	}
	if (ctx?.workLoaded && ctx.organizationId === organizationId && ctx.workItems) {
		return mapShellSummaryWork(ctx.workItems, memberId);
	}
	return fetchFallbackOpenWork(organizationId, memberId, signal);
}

/**
 * Lazy shell adapter calling `membersApi.profile` (reusing `profile.openWork` or hydrated
 * `shellData.workItems` before falling back to `workItemsApi.queue`) with end-to-end `AbortSignal`.
 */
export function createPersonPanelDataSource(
	getContext?: () => PersonPanelShellContext
): PersonPanelDataSource {
	return {
		async loadMember(
			organizationId: string,
			memberId: string,
			signal: AbortSignal
		): Promise<PersonPanelLoadedData> {
			if (signal.aborted) throw abortException();
			const ctx = getContext?.();
			try {
				const profile = await membersApi.profile(organizationId, memberId, signal);
				if (signal.aborted) throw abortException();
				const { openWork, totalOpenWork } = await resolveOpenWork(
					organizationId,
					memberId,
					profile,
					ctx,
					signal
				);
				return {
					profile: mapProfile(profile, ctx?.snapshot),
					openWork,
					totalOpenWork
				};
			} catch (err) {
				if (signal.aborted || isAbortError(err)) {
					throw isAbortError(err) ? err : abortException();
				}
				return {
					openWork: [],
					totalOpenWork: 0,
					error: toAppError(err).message || 'Could not load profile.'
				};
			}
		}
	};
}

export const personPanelDataSource = createPersonPanelDataSource();
