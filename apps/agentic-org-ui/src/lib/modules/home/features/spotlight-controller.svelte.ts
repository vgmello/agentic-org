import type { OrgSnapshot } from '$lib/proto';
import { toAppError } from '$lib/proto';
import { inboxApi } from '$lib/modules/inbox/api';
import { executeDecisionAction } from '$lib/modules/inbox/decisions';
import { isDenyAction } from '$lib/modules/inbox/spotlight';
import type { InboxItem } from '$lib/modules/inbox/types';
import type { SpotlightDecision } from '$lib/modules/home/model';

export interface SpotlightReceipt {
	outcome: string;
	note?: string;
}

export interface SpotlightSessionReceiptItem {
	title: string;
	outcome: string;
	note?: string;
}

export interface SpotlightControllerDeps {
	getOrgId: () => string;
	actReply?: (organizationId: string, itemId: string, text: string) => Promise<InboxItem>;
	executeAction?: (
		organizationId: string,
		decision: SpotlightDecision,
		kind: 'primary' | 'secondary',
		snapshot: OrgSnapshot,
		declineReason: string
	) => Promise<{ message: string; navigateTo?: string }>;
	navigate: (href: string) => Promise<void>;
	notifySuccess: (message: string) => void;
	notifyError: (message: string) => void;
	refresh: () => Promise<void>;
}

const EMPTY_RECEIPTS: Readonly<Record<string, SpotlightReceipt>> = Object.freeze({});

export function outcomeLabelFor(
	decision: SpotlightDecision,
	kind: 'primary' | 'secondary'
): string {
	if (kind === 'secondary') {
		return isDenyAction(decision.secondaryAction)
			? 'Denied'
			: (decision.secondaryAction?.label ?? 'Handled');
	}
	if (decision.primaryAction.kind === 'approve_inbox') return 'Approved';
	if (decision.primaryAction.kind === 'choose_inbox') {
		return `Chose: ${decision.primaryAction.label}`;
	}
	return decision.primaryAction.label;
}

/**
 * Stateful async action & receipt controller for Home Decision Spotlight.
 * Scopes busy state and receipts per organizationId with monotonic sequence + dispose guards
 * so org switches or component unmount mid-flight never leak state, toasts, navigation, or refreshes.
 */
export class SpotlightController {
	busyAction = $state.raw<{ orgId: string; decisionId: string; seq: number } | undefined>(
		undefined
	);
	#receiptsByOrg = $state.raw<Record<string, Record<string, SpotlightReceipt>>>({});
	#lastReceiptByOrg = $state.raw<Record<string, SpotlightSessionReceiptItem | undefined>>({});
	#handledCountByOrg = $state.raw<Record<string, number>>({});

	#deps: SpotlightControllerDeps;
	#actionSeq = 0;
	#destroyed = false;

	constructor(deps: SpotlightControllerDeps) {
		this.#deps = deps;
	}

	get destroyed(): boolean {
		return this.#destroyed;
	}

	get handledReceipts(): Readonly<Record<string, SpotlightReceipt>> {
		return this.#receiptsByOrg[this.#deps.getOrgId()] ?? EMPTY_RECEIPTS;
	}

	get lastReceipt(): SpotlightSessionReceiptItem | undefined {
		return this.#lastReceiptByOrg[this.#deps.getOrgId()];
	}

	get sessionHandledCount(): number {
		return this.#handledCountByOrg[this.#deps.getOrgId()] ?? 0;
	}

	isOrgBusy(orgId = this.#deps.getOrgId()): boolean {
		return this.busyAction !== undefined && this.busyAction.orgId === orgId;
	}

	isBusyFor(decisionId: string | undefined): boolean {
		return (
			decisionId !== undefined &&
			this.busyAction !== undefined &&
			this.busyAction.orgId === this.#deps.getOrgId() &&
			this.busyAction.decisionId === decisionId
		);
	}

	dismissLastReceipt(): void {
		const orgId = this.#deps.getOrgId();
		if (!this.#lastReceiptByOrg[orgId]) return;
		this.#lastReceiptByOrg = { ...this.#lastReceiptByOrg, [orgId]: undefined };
	}

	#recordHandled(
		targetOrgId: string,
		decision: SpotlightDecision,
		outcome: string,
		note?: string
	): void {
		const trimmedNote = note?.trim() || undefined;
		const existing = this.#receiptsByOrg[targetOrgId] ?? {};
		this.#receiptsByOrg = {
			...this.#receiptsByOrg,
			[targetOrgId]: {
				...existing,
				[decision.id]: { outcome, note: trimmedNote }
			}
		};
		this.#lastReceiptByOrg = {
			...this.#lastReceiptByOrg,
			[targetOrgId]: { title: decision.title, outcome, note: trimmedNote }
		};
		this.#handledCountByOrg = {
			...this.#handledCountByOrg,
			[targetOrgId]: (this.#handledCountByOrg[targetOrgId] ?? 0) + 1
		};
	}

	async #runGuarded(
		decisionId: string,
		work: (targetOrgId: string, isActive: () => boolean) => Promise<boolean>
	): Promise<boolean> {
		if (this.#destroyed) return false;
		const targetOrgId = this.#deps.getOrgId();
		if (this.isOrgBusy(targetOrgId)) return false;
		const seq = ++this.#actionSeq;
		const isActive = () =>
			!this.#destroyed && this.#deps.getOrgId() === targetOrgId && seq === this.#actionSeq;
		this.busyAction = { orgId: targetOrgId, decisionId, seq };
		try {
			return await work(targetOrgId, isActive);
		} catch (e) {
			if (isActive()) this.#deps.notifyError(toAppError(e).message);
			return false;
		} finally {
			if (this.busyAction?.seq === seq) this.busyAction = undefined;
		}
	}

	async submitReply(
		decision: SpotlightDecision,
		rawText: string,
		onHandled?: () => void
	): Promise<boolean> {
		const inboxItem = decision.inboxItem;
		const text = rawText.trim();
		if (!inboxItem || !text) return false;
		const actReply =
			this.#deps.actReply ??
			((orgId: string, itemId: string, body: string) =>
				inboxApi.act(orgId, itemId, { type: 'reply', text: body }));

		return this.#runGuarded(decision.id, async (targetOrgId, isActive) => {
			const updated = await actReply(targetOrgId, inboxItem.id, text);
			if (!isActive()) return false;
			const outcome = updated.handled?.outcome?.trim() || 'Answered';
			this.#recordHandled(targetOrgId, decision, outcome, text);
			onHandled?.();
			this.#deps.notifySuccess(`Sent answer to ${decision.actorName}.`);
			await this.#deps.refresh();
			return isActive();
		});
	}

	async runAction(
		decision: SpotlightDecision,
		kind: 'primary' | 'secondary',
		snapshot: OrgSnapshot,
		rawReason: string,
		onHandled?: () => void
	): Promise<boolean> {
		const reason = rawReason.trim();
		const execute =
			this.#deps.executeAction ??
			((orgId, item, actionKind, snap, declineReason) =>
				executeDecisionAction(orgId, item, actionKind, snap, declineReason));

		return this.#runGuarded(decision.id, async (targetOrgId, isActive) => {
			const res = await execute(targetOrgId, decision, kind, snapshot, reason);
			if (!isActive()) return false;
			onHandled?.();
			if (res.navigateTo) {
				await this.#deps.navigate(res.navigateTo);
				return false;
			}
			const note = kind === 'secondary' ? reason : undefined;
			this.#recordHandled(targetOrgId, decision, outcomeLabelFor(decision, kind), note);
			if (res.message) this.#deps.notifySuccess(res.message);
			await this.#deps.refresh();
			return isActive();
		});
	}

	dispose(): void {
		this.#destroyed = true;
		this.busyAction = undefined;
	}
}
