import { membersApi } from '$lib/modules/members/api';
import { PRESENCE, headerPresence, kindWord, presenceLabel } from '$lib/modules/members/present';
import type { MemberProfile } from '$lib/modules/members/types';
import { workItemsApi } from '$lib/modules/workitems/api';
import { taskHref } from '$lib/modules/workitems/present';
import type { TaskRow } from '$lib/modules/workitems/types';
import { isBuiltInHandle } from '$lib/proto';
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

function mapPresenceTone(
	tone: 'amber' | 'indigo' | 'green' | 'gray' | 'sky'
): PersonPanelPresenceTone {
	if (tone === 'green') return 'ok';
	if (tone === 'amber') return 'warn';
	if (tone === 'indigo' || tone === 'sky') return 'accent';
	return 'muted';
}

function mapProfile(profile: MemberProfile): PersonPanelProfile {
	const presence = headerPresence(profile);
	const presenceMeta = presence ? PRESENCE[presence.id] : undefined;
	return {
		id: profile.id,
		name: profile.name,
		kind: profile.kind,
		kindLabel: kindWord(profile.kind),
		owner: profile.owner,
		isBuiltIn: isBuiltInHandle(profile.kind, profile.handle, profile.role),
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

function mapWorkItem(item: TaskRow): PersonPanelWorkItem {
	return {
		id: item.id,
		ref: item.ref,
		title: item.title,
		statusLabel: TASK_STATUS_LABEL[item.status],
		href: taskHref(item.ref)
	};
}

function isAbortError(error: unknown): boolean {
	return (
		(error instanceof DOMException && error.name === 'AbortError') ||
		(error instanceof Error && error.name === 'AbortError')
	);
}

/**
 * Lazy shell adapter that calls `membersApi.profile` and `workItemsApi.queue` through public
 * non-UI module barrels while preserving `AbortSignal` end-to-end.
 */
export const personPanelDataSource: PersonPanelDataSource = {
	async loadMember(
		organizationId: string,
		memberId: string,
		signal: AbortSignal
	): Promise<PersonPanelLoadedData> {
		const [profileResult, workResult] = await Promise.allSettled([
			membersApi.profile(organizationId, memberId, signal),
			workItemsApi.queue(organizationId, { kind: 'organization' }, signal)
		]);

		if (signal.aborted) {
			return { openWork: [], totalOpenWork: 0 };
		}

		let openWork: readonly PersonPanelWorkItem[] = [];
		let totalOpenWork = 0;
		if (workResult.status === 'fulfilled') {
			const openRows = workResult.value.rows.filter(
				(item) =>
					item.assignee?.id === memberId && item.status !== 'done' && item.status !== 'cancelled'
			);
			totalOpenWork = openRows.length;
			openWork = openRows.slice(0, MAX_PANEL_WORK_ITEMS).map(mapWorkItem);
		}

		if (profileResult.status === 'fulfilled') {
			return {
				profile: mapProfile(profileResult.value),
				openWork,
				totalOpenWork
			};
		}

		if (isAbortError(profileResult.reason)) {
			return { openWork, totalOpenWork };
		}

		const err = profileResult.reason;
		return {
			openWork,
			totalOpenWork,
			error: err instanceof Error && err.message ? err.message : 'Could not load profile.'
		};
	}
};
