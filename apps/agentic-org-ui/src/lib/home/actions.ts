import type { OrgSnapshot } from '$lib/proto';
import { inboxApi } from '$lib/modules/inbox/api';
import { newId } from '$lib/org/commands';
import { sender, workApi } from '$lib/work';
import type { SpotlightDecision } from './model';

/**
 * Execute a real decision action from Executive Home Spotlight or Inbox
 * through the owning canonical domain channel (`inboxApi` or `workApi`).
 */
export async function executeDecisionAction(
	organizationId: string,
	decision: SpotlightDecision,
	action: 'primary' | 'secondary',
	snapshot: OrgSnapshot,
	declineReason = 'Declined from executive review'
): Promise<{ message: string; navigateTo?: string }> {
	if (decision.sourceKind === 'inbox' && decision.inboxItem) {
		const inboxItem = decision.inboxItem;
		if (action === 'secondary') {
			if (decision.secondaryAction?.kind === 'deny_inbox') {
				const reason = declineReason.trim() || 'Declined from executive review';
				await inboxApi.act(organizationId, inboxItem.id, { type: 'deny', reason });
				return { message: `Denied “${inboxItem.title}”.` };
			}
			if (decision.secondaryAction?.kind === 'choose_inbox') {
				await inboxApi.act(organizationId, inboxItem.id, {
					type: 'choose',
					choice: decision.secondaryAction.choiceId
				});
				return { message: `Recorded “${decision.secondaryAction.label}”.` };
			}
			return { message: '', navigateTo: decision.detailHref };
		}

		switch (decision.primaryAction.kind) {
			case 'approve_inbox': {
				await inboxApi.act(organizationId, inboxItem.id, { type: 'approve' });
				return { message: `Approved “${inboxItem.title}”.` };
			}
			case 'choose_inbox': {
				await inboxApi.act(organizationId, inboxItem.id, {
					type: 'choose',
					choice: decision.primaryAction.choiceId
				});
				return { message: `Recorded “${decision.primaryAction.label}”.` };
			}
			case 'dismiss_inbox': {
				await inboxApi.act(organizationId, inboxItem.id, { type: 'dismiss' });
				return { message: `Dismissed “${inboxItem.title}”.` };
			}
			case 'open_inbox':
			default:
				return { message: '', navigateTo: decision.detailHref };
		}
	}

	const item = decision.item;
	if (!item) return { message: '', navigateTo: decision.detailHref };
	const send = sender(organizationId);

	if (action === 'secondary') {
		if (decision.secondaryAction?.kind === 'decline_request') {
			await send({
				method: 'request.decline',
				item_id: item.id,
				expected_version: item.version,
				reason: declineReason.trim() || 'Declined from executive review'
			});
			return { message: `Declined “${item.title}”.` };
		}
		return { message: '', navigateTo: decision.detailHref };
	}

	switch (decision.primaryAction.kind) {
		case 'approve_request': {
			const full = await workApi.get(organizationId, item.id);
			const draftTasks =
				full.outcome && full.outcome.draft_tasks.length > 0
					? full.outcome.draft_tasks.map((t) => ({
							item_id: newId(),
							link_id: newId(),
							title: t.title,
							...(t.description ? { description: t.description } : {}),
							...(t.assignee_id ? { assignee_id: t.assignee_id } : {}),
							...(t.team_id ? { team_id: t.team_id } : {}),
							...(t.priority ? { priority: t.priority } : {})
						}))
					: [
							{
								item_id: newId(),
								link_id: newId(),
								title: item.title,
								...(item.priority ? { priority: item.priority } : {})
							}
						];
			await send({
				method: 'request.approve',
				item_id: item.id,
				expected_version: item.version,
				tasks: draftTasks
			});
			return { message: `Approved “${item.title}”.` };
		}
		case 'verify_bug': {
			await send({
				method: 'bug.verify',
				item_id: item.id,
				expected_version: item.version,
				outcome: 'fixed'
			});
			return { message: `Verified and closed “${item.title}”.` };
		}
		case 'unblock_task': {
			await send({
				method: 'task.set_status',
				item_id: item.id,
				expected_version: item.version,
				status: 'in_progress'
			});
			return { message: `Resumed “${item.title}”.` };
		}
		case 'triage_bug': {
			const area =
				(snapshot.areas ?? []).find((a) => !a.deleted && a.project_id === item.project_id) ??
				(snapshot.areas ?? []).find((a) => !a.deleted);
			if (!area) return { message: '', navigateTo: decision.detailHref };
			const sev = item.severity && item.severity !== 'unknown' ? item.severity : 'minor';
			await send({
				method: 'bug.triage',
				item_id: item.id,
				expected_version: item.version,
				area_id: area.id,
				severity: sev,
				priority: item.priority && item.priority !== 'unknown' ? item.priority : 'normal'
			});
			return { message: `Triaged “${item.title}”.` };
		}
		default:
			return { message: '', navigateTo: decision.detailHref };
	}
}
