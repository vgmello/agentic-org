<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Inbox from '@lucide/svelte/icons/inbox';
	import { appendInboxDeviceComment, readInboxDeviceComments } from '../../device-comments';
	import InboxDetailActivity from './InboxDetailActivity.svelte';
	import InboxDetailDecisionActions from './InboxDetailDecisionActions.svelte';
	import InboxDetailRequestFields from './InboxDetailRequestFields.svelte';
	import { formatSnoozeReturn } from './snooze-presets';
	import { highlightSegments, type InboxCommentEntry, type TriageItem } from './triage-model';
	import { computeMemberPreviewClamp } from './triage-preview-clamp';

	let {
		organizationId,
		item,
		searchQuery,
		busyId,
		actionLocked,
		shortcutsEnabled,
		decliningId,
		declineReason,
		replyText,
		commentDraft,
		zone = 'UTC',
		nowMs = Date.now(),
		onDeclineReasonChange,
		onReplyTextChange,
		onCommentDraftChange,
		onCommentDraftClear,
		onBackMobile,
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
		organizationId: string;
		item?: TriageItem;
		searchQuery: string;
		busyId?: string;
		actionLocked: boolean;
		shortcutsEnabled: boolean;
		decliningId?: string;
		declineReason: string;
		replyText: string;
		commentDraft: string;
		zone?: string;
		nowMs?: number;
		onDeclineReasonChange: (itemId: string, value: string) => void;
		onReplyTextChange: (itemId: string, value: string) => void;
		onCommentDraftChange: (itemId: string, value: string) => void;
		onCommentDraftClear: (orgId: string, itemId: string) => void;
		onBackMobile: () => void;
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

	let denyInputEl = $state<HTMLInputElement | null>(null);
	let commentInputEl = $state<HTMLTextAreaElement | null>(null);
	let readingEl = $state<HTMLElement | null>(null);
	let copiedKey = $state<string | undefined>(undefined);
	let copyTimer: ReturnType<typeof setTimeout> | undefined;

	// Hover/focus member preview card dismissal & collision clamping (WCAG 1.4.13)
	let memberHovered = $state(false);
	let memberFocused = $state(false);
	let previewFlipAbove = $state(false);
	let previewShiftX = $state(0);
	let previewShiftY = $state(0);
	let previewMaxHeight = $state<number | undefined>(undefined);
	let memberNodeEl = $state<HTMLElement | null>(null);
	let previewDismissed = $derived.by(() => {
		void item?.id;
		return false;
	});

	// Device-local notes version counter & status announcement (cleared on sign-out)
	let commentVersion = $state(0);
	let lastCommentNotice = $state<{ key: string; text: string } | undefined>(undefined);
	const commentScopeKey = $derived(`${organizationId}:${item?.id ?? ''}`);
	const commentStatus = $derived(
		lastCommentNotice?.key === commentScopeKey ? lastCommentNotice.text : ''
	);

	const comments = $derived.by<InboxCommentEntry[]>(() => {
		void commentVersion;
		return item ? readInboxDeviceComments(organizationId, item.id) : [];
	});

	const isBusy = $derived(Boolean(item && busyId === item.id));
	const controlsDisabled = $derived(isBusy || actionLocked);
	const isDenying = $derived(Boolean(item && decliningId === item.id));
	const snoozeReturn = $derived(
		item?.state === 'snoozed' && item.snoozed?.untilIso
			? formatSnoozeReturn(item.snoozed.untilIso, new Date(nowMs), zone)
			: undefined
	);
	const isAskReplyCutoff = $derived(
		item?.inboxItem?.source === 'Chat ask' || item?.inboxItem?.source === 'Chat approval'
	);

	$effect(() => {
		if (isDenying) {
			void tick().then(() => denyInputEl?.focus());
		}
	});

	$effect(() => {
		void organizationId;
		void item?.id;
		commentInputEl?.setCustomValidity('');
	});

	$effect(() => {
		const onKeydownCapture = (event: KeyboardEvent) => {
			if (event.key === 'Escape' && (memberHovered || memberFocused) && !previewDismissed) {
				event.preventDefault();
				event.stopPropagation();
				previewDismissed = true;
			}
		};
		window.addEventListener('keydown', onKeydownCapture, { capture: true });
		return () => window.removeEventListener('keydown', onKeydownCapture, { capture: true });
	});

	onDestroy(() => clearTimeout(copyTimer));

	async function copyWorkRef(itemId: string, ref: string) {
		try {
			if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
				await navigator.clipboard.writeText(ref);
			}
		} catch {
			// Clipboard write may be restricted; still provide visual feedback.
		}
		const key = `${itemId}:${ref}`;
		copiedKey = key;
		clearTimeout(copyTimer);
		copyTimer = setTimeout(() => {
			if (copiedKey === key) copiedKey = undefined;
		}, 1800);
	}

	let clampRaf: number | undefined;
	function resetPreviewPosition() {
		previewFlipAbove = false;
		previewShiftX = 0;
		previewShiftY = 0;
		previewMaxHeight = undefined;
	}
	function clampMemberPreview(node: HTMLElement) {
		resetPreviewPosition();
		if (clampRaf !== undefined) cancelAnimationFrame(clampRaf);
		clampRaf = requestAnimationFrame(() => {
			clampRaf = undefined;
			if ((!memberHovered && !memberFocused) || previewDismissed) return;
			const next = computeMemberPreviewClamp(node, readingEl);
			if (!next) return;
			previewFlipAbove = next.flipAbove;
			previewShiftX = next.shiftX;
			previewShiftY = next.shiftY;
			previewMaxHeight = next.maxHeight;
		});
	}
	$effect(() => {
		if ((!memberHovered && !memberFocused) || previewDismissed || !memberNodeEl) return;
		const node = memberNodeEl;
		const reading = readingEl;
		const onViewportChange = () => clampMemberPreview(node);
		window.addEventListener('resize', onViewportChange);
		window.addEventListener('scroll', onViewportChange, { capture: true, passive: true });
		reading?.addEventListener('scroll', onViewportChange, { passive: true });
		return () => {
			if (clampRaf !== undefined) cancelAnimationFrame(clampRaf);
			window.removeEventListener('resize', onViewportChange);
			window.removeEventListener('scroll', onViewportChange, { capture: true });
			reading?.removeEventListener('scroll', onViewportChange);
		};
	});

	function memberPreviewTracking(node: HTMLElement) {
		memberNodeEl = node;
		const resetPreview = () => {
			previewDismissed = false;
			resetPreviewPosition();
		};
		const onEnter = () => {
			memberHovered = true;
			clampMemberPreview(node);
		};
		const onLeave = () => {
			memberHovered = false;
			if (!memberFocused) resetPreview();
		};
		const onFocusIn = () => {
			memberFocused = true;
			clampMemberPreview(node);
		};
		const onFocusOut = (event: FocusEvent) => {
			const next = event.relatedTarget as Node | null;
			if (next && node.contains(next)) return;
			memberFocused = false;
			if (!memberHovered) resetPreview();
		};
		node.addEventListener('pointerenter', onEnter);
		node.addEventListener('pointerleave', onLeave);
		node.addEventListener('focusin', onFocusIn);
		node.addEventListener('focusout', onFocusOut);
		return () => {
			if (memberNodeEl === node) memberNodeEl = null;
			node.removeEventListener('pointerenter', onEnter);
			node.removeEventListener('pointerleave', onLeave);
			node.removeEventListener('focusin', onFocusIn);
			node.removeEventListener('focusout', onFocusOut);
		};
	}

	function updateCommentDraft(value: string) {
		if (!item) return;
		onCommentDraftChange(item.id, value);
		commentInputEl?.setCustomValidity('');
	}

	function handleCommentSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (!item) return;
		const trimmed = commentDraft.trim();
		if (!trimmed) {
			if (commentInputEl) {
				commentInputEl.setCustomValidity('Enter a comment.');
				commentInputEl.reportValidity();
			}
			return;
		}
		const nextEntries = appendInboxDeviceComment(organizationId, item.id, trimmed);
		if (!nextEntries) {
			toast.error('Could not save the comment on this device.');
			commentInputEl?.focus();
			return;
		}
		onCommentDraftClear(organizationId, item.id);
		commentVersion += 1;
		lastCommentNotice = {
			key: commentScopeKey,
			text: `Saved locally on this device (${nextEntries.length} total; not sent to office).`
		};
		commentInputEl?.setCustomValidity('');
		commentInputEl?.focus();
	}
</script>

{#snippet highlighted(text: string)}
	{#each highlightSegments(text, searchQuery) as seg, i (`${i}-${seg.match}`)}
		{#if seg.match}<mark class="triage-hit">{seg.text}</mark>{:else}{seg.text}{/if}
	{/each}
{/snippet}

<section
	bind:this={readingEl}
	class="triage-reading flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto"
	aria-label="Item details"
>
	{#if !item}
		<div class="triage-empty-detail">
			<span class="triage-empty-badge flex size-10 items-center justify-center">
				<Inbox class="size-5" aria-hidden="true" />
			</span>
			<h2 class="triage-detail-heading">No item selected</h2>
			<p class="triage-detail-muted">Your queue appears on the left.</p>
		</div>
	{:else}
		<article class="triage-document" aria-labelledby="triage-detail-heading">
			<button
				type="button"
				onclick={onBackMobile}
				class="triage-back inline-flex min-h-11 w-fit items-center gap-1.5 text-xs font-semibold"
			>
				<ArrowLeft class="size-4" aria-hidden="true" />
				<span>Back to Inbox</span>
			</button>

			<header class="triage-intro">
				<p class="triage-from">
					<span>From:</span>
					{#if item.actorId}
						<span
							{@attach memberPreviewTracking}
							class="triage-from-member"
							data-preview-dismissed={previewDismissed ? '' : undefined}
						>
							<a
								class="triage-from-link"
								href={resolve(`/members/${encodeURIComponent(item.actorId)}` as '/teams/members')}
								aria-describedby="triage-from-role-{item.id}"
							>
								{item.actorName}
							</a>
							<span
								class="triage-from-popover"
								data-flip-above={previewFlipAbove ? '' : undefined}
								style:--preview-shift-x="{previewShiftX}px"
								style:--preview-shift-y="{previewShiftY}px"
							>
								<span
									class="triage-from-card"
									role="tooltip"
									style:--preview-max-h={previewMaxHeight ? `${previewMaxHeight}px` : undefined}
								>
									<span class="triage-from-av" aria-hidden="true">{item.actorInitials}</span>
									<span>
										<strong aria-hidden="true">{item.actorName}</strong>
										<span id="triage-from-role-{item.id}">{item.actorRole}</span>
									</span>
								</span>
							</span>
						</span>
					{:else}
						<strong class="triage-ink">{item.actorName}</strong>
					{/if}
					<span aria-hidden="true">·</span>
					<span class="triage-from-time">{item.ageLabel}</span>
				</p>

				<h2 id="triage-detail-heading" tabindex="-1" class="triage-detail-title font-bold">
					{@render highlighted(item.title)}
				</h2>
			</header>

			{#if item.why}
				<p class="triage-description">
					{@render highlighted(item.why)}
				</p>
			{/if}

			<InboxDetailRequestFields
				{item}
				{copiedKey}
				{isAskReplyCutoff}
				snoozeReturnFull={snoozeReturn?.full}
				onCopyWorkRef={(id, ref) => void copyWorkRef(id, ref)}
			/>

			<InboxDetailDecisionActions
				{item}
				{isBusy}
				{controlsDisabled}
				{shortcutsEnabled}
				{isDenying}
				{declineReason}
				{replyText}
				{zone}
				{nowMs}
				{snoozeReturn}
				{isAskReplyCutoff}
				bind:denyInputEl
				{onDeclineReasonChange}
				{onReplyTextChange}
				{onPrimaryAction}
				{onSecondaryAction}
				{onArmDeny}
				{onCancelDeny}
				{onChoose}
				{onReply}
				{onDefer}
				{onOpenCustomSnooze}
				{onUnsnooze}
			/>

			<InboxDetailActivity
				{item}
				{comments}
				{commentDraft}
				{commentStatus}
				snoozeReturnFull={snoozeReturn?.full}
				bind:commentInputEl
				onCommentDraftChange={updateCommentDraft}
				onCommentSubmit={handleCommentSubmit}
			/>
		</article>
	{/if}
</section>

<style>
	.triage-reading {
		min-width: 0;
		container: inbox-reading / inline-size;
		overscroll-behavior-y: contain;
	}
	.triage-empty-detail {
		display: grid;
		justify-items: start;
		align-content: start;
		gap: 6px;
		padding: 32px;
		color: var(--exec-muted);
	}
	.triage-empty-badge {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.triage-detail-heading {
		color: var(--exec-ink);
		font-size: 0.9375rem;
		font-weight: 600;
	}
	.triage-detail-muted {
		color: var(--exec-muted);
		font-size: 0.875rem;
	}
	.triage-document {
		max-width: 68rem;
		width: 100%;
		margin-inline: auto;
		padding: clamp(28px, 4.5cqi, 56px) clamp(16px, 5cqi, 56px) 36px;
		display: flex;
		flex-direction: column;
		gap: clamp(16px, 2.4cqi, 24px);
		color: var(--exec-ink);
		font-size: 0.9375rem;
		line-height: 1.6;
	}
	.triage-back {
		display: none;
		color: var(--exec-body);
		cursor: pointer;
	}
	.triage-back:hover {
		color: var(--exec-ink);
	}
	@media (max-width: 959px) {
		.triage-document {
			padding-block-start: 12px;
		}
		.triage-back {
			display: inline-flex;
		}
	}
	.triage-intro {
		display: grid;
		gap: 8px;
	}
	.triage-from {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 3px 6px;
		color: var(--exec-body);
		font-size: 0.875rem;
	}
	.triage-from-time {
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.triage-from-member {
		position: relative;
		display: inline-flex;
		align-items: center;
	}
	.triage-from-link {
		display: inline-flex;
		align-items: center;
		min-width: 24px;
		min-height: 24px;
		color: var(--exec-ink);
		font-weight: 600;
		text-decoration: none;
	}
	.triage-from-link:is(:hover, :focus-visible) {
		text-decoration: underline;
	}
	.triage-from-popover {
		position: absolute;
		z-index: 10;
		inset-block-start: 100%;
		inset-inline-start: 0;
		display: none;
		padding-block-start: calc(4px + max(0px, var(--preview-shift-y, 0px)));
		transform: translate(var(--preview-shift-x, 0px), min(0px, var(--preview-shift-y, 0px)));
	}
	.triage-from-popover[data-flip-above] {
		inset-block-start: auto;
		inset-block-end: 100%;
		padding-block-start: 0;
		padding-block-end: calc(4px + max(0px, calc(-1 * var(--preview-shift-y, 0px))));
		transform: translate(var(--preview-shift-x, 0px), max(0px, var(--preview-shift-y, 0px)));
	}
	.triage-from-member:not([data-preview-dismissed]):is(:hover, :focus-within) .triage-from-popover {
		display: block;
	}
	.triage-from-card {
		display: flex;
		align-items: center;
		gap: 10px;
		width: max-content;
		max-width: min(18rem, 65cqi);
		max-height: var(--preview-max-h, 16rem);
		overflow-y: auto;
		padding: 9px 12px;
		border: 1px solid var(--exec-border-hi);
		border-radius: var(--exec-inset-radius);
		background: var(--popover);
		box-shadow: 0 8px 24px color-mix(in srgb, var(--shade) 24%, transparent);
		color: var(--exec-body);
		font-size: 0.75rem;
		line-height: 1.45;
	}
	.triage-from-card strong {
		display: block;
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 600;
	}
	.triage-from-av {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 32px;
		height: 32px;
		flex: none;
		border-radius: 9999px;
		background: var(--exec-avatar-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.75rem;
		font-weight: 700;
	}
	.triage-detail-title {
		color: var(--exec-ink);
		font-size: clamp(1.5rem, 3.5cqi, 1.875rem);
		line-height: 1.22;
		letter-spacing: -0.025em;
		overflow-wrap: anywhere;
	}
	.triage-detail-title:focus {
		outline: none;
	}
	.triage-detail-title:focus-visible,
	.triage-back:focus-visible,
	.triage-from-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.triage-description {
		max-width: 75ch;
		color: var(--exec-body);
		overflow-wrap: anywhere;
	}
	.triage-ink {
		color: var(--exec-ink);
	}
	.triage-hit {
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
		border-radius: 3px;
		padding: 0 2px;
		font-weight: 600;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.triage-from-link {
			min-width: 44px;
			min-height: 44px;
		}
	}
	@media (forced-colors: active) {
		.triage-back {
			border: 1px solid ButtonText;
		}
	}
</style>
