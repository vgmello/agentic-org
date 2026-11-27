<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { resolve } from '$app/paths';
	import Ban from '@lucide/svelte/icons/ban';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Info from '@lucide/svelte/icons/info';
	import Pencil from '@lucide/svelte/icons/pencil';
	import { isDenyAction } from '$lib/modules/inbox/spotlight';
	import type { SpotlightDecision } from '$lib/modules/home/model';
	import SpotlightDenyGuard from './SpotlightDenyGuard.svelte';
	import SpotlightReplyBox from './SpotlightReplyBox.svelte';

	export interface SpotlightReceipt {
		outcome: string;
		note?: string;
	}

	interface Props {
		decision: SpotlightDecision;
		totalCount: number;
		busy: boolean;
		receipt?: SpotlightReceipt;
		declining: boolean;
		declineReason: string;
		replying: boolean;
		replyText: string;
		restoreDenyFocus: boolean;
		onClearRestoreDenyFocus: () => void;
		onPrimary: () => void;
		onSecondary: () => void;
		onArmDeny: () => void;
		onCancelDeny: () => void;
		onOpenReply: () => void;
		onSubmitReply: () => void;
		onCancelReply: () => void;
	}

	let {
		decision,
		totalCount,
		busy,
		receipt,
		declining,
		declineReason = $bindable(''),
		replying,
		replyText = $bindable(''),
		restoreDenyFocus,
		onClearRestoreDenyFocus,
		onPrimary,
		onSecondary,
		onArmDeny,
		onCancelDeny,
		onOpenReply,
		onSubmitReply,
		onCancelReply
	}: Props = $props();

	const canReplyInline = $derived(
		decision.category === 'question' &&
			decision.sourceKind === 'inbox' &&
			Boolean(decision.inboxItem?.actions.includes('reply'))
	);
	const hasInlinePrimaryChoice = $derived(
		decision.primaryAction.kind !== 'open_inbox' &&
			!(canReplyInline && decision.primaryAction.kind === 'dismiss_inbox')
	);
	const isConsequentialRoute = $derived(
		!receipt &&
			decision.sourceKind === 'inbox' &&
			decision.primaryAction.kind === 'open_inbox' &&
			!canReplyInline
	);
	const routeReason = $derived(
		decision.inboxItem?.open?.label || 'Full context and checks live in Inbox'
	);

	const focusDenyTriggerOnCancel: Attachment<HTMLButtonElement> = (el) => {
		if (restoreDenyFocus) {
			onClearRestoreDenyFocus();
			el.focus();
		}
	};
</script>

<div class="spot-foot flex flex-wrap items-center justify-between gap-3 pt-3.5">
	{#if receipt}
		<div class="ask-receipt flex flex-1 flex-col items-start gap-1.5" role="status">
			<span class="handled-win inline-flex items-center gap-1.5">
				<CheckCircle2 class="size-3.5" aria-hidden="true" />
				<span>{receipt.outcome}</span>
			</span>
			{#if receipt.note}
				<p class="receipt-note">{receipt.note}</p>
			{/if}
		</div>
	{:else if replying}
		<SpotlightReplyBox
			decisionId={decision.id}
			actorName={decision.actorName}
			{busy}
			bind:replyText
			onSubmit={onSubmitReply}
			onCancel={onCancelReply}
		/>
	{:else if isConsequentialRoute}
		<div class="flex flex-1 flex-wrap items-center gap-3">
			<a href={resolve(decision.detailHref as '/inbox')} class="exec-cta min-h-9">
				<span class="exec-cta-fill" aria-hidden="true"></span>
				<span class="exec-cta-sheen" aria-hidden="true"></span>
				<span class="exec-cta-inner">
					<span>Review in Inbox</span>
					<ChevronRight class="size-4" aria-hidden="true" />
				</span>
			</a>
			<p class="ask-route inline-flex items-center gap-1.5 text-xs text-[var(--exec-muted)]">
				<Info class="size-3.5 shrink-0" aria-hidden="true" />
				<span>{routeReason}</span>
			</p>
		</div>
	{:else}
		<div class="flex flex-1 flex-wrap items-center gap-2">
			{#if canReplyInline && !hasInlinePrimaryChoice}
				<button type="button" disabled={busy} onclick={onOpenReply} class="exec-cta min-h-9">
					<span class="exec-cta-fill" aria-hidden="true"></span>
					<span class="exec-cta-sheen" aria-hidden="true"></span>
					<span class="exec-cta-inner">
						<Pencil class="size-3.5" aria-hidden="true" />
						<span>Answer…</span>
					</span>
				</button>
			{:else}
				<button type="button" disabled={busy} onclick={onPrimary} class="exec-cta min-h-9">
					<span class="exec-cta-fill" aria-hidden="true"></span>
					<span class="exec-cta-sheen" aria-hidden="true"></span>
					<span class="exec-cta-inner">
						<Check class="size-4" aria-hidden="true" />
						<span>{decision.primaryAction.label}</span>
					</span>
				</button>
				{#if canReplyInline}
					<button
						type="button"
						disabled={busy}
						onclick={onOpenReply}
						class="outline-btn inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold"
					>
						<Pencil class="size-3.5" aria-hidden="true" />
						<span>Answer…</span>
					</button>
				{/if}
			{/if}

			{#if isDenyAction(decision.secondaryAction)}
				{#if !declining}
					<span class="action-sep" aria-hidden="true"></span>
					<button
						{@attach focusDenyTriggerOnCancel}
						type="button"
						disabled={busy}
						onclick={onArmDeny}
						class="deny-btn inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold"
					>
						<Ban class="size-3.5" aria-hidden="true" />
						<span>{decision.secondaryAction.label}</span>
					</button>
				{/if}
			{:else if decision.secondaryAction}
				<button
					type="button"
					disabled={busy}
					onclick={onSecondary}
					class="outline-btn inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold"
				>
					<span>{decision.secondaryAction.label}</span>
				</button>
			{/if}
		</div>
	{/if}

	<div class="flex items-center gap-2">
		{#if !receipt && !isConsequentialRoute}
			<a
				href={resolve(decision.detailHref as '/work')}
				class="skip-btn inline-flex min-h-9 items-center gap-0.5 rounded-lg px-2 text-xs font-medium"
			>
				<span>Details</span>
				<ChevronRight class="size-3.5" aria-hidden="true" />
			</a>
		{/if}
		<a
			href={resolve('/inbox')}
			class="skip-btn inline-flex min-h-9 items-center gap-0.5 rounded-lg px-2 text-xs font-medium"
		>
			<span>All {totalCount} in Inbox</span>
			<ChevronRight class="size-3.5" aria-hidden="true" />
		</a>
	</div>

	{#if declining}
		<SpotlightDenyGuard
			decisionId={decision.id}
			actorName={decision.actorName}
			{busy}
			bind:declineReason
			onConfirm={onSecondary}
			onCancel={onCancelDeny}
		/>
	{/if}
</div>

<style>
	.spot-foot {
		margin-top: 4px;
		border-top: 1px solid var(--exec-border);
	}
	.handled-win {
		padding: 4px 10px;
		border-radius: 999px;
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-ok);
	}
	.receipt-note {
		margin: 0;
		padding: 2px 0 2px 12px;
		border-inline-start: 2px solid var(--exec-border-hi);
		font-size: 0.8125rem;
		line-height: 1.5;
		color: var(--exec-body);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.action-sep {
		width: 1px;
		height: 20px;
		background: var(--exec-border-hi);
		margin: 0 2px;
		flex: none;
	}
	.outline-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.outline-btn:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.deny-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-deny-bg);
		color: var(--exec-deny-ink);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.deny-btn:hover {
		background: var(--exec-deny-bg-hover);
		color: var(--exec-deny-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring-hover);
	}
	.skip-btn {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.skip-btn:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.deny-btn,
		.outline-btn,
		.skip-btn {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.outline-btn,
		.deny-btn,
		.skip-btn {
			transition: none;
		}
	}
</style>
