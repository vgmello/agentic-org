import type { ShellRefreshResult } from '$lib/platform/shell/shell-data.svelte';
import type { OrgSnapshot } from '$lib/proto';
import { inboxApi } from '../../api';
import { executeDecisionAction } from '../../decisions';
import { isDenyAction } from '../../spotlight';
import { formatSnoozeReturn } from './snooze-presets';
import type { TriageItem, TriageTab } from './triage-model';

export interface TriageUrlPatch {
	tab?: TriageTab | 'notifications';
	item?: string | null;
	team?: string | null;
	project?: string | null;
}

export function buildNextTriageUrl(
	baseUrl: URL,
	defaultTab: TriageTab | 'notifications',
	patch: TriageUrlPatch
): URL {
	const url = new URL(baseUrl);
	const nextTab = patch.tab ?? defaultTab;
	if (nextTab === 'all') url.searchParams.delete('tab');
	else url.searchParams.set('tab', nextTab);

	for (const key of ['item', 'team', 'project'] as const) {
		if (!(key in patch)) continue;
		const val = patch[key];
		if (val) url.searchParams.set(key, val);
		else url.searchParams.delete(key);
	}
	return url;
}

export type TriageActionOutcome =
	| { kind: 'navigate'; href: string }
	| { kind: 'advance'; message: string; clearDrafts: boolean }
	| { kind: 'refresh'; message: string }
	| { kind: 'none' };

export function canDenyTriageItem(item: TriageItem): boolean {
	return Boolean(
		item.inboxItem?.actions.includes('deny') || isDenyAction(item.decision?.secondaryAction)
	);
}

export async function runPrimaryTriageMutation(
	orgId: string,
	item: TriageItem,
	snapshot: OrgSnapshot,
	reason: string
): Promise<TriageActionOutcome> {
	if (!item.decision) return { kind: 'none' };
	const res = await executeDecisionAction(orgId, item.decision, 'primary', snapshot, reason);
	if (res.navigateTo) return { kind: 'navigate', href: res.navigateTo };
	return { kind: 'advance', message: res.message, clearDrafts: true };
}

export async function runSecondaryTriageMutation(
	orgId: string,
	item: TriageItem,
	snapshot: OrgSnapshot,
	reason: string
): Promise<TriageActionOutcome> {
	if (item.decision && isDenyAction(item.decision.secondaryAction)) {
		const res = await executeDecisionAction(orgId, item.decision, 'secondary', snapshot, reason);
		if (res.navigateTo) return { kind: 'navigate', href: res.navigateTo };
		return { kind: 'advance', message: res.message, clearDrafts: true };
	}
	if (item.inboxItem?.actions.includes('deny')) {
		const finalReason = reason.trim() || 'Declined from executive review';
		await inboxApi.act(orgId, item.inboxItem.id, { type: 'deny', reason: finalReason });
		return {
			kind: 'advance',
			message: `Denied “${item.inboxItem.title}”.`,
			clearDrafts: true
		};
	}
	return { kind: 'none' };
}

export async function runChooseTriageMutation(
	orgId: string,
	inboxId: string,
	choiceId: string,
	choiceLabel: string
): Promise<TriageActionOutcome> {
	await inboxApi.act(orgId, inboxId, { type: 'choose', choice: choiceId });
	return { kind: 'advance', message: `Recorded “${choiceLabel}”.`, clearDrafts: true };
}

export async function runReplyTriageMutation(
	orgId: string,
	inboxId: string,
	actorName: string,
	text: string
): Promise<TriageActionOutcome> {
	await inboxApi.act(orgId, inboxId, { type: 'reply', text: text.trim() });
	return { kind: 'advance', message: `Sent reply to ${actorName}.`, clearDrafts: true };
}

export async function runDeferTriageMutation(
	orgId: string,
	item: TriageItem,
	untilIso: string,
	zone: string,
	note?: string
): Promise<TriageActionOutcome> {
	if (!item.inboxItem) return { kind: 'none' };
	const wasSnoozed = item.state === 'snoozed';
	await inboxApi.defer(orgId, item.inboxItem.id, untilIso, note);
	const shortLabel = formatSnoozeReturn(untilIso, new Date(), zone).short;
	if (wasSnoozed) return { kind: 'refresh', message: `Snooze updated to ${shortLabel}.` };
	return { kind: 'advance', message: `Snoozed until ${shortLabel}.`, clearDrafts: false };
}

export async function runUnsnoozeTriageMutation(
	orgId: string,
	inboxId: string,
	title: string
): Promise<TriageActionOutcome> {
	await inboxApi.unsnooze(orgId, inboxId);
	return { kind: 'advance', message: `Returned “${title}” to Inbox.`, clearDrafts: false };
}

export function isFutureIsoTimestamp(iso: string): boolean {
	const untilMs = Date.parse(iso);
	return !Number.isNaN(untilMs) && untilMs > Date.now();
}

export function isTriageSourceFresh(
	kind: TriageItem['sourceKind'],
	refreshed: ShellRefreshResult
): boolean {
	return kind === 'inbox' ? refreshed.inboxUpdated : refreshed.workUpdated;
}

export function selectNextVisibleAfterResolve(
	visibleItems: readonly TriageItem[],
	resolvedId: string,
	candidateIds: readonly string[],
	refreshed: ShellRefreshResult,
	wideViewport: boolean
): string | undefined {
	const surviving = visibleItems.filter(
		(i) => i.id !== resolvedId && isTriageSourceFresh(i.sourceKind, refreshed)
	);
	const refreshedIds = new Set(surviving.map((i) => i.id));
	return (
		candidateIds.find((id) => refreshedIds.has(id)) ?? (wideViewport ? surviving[0]?.id : undefined)
	);
}

export function deleteDraftKey(
	drafts: Record<string, string>,
	key: string
): Record<string, string> | undefined {
	if (!(key in drafts)) return undefined;
	const next = { ...drafts };
	delete next[key];
	return next;
}

export function focusElementFallback(...candidates: (HTMLElement | null | undefined)[]): void {
	for (const el of candidates) {
		if (el && el.isConnected) {
			el.focus();
			return;
		}
	}
}

export type TriageActionResult = { ok: true } | { ok: false; error: string };

export interface TriageActionDeps {
	isLocked: () => boolean;
	getOrgId: () => string;
	getSnapshot: () => OrgSnapshot;
	getZone: () => string;
	getDecliningId: () => string | undefined;
	setDecliningId: (id: string | undefined) => void;
	getDenyDraft: (orgId: string, itemId: string) => string;
	onError: (message: string) => void;
	runItemAction: (
		item: TriageItem,
		execute: (targetOrg: string) => Promise<TriageActionOutcome>,
		formatError?: (msg: string) => string
	) => Promise<TriageActionResult>;
	formatSnoozeError: (msg: string) => string;
}

export function createTriageActionHandlers(deps: TriageActionDeps) {
	return {
		async primary(item: TriageItem): Promise<void> {
			if (deps.isLocked() || !item.decision) return;
			const reason = deps.getDenyDraft(deps.getOrgId(), item.id);
			await deps.runItemAction(item, (orgId) =>
				runPrimaryTriageMutation(orgId, item, deps.getSnapshot(), reason)
			);
		},
		async secondary(item: TriageItem): Promise<void> {
			if (deps.isLocked()) return;
			if (canDenyTriageItem(item) && deps.getDecliningId() !== item.id) {
				deps.setDecliningId(item.id);
				return;
			}
			const reason = deps.getDenyDraft(deps.getOrgId(), item.id);
			await deps.runItemAction(item, (orgId) =>
				runSecondaryTriageMutation(orgId, item, deps.getSnapshot(), reason)
			);
		},
		async choose(item: TriageItem, choiceId: string, choiceLabel: string): Promise<void> {
			if (deps.isLocked() || !item.inboxItem) return;
			const inboxId = item.inboxItem.id;
			await deps.runItemAction(item, (orgId) =>
				runChooseTriageMutation(orgId, inboxId, choiceId, choiceLabel)
			);
		},
		async reply(item: TriageItem, text: string): Promise<void> {
			if (deps.isLocked() || !item.inboxItem || !text.trim()) return;
			const inboxId = item.inboxItem.id;
			await deps.runItemAction(item, (orgId) =>
				runReplyTriageMutation(orgId, inboxId, item.actorName, text)
			);
		},
		async defer(item: TriageItem, untilIso: string, note?: string): Promise<TriageActionResult> {
			if (deps.isLocked() || !item.inboxItem) {
				return { ok: false, error: 'Action already in progress.' };
			}
			if (!isFutureIsoTimestamp(untilIso)) {
				const msg = 'Return time must be in the future.';
				deps.onError(msg);
				return { ok: false, error: msg };
			}
			return deps.runItemAction(
				item,
				(orgId) => runDeferTriageMutation(orgId, item, untilIso, deps.getZone(), note),
				deps.formatSnoozeError
			);
		},
		async unsnooze(item: TriageItem): Promise<void> {
			if (deps.isLocked() || !item.inboxItem) return;
			const inboxId = item.inboxItem.id;
			await deps.runItemAction(item, (orgId) =>
				runUnsnoozeTriageMutation(orgId, inboxId, item.title)
			);
		}
	};
}
