<script lang="ts">
	import { tick } from 'svelte';
	import { resolve } from '$app/paths';
	import Ban from '@lucide/svelte/icons/ban';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Clock from '@lucide/svelte/icons/clock';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
	import Send from '@lucide/svelte/icons/send';
	import { isDenyAction } from '../../spotlight';
	import InboxDetailDenyGuard from './InboxDetailDenyGuard.svelte';
	import InboxDetailRecommendation from './InboxDetailRecommendation.svelte';
	import InboxSnoozeDropdown from './InboxSnoozeDropdown.svelte';
	import { isDeniedOutcome, splitHref, type TriageItem } from './triage-model';

	let {
		item,
		isBusy,
		controlsDisabled,
		shortcutsEnabled,
		isDenying,
		declineReason,
		replyText,
		zone,
		nowMs,
		snoozeReturn,
		isAskReplyCutoff,
		denyInputEl = $bindable(null),
		onDeclineReasonChange,
		onReplyTextChange,
		onPrimaryAction,
		onSecondaryAction,
		onArmDeny,
		onCancelDeny,
		onChoose,
		onReply,
		onDefer,
		onOpenCustomSnooze,
		onUnsnooze
	}: {
		item: TriageItem;
		isBusy: boolean;
		controlsDisabled: boolean;
		shortcutsEnabled: boolean;
		isDenying: boolean;
		declineReason: string;
		replyText: string;
		zone: string;
		nowMs: number;
		snoozeReturn?: { short: string; full: string };
		isAskReplyCutoff: boolean;
		denyInputEl?: HTMLInputElement | null;
		onDeclineReasonChange: (itemId: string, value: string) => void;
		onReplyTextChange: (itemId: string, value: string) => void;
		onPrimaryAction: (item: TriageItem) => void;
		onSecondaryAction: (item: TriageItem) => void;
		onArmDeny: (id: string) => void;
		onCancelDeny: () => void;
		onChoose: (item: TriageItem, choiceId: string, choiceLabel: string) => void;
		onReply: (item: TriageItem, text: string) => void;
		onDefer: (item: TriageItem, untilIso: string, note?: string) => void;
		onOpenCustomSnooze: (item: TriageItem, from: HTMLElement | null) => void;
		onUnsnooze: (item: TriageItem) => void;
	} = $props();

	let denyBtnEl = $state<HTMLButtonElement | null>(null);

	const openParts = $derived(item.openHref ? splitHref(item.openHref) : undefined);
	const hasChoices = $derived(
		Boolean(
			item.state === 'open' &&
			item.inboxItem?.actions.includes('choose') &&
			item.inboxItem.choices &&
			item.inboxItem.choices.length > 0
		)
	);
	const hasReply = $derived(
		Boolean(item.state === 'open' && item.inboxItem?.actions.includes('reply'))
	);
	const canDeny = $derived(
		Boolean(
			item.state === 'open' &&
			(item.inboxItem?.actions.includes('deny') || isDenyAction(item.decision?.secondaryAction))
		)
	);
	const isOpenOnly = $derived(
		Boolean(
			item.state === 'open' &&
			item.sourceKind === 'inbox' &&
			!hasChoices &&
			!hasReply &&
			!canDeny &&
			item.decision?.primaryAction.kind === 'open_inbox'
		)
	);
	const canPrimaryAction = $derived(
		Boolean(
			item.state === 'open' &&
			!hasChoices &&
			!hasReply &&
			item.decision &&
			item.decision.primaryAction.kind !== 'open_inbox'
		)
	);
	const isExplicitApprove = $derived(
		Boolean(
			item.state === 'open' &&
			(item.decision?.primaryAction.kind === 'approve_inbox' ||
				item.decision?.primaryAction.kind === 'approve_request')
		)
	);
	const canSnooze = $derived(
		Boolean(
			item.sourceKind === 'inbox' &&
			item.inboxItem &&
			item.inboxItem.snoozeable !== false &&
			item.state !== 'handled'
		)
	);
	const deniedHandled = $derived(isDeniedOutcome(item.handled?.outcome));

	const btnTouch =
		'[@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11';

	async function handleCancelDeny() {
		onCancelDeny();
		await tick();
		denyBtnEl?.focus();
	}

	function handleReplyInputKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && event.isComposing) event.preventDefault();
	}

	function handleReplySubmit(event: SubmitEvent) {
		event.preventDefault();
		if (!replyText.trim() || controlsDisabled) return;
		onReply(item, replyText.trim());
	}
</script>

<div class="triage-decision">
	{#if item.recommendation}
		<InboxDetailRecommendation text={item.recommendation} tone={item.recommendationTone} />
	{/if}

	<div class="triage-actions">
		{#if item.state === 'handled' && item.handled}
			<div class="flex flex-wrap items-center gap-2 text-xs">
				<span
					class={[
						'triage-handled-badge inline-flex items-center gap-1.5 px-2.5 py-1 font-semibold',
						deniedHandled ? 'is-denied' : 'is-ok'
					]}
				>
					<CheckCircle2 class="size-3.5 shrink-0" aria-hidden="true" />
					<span>{item.handled.outcome}</span>
				</span>
				{#if item.handled.by}
					<span class="triage-detail-muted">by {item.handled.by}</span>
				{/if}
				<span class="triage-detail-muted font-mono tabular-nums">· {item.handled.atLabel}</span>
			</div>
		{:else if item.state === 'snoozed'}
			<div class="flex w-full flex-col gap-3">
				<div class="flex flex-wrap items-center gap-2 text-xs">
					<span
						data-testid="snoozed-badge"
						class="triage-snoozed-badge inline-flex items-center gap-1.5 px-2.5 py-1 font-semibold"
					>
						<Clock class="size-3.5 shrink-0" aria-hidden="true" />
						<span>{snoozeReturn ? `Snoozed until ${snoozeReturn.short}` : 'Snoozed'}</span>
					</span>
					{#if snoozeReturn}
						<span class="triage-detail-muted font-mono text-xs tabular-nums">
							({snoozeReturn.full})
						</span>
					{/if}
				</div>
				{#if item.snoozed?.note}
					<p class="triage-detail-muted text-xs [overflow-wrap:anywhere]">
						Note: {item.snoozed.note}
					</p>
				{/if}
				<div class="flex flex-wrap items-center gap-2">
					<button
						type="button"
						disabled={controlsDisabled}
						onclick={() => onUnsnooze(item)}
						class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
					>
						<span class="exec-cta-fill" aria-hidden="true"></span>
						<span class="exec-cta-inner">
							<RotateCcw class="size-3.5" aria-hidden="true" />
							<span>{isBusy ? 'Un-snoozing…' : 'Un-snooze'}</span>
						</span>
					</button>
					<InboxSnoozeDropdown
						{item}
						triggerLabel="Re-snooze"
						menuHeading="Return to inbox"
						preserveNote={true}
						{controlsDisabled}
						{zone}
						{nowMs}
						{isAskReplyCutoff}
						{onDefer}
						{onOpenCustomSnooze}
					/>
				</div>
			</div>
		{:else}
			{#if hasReply}
				<form class="flex w-full flex-col gap-2.5" onsubmit={handleReplySubmit}>
					<label class="sr-only" for="triage-reply-{item.id}">Reply to {item.actorName}</label>
					<div class="flex flex-wrap items-center gap-2">
						<input
							id="triage-reply-{item.id}"
							type="text"
							autocomplete="off"
							value={replyText}
							oninput={(e) => onReplyTextChange(item.id, e.currentTarget.value)}
							onkeydown={handleReplyInputKeydown}
							disabled={controlsDisabled}
							placeholder="Write your reply to {item.actorName}…"
							class="triage-input min-h-9 min-w-[200px] flex-1 px-3 py-1.5 text-sm [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
						/>
						<button
							type="submit"
							disabled={controlsDisabled || !replyText.trim()}
							class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
						>
							<span class="exec-cta-fill" aria-hidden="true"></span>
							<span class="exec-cta-inner">
								<Send class="size-3.5" aria-hidden="true" />
								<span>{isBusy ? 'Sending…' : 'Send reply'}</span>
							</span>
						</button>
					</div>
				</form>
			{/if}

			<div class="flex w-full flex-wrap items-center justify-between gap-3">
				<div class="flex flex-wrap items-center gap-2">
					{#if hasChoices && item.inboxItem?.choices}
						{#each item.inboxItem.choices as choice, idx (choice.id)}
							{#if idx === 0}
								<button
									type="button"
									disabled={controlsDisabled}
									onclick={() => onChoose(item, choice.id, choice.label)}
									class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
								>
									<span class="exec-cta-fill" aria-hidden="true"></span>
									<span class="exec-cta-inner">
										<Check class="size-4" aria-hidden="true" />
										<span>{isBusy ? 'Recording…' : choice.label}</span>
									</span>
								</button>
							{:else}
								<button
									type="button"
									disabled={controlsDisabled}
									onclick={() => onChoose(item, choice.id, choice.label)}
									class="triage-secondary-btn inline-flex min-h-9 min-w-9 items-center justify-center gap-1.5 px-3.5 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
								>
									<span>{choice.label}</span>
								</button>
							{/if}
						{/each}
					{:else if canPrimaryAction && item.decision}
						<button
							type="button"
							disabled={controlsDisabled}
							onclick={() => onPrimaryAction(item)}
							class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
						>
							<span class="exec-cta-fill" aria-hidden="true"></span>
							<span class="exec-cta-inner">
								<Check class="size-4" aria-hidden="true" />
								<span>{isBusy ? 'Working…' : item.decision.primaryAction.label}</span>
							</span>
						</button>
					{:else if isOpenOnly && openParts}
						<a
							href="{resolve(openParts.pathname as '/work')}{openParts.suffix}"
							class="exec-cta min-w-9 {btnTouch}"
						>
							<span class="exec-cta-fill" aria-hidden="true"></span>
							<span class="exec-cta-inner">
								<ExternalLink class="size-3.5" aria-hidden="true" />
								<span>{item.openLabel ?? `Open ${item.workRef ?? 'item'}`}</span>
							</span>
						</a>
					{/if}

					{#if canDeny && !isDenying}
						<button
							bind:this={denyBtnEl}
							type="button"
							disabled={controlsDisabled}
							onclick={() => onArmDeny(item.id)}
							class="triage-deny-btn inline-flex min-h-9 min-w-9 items-center justify-center gap-1.5 px-3.5 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
						>
							<Ban class="size-3.5 shrink-0" aria-hidden="true" />
							<span>{item.decision?.secondaryAction?.label ?? 'Deny'}</span>
						</button>
					{/if}

					{#if canSnooze && !isDenying}
						<InboxSnoozeDropdown
							{item}
							triggerLabel="Snooze"
							menuHeading="Snooze until"
							preserveNote={false}
							testId="inbox-snooze-trigger"
							{controlsDisabled}
							{zone}
							{nowMs}
							{isAskReplyCutoff}
							{onDefer}
							{onOpenCustomSnooze}
						/>
					{/if}
				</div>

				{#if shortcutsEnabled}
					<div class="triage-kbd-hints hidden items-center gap-2 text-xs lg:flex">
						<span class="inline-flex items-center gap-1">
							<kbd class="triage-kbd px-1.5 py-0.5 font-mono text-xs">J</kbd>/<kbd
								class="triage-kbd px-1.5 py-0.5 font-mono text-xs">K</kbd
							>
							<span>navigate</span>
						</span>
						{#if isExplicitApprove}
							<span class="inline-flex items-center gap-1">
								<kbd class="triage-kbd px-1.5 py-0.5 font-mono text-xs">A</kbd>
								<span>approve</span>
							</span>
						{/if}
						{#if canDeny}
							<span class="inline-flex items-center gap-1">
								<kbd class="triage-kbd px-1.5 py-0.5 font-mono text-xs">D</kbd>
								<span>deny</span>
							</span>
						{/if}
					</div>
				{/if}
			</div>

			{#if canDeny && isDenying}
				<InboxDetailDenyGuard
					itemId={item.id}
					actorName={item.actorName}
					{declineReason}
					{isBusy}
					{controlsDisabled}
					bind:denyInputEl
					onDeclineReasonChange={(value) => onDeclineReasonChange(item.id, value)}
					onCancel={() => void handleCancelDeny()}
					onConfirm={() => onSecondaryAction(item)}
				/>
			{/if}
		{/if}
	</div>
</div>

<style>
	.triage-decision {
		display: grid;
		min-width: 0;
		gap: 12px;
	}
	.triage-detail-muted {
		color: var(--exec-muted);
		font-size: 0.875rem;
	}
	.triage-snoozed-badge {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.triage-actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
	}
	.triage-kbd-hints {
		color: var(--exec-muted);
	}
	.triage-secondary-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-secondary-btn:hover:not(:disabled) {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.triage-deny-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-deny-bg);
		color: var(--exec-deny-ink);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-deny-btn:hover:not(:disabled) {
		background: var(--exec-deny-bg-hover);
		color: var(--exec-deny-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring-hover);
	}
	.triage-input {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.triage-input::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	.triage-input:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.triage-kbd {
		border-radius: var(--exec-cmdk-kbd-radius);
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.triage-handled-badge {
		border-radius: var(--exec-pill-radius);
	}
	.triage-handled-badge.is-ok {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: var(--exec-pill-ring-ok);
	}
	.triage-handled-badge.is-denied {
		background: var(--exec-bad-bg);
		color: var(--exec-bad);
		box-shadow: var(--exec-pill-ring-bad);
	}
	.triage-secondary-btn:focus-visible,
	.triage-deny-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (forced-colors: active) {
		.triage-secondary-btn,
		.triage-deny-btn,
		.triage-actions :global(.exec-cta) {
			border: 1px solid ButtonText;
		}
		.triage-input {
			border: 1px solid CanvasText;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.triage-secondary-btn,
		.triage-deny-btn {
			transition: none;
		}
	}
</style>
