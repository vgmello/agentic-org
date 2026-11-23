<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Ban from '@lucide/svelte/icons/ban';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Clock from '@lucide/svelte/icons/clock';
	import Copy from '@lucide/svelte/icons/copy';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import Inbox from '@lucide/svelte/icons/inbox';
	import Info from '@lucide/svelte/icons/info';
	import Send from '@lucide/svelte/icons/send';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import { isDenyAction } from '$lib/home/model';
	import {
		highlightSegments,
		inboxCommentsStorageKey,
		isDeniedOutcome,
		MAX_INBOX_COMMENT_LENGTH,
		parseInboxCommentsJson,
		splitHref,
		type InboxCommentEntry,
		type TriageItem
	} from './triage-model';

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
		onReply
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
	} = $props();

	let denyInputEl = $state<HTMLInputElement | null>(null);
	let denyBtnEl = $state<HTMLButtonElement | null>(null);
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

	// Persisted comments version counter & status announcement
	let commentVersion = $state(0);
	let commentStatus = $derived.by(() => {
		void organizationId;
		void item?.id;
		return '';
	});

	function readStoredComments(orgId: string, itemId: string): InboxCommentEntry[] {
		if (typeof window === 'undefined') return [];
		try {
			const raw = window.localStorage.getItem(inboxCommentsStorageKey(orgId, itemId));
			return parseInboxCommentsJson(raw);
		} catch {
			return [];
		}
	}

	const comments = $derived.by<InboxCommentEntry[]>(() => {
		void commentVersion;
		if (!item) return [];
		return readStoredComments(organizationId, item.id);
	});

	const isBusy = $derived(Boolean(item && busyId === item.id));
	const controlsDisabled = $derived(isBusy || actionLocked);
	const isDenying = $derived(Boolean(item && decliningId === item.id));
	const workParts = $derived(item?.workHref ? splitHref(item.workHref) : undefined);
	const openParts = $derived(item?.openHref ? splitHref(item.openHref) : undefined);
	const relatedParts = $derived(item?.relatedHref ? splitHref(item.relatedHref) : undefined);
	const hasChoices = $derived(
		Boolean(
			item?.state === 'open' &&
			item.inboxItem?.actions.includes('choose') &&
			item.inboxItem.choices &&
			item.inboxItem.choices.length > 0
		)
	);
	const hasReply = $derived(
		Boolean(item?.state === 'open' && item.inboxItem?.actions.includes('reply'))
	);
	const canDeny = $derived(
		Boolean(
			item?.state === 'open' &&
			(item.inboxItem?.actions.includes('deny') || isDenyAction(item.decision?.secondaryAction))
		)
	);
	const isOpenOnly = $derived(
		Boolean(
			item?.state === 'open' &&
			item.sourceKind === 'inbox' &&
			!hasChoices &&
			!hasReply &&
			!canDeny &&
			item.decision?.primaryAction.kind === 'open_inbox'
		)
	);
	const canPrimaryAction = $derived(
		Boolean(
			item?.state === 'open' &&
			!hasChoices &&
			!hasReply &&
			item.decision &&
			item.decision.primaryAction.kind !== 'open_inbox'
		)
	);
	const isExplicitApprove = $derived(
		Boolean(
			item?.state === 'open' &&
			(item.decision?.primaryAction.kind === 'approve_inbox' ||
				item.decision?.primaryAction.kind === 'approve_request')
		)
	);
	const deniedHandled = $derived(isDeniedOutcome(item?.handled?.outcome));

	const RECOMMENDATION_LABELS: Record<TriageItem['recommendationTone'], string> = {
		ok: 'Recommendation · Low risk',
		warn: 'Recommendation · Investigate',
		bad: 'Recommendation · Use caution',
		neutral: 'Recommendation'
	};

	$effect(() => {
		if (isDenying) {
			void tick().then(() => denyInputEl?.focus());
		}
	});

	// Clear stale constraint validation message whenever the active item or org changes.
	$effect(() => {
		void organizationId;
		void item?.id;
		commentInputEl?.setCustomValidity('');
	});

	// Capture-phase Escape dismisses the hover/focus member preview before Bits UI / document handlers.
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

	function formatCommentTime(at: number): string {
		const date = new Date(at);
		return date.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
	}

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

	async function handleCancelDeny() {
		onCancelDeny();
		await tick();
		denyBtnEl?.focus();
	}

	function handleDenyGuardKeydown(event: KeyboardEvent) {
		if (event.isComposing) return;
		if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			void handleCancelDeny();
		}
	}

	function denyGuardEscape(node: HTMLElement) {
		const listener = (event: KeyboardEvent) => handleDenyGuardKeydown(event);
		node.addEventListener('keydown', listener);
		return () => node.removeEventListener('keydown', listener);
	}

	function handleDenyInputKeydown(event: KeyboardEvent) {
		if (!item || event.isComposing) return;
		if (event.key === 'Enter') {
			event.preventDefault();
			onSecondaryAction(item);
		}
	}

	function handleReplyInputKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && event.isComposing) {
			event.preventDefault();
		}
	}

	function handleReplySubmit(event: SubmitEvent) {
		event.preventDefault();
		if (!item || !replyText.trim() || controlsDisabled) return;
		onReply(item, replyText.trim());
	}

	function clampMemberPreview(node: HTMLElement) {
		previewFlipAbove = false;
		previewShiftX = 0;
		previewShiftY = 0;
		previewMaxHeight = undefined;
		requestAnimationFrame(() => {
			if ((!memberHovered && !memberFocused) || previewDismissed) return;
			const card = node.querySelector<HTMLElement>('.triage-from-card');
			if (!card) return;
			const cardRect = card.getBoundingClientRect();
			if (cardRect.width === 0 || cardRect.height === 0) return;
			const anchorRect = node.getBoundingClientRect();
			const containerRect = readingEl?.getBoundingClientRect();
			const pad = 8;
			const minLeft = Math.max(pad, (containerRect?.left ?? 0) + pad);
			const maxRight = Math.min(
				window.innerWidth - pad,
				(containerRect?.right ?? window.innerWidth) - pad
			);
			const minTop = Math.max(pad, (containerRect?.top ?? 0) + pad);
			const maxBottom = Math.min(
				window.innerHeight - pad,
				(containerRect?.bottom ?? window.innerHeight) - pad
			);
			const availableHeight = Math.max(48, Math.floor(maxBottom - minTop));
			previewMaxHeight = availableHeight;
			const cardH = Math.min(cardRect.height, availableHeight);

			const spaceBelow = maxBottom - (anchorRect.bottom + 4);
			const spaceAbove = anchorRect.top - 4 - minTop;
			const flip = spaceBelow < cardH && spaceAbove > spaceBelow;
			previewFlipAbove = flip;

			const naturalTop = flip ? anchorRect.top - 4 - cardH : anchorRect.bottom + 4;
			let shiftY = 0;
			if (naturalTop + cardH > maxBottom) {
				shiftY = maxBottom - (naturalTop + cardH);
			}
			if (naturalTop + shiftY < minTop) {
				shiftY = minTop - naturalTop;
			}
			previewShiftY = Math.round(shiftY);

			let shiftX = 0;
			if (cardRect.right > maxRight) {
				shiftX = Math.max(minLeft - cardRect.left, maxRight - cardRect.right);
			} else if (cardRect.left < minLeft) {
				shiftX = minLeft - cardRect.left;
			}
			previewShiftX = Math.round(shiftX);
		});
	}

	// Re-clamp open preview card on scroll or viewport resize so it never clips.
	$effect(() => {
		if ((!memberHovered && !memberFocused) || previewDismissed || !memberNodeEl) return;
		const node = memberNodeEl;
		const reading = readingEl;
		const onViewportChange = () => clampMemberPreview(node);
		window.addEventListener('resize', onViewportChange);
		window.addEventListener('scroll', onViewportChange, { capture: true, passive: true });
		reading?.addEventListener('scroll', onViewportChange, { passive: true });
		return () => {
			window.removeEventListener('resize', onViewportChange);
			window.removeEventListener('scroll', onViewportChange, { capture: true });
			reading?.removeEventListener('scroll', onViewportChange);
		};
	});

	function memberPreviewTracking(node: HTMLElement) {
		memberNodeEl = node;
		const resetPreview = () => {
			previewDismissed = false;
			previewFlipAbove = false;
			previewShiftX = 0;
			previewShiftY = 0;
			previewMaxHeight = undefined;
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
		const text = trimmed.slice(0, MAX_INBOX_COMMENT_LENGTH);
		const id =
			typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
				? crypto.randomUUID()
				: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
		const nextEntries: InboxCommentEntry[] = [
			...readStoredComments(organizationId, item.id),
			{ id, text, at: Date.now() }
		];
		try {
			window.localStorage.setItem(
				inboxCommentsStorageKey(organizationId, item.id),
				JSON.stringify(nextEntries)
			);
		} catch {
			toast.error('Could not save the comment on this device.');
			commentInputEl?.focus();
			return;
		}
		onCommentDraftClear(organizationId, item.id);
		commentVersion += 1;
		commentStatus = `Comment posted (${nextEntries.length} total).`;
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
			<!-- Mobile Back Button (< 960px) -->
			<button
				type="button"
				onclick={onBackMobile}
				class="triage-back inline-flex min-h-11 w-fit items-center gap-1.5 text-xs font-semibold"
			>
				<ArrowLeft class="size-4" aria-hidden="true" />
				<span>Back to Inbox</span>
			</button>

			<!-- Simplified Activity-Matched Sender Intro with Hover/Focus Member Preview Card -->
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

				<h2 id="triage-detail-heading" tabindex="-1" class="triage-detail-title font-semibold">
					{@render highlighted(item.title)}
				</h2>
			</header>

			<!-- Description -->
			{#if item.why}
				<p class="triage-description">
					{@render highlighted(item.why)}
				</p>
			{/if}

			<!-- Request Details -->
			<section class="triage-context" aria-labelledby="triage-context-title">
				<h3 id="triage-context-title">Request details</h3>
				<dl class="triage-fields">
					<div>
						<dt>Kind</dt>
						<dd>{item.kindDisplay}</dd>
					</div>

					<div>
						<dt>Project</dt>
						<dd>
							{#if item.projectId}
								<a
									class="triage-link"
									href={resolve(
										`/projects/${encodeURIComponent(item.projectId)}/work` as '/projects'
									)}
								>
									{item.projectName}
								</a>
							{:else}
								<span>{item.projectName}</span>
							{/if}
						</dd>
					</div>

					{#if item.teamName}
						<div>
							<dt>Team</dt>
							<dd>{item.teamName}</dd>
						</div>
					{/if}

					{#if item.workRef}
						{@const isCopied = copiedKey === `${item.id}:${item.workRef}`}
						<div>
							<dt>Work item</dt>
							<dd>
								{#if workParts}
									<a
										class="triage-link"
										href="{resolve(workParts.pathname as '/work')}{workParts.suffix}"
									>
										{item.workRefDisplay ?? item.workRef}
									</a>
								{:else}
									<span>{item.workRefDisplay ?? item.workRef}</span>
								{/if}
								<button
									type="button"
									onclick={() => void copyWorkRef(item.id, item.workRef!)}
									aria-label="Copy {item.workRef}"
									class={['triage-copy', isCopied && 'is-copied']}
								>
									{#if isCopied}
										<Check class="size-3.5 shrink-0" aria-hidden="true" />
										<span>Copied</span>
									{:else}
										<Copy class="size-3.5 shrink-0" aria-hidden="true" />
										<span>Copy</span>
									{/if}
								</button>
							</dd>
						</div>
					{/if}

					{#if item.expiresLabel}
						<div>
							<dt>Expires</dt>
							<dd class={item.state === 'open' ? 'triage-expiry' : undefined}>
								{item.expiresLabel}
							</dd>
						</div>
					{/if}

					<div>
						<dt>Clears</dt>
						<dd>{item.clearsLabel}</dd>
					</div>

					{#if relatedParts}
						<div>
							<dt>Related page</dt>
							<dd>
								<a
									class="triage-link"
									href="{resolve(relatedParts.pathname as '/work')}{relatedParts.suffix}"
								>
									{item.openLabel ?? 'Open related page'}
								</a>
							</dd>
						</div>
					{/if}
				</dl>
			</section>

			<!-- Decision Block: Recommendation Callout + Actions -->
			<div class="triage-decision">
				{#if item.recommendation}
					<div class="recommendation" data-tone={item.recommendationTone}>
						<span class="recommendation-ico" aria-hidden="true">
							{#if item.recommendationTone === 'ok'}
								<CheckCircle2 class="size-5" />
							{:else if item.recommendationTone === 'warn'}
								<AlertTriangle class="size-5" />
							{:else if item.recommendationTone === 'bad'}
								<ShieldAlert class="size-5" />
							{:else}
								<Info class="size-5" />
							{/if}
						</span>
						<div class="min-w-0">
							<h3 class="recommendation-label">{RECOMMENDATION_LABELS[item.recommendationTone]}</h3>
							<p>{item.recommendation}</p>
						</div>
					</div>
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
							<span class="triage-detail-muted font-mono tabular-nums"
								>· {item.handled.atLabel}</span
							>
						</div>
					{:else}
						{#if hasReply}
							<form class="flex w-full flex-col gap-2.5" onsubmit={handleReplySubmit}>
								<label class="sr-only" for="triage-reply-{item.id}">
									Reply to {item.actorName}
								</label>
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
										class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
												class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
												class="triage-secondary-btn inline-flex min-h-9 min-w-9 items-center justify-center gap-1.5 px-3.5 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
										class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
										class="exec-cta min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
										class="triage-deny-btn inline-flex min-h-9 min-w-9 items-center justify-center gap-1.5 px-3.5 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
									>
										<Ban class="size-3.5 shrink-0" aria-hidden="true" />
										<span>{item.decision?.secondaryAction?.label ?? 'Deny'}</span>
									</button>
								{/if}
							</div>

							<!-- Desktop keyboard ergonomics hint -->
							{#if shortcutsEnabled}
								<div class="triage-kbd-hints hidden items-center gap-2 text-[11px] lg:flex">
									<span class="inline-flex items-center gap-1">
										<kbd class="triage-kbd px-1.5 py-0.5 font-mono text-[10px]">J</kbd>/<kbd
											class="triage-kbd px-1.5 py-0.5 font-mono text-[10px]">K</kbd
										>
										<span>navigate</span>
									</span>
									{#if isExplicitApprove}
										<span class="inline-flex items-center gap-1">
											<kbd class="triage-kbd px-1.5 py-0.5 font-mono text-[10px]">A</kbd>
											<span>approve</span>
										</span>
									{/if}
									{#if canDeny}
										<span class="inline-flex items-center gap-1">
											<kbd class="triage-kbd px-1.5 py-0.5 font-mono text-[10px]">D</kbd>
											<span>deny</span>
										</span>
									{/if}
								</div>
							{/if}
						</div>

						<!-- Two-step Inline Deny Confirmation Guard -->
						{#if canDeny && isDenying}
							<div
								{@attach denyGuardEscape}
								class="triage-deny-guard flex w-full flex-col gap-2.5 p-3.5"
								role="group"
								aria-label="Confirm deny"
							>
								<div class="flex flex-wrap items-center justify-between gap-2">
									<span
										class="triage-deny-label inline-flex items-center gap-1.5 text-xs font-semibold"
									>
										<Ban class="size-3.5 shrink-0" aria-hidden="true" />
										<span>Confirm deny?</span>
									</span>
									<div class="flex items-center gap-2">
										<button
											type="button"
											disabled={controlsDisabled}
											onclick={() => void handleCancelDeny()}
											class="triage-keep-btn inline-flex min-h-9 min-w-9 items-center justify-center px-3 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
										>
											Keep
										</button>
										<button
											type="button"
											disabled={controlsDisabled}
											onclick={() => onSecondaryAction(item)}
											class="triage-deny-confirm inline-flex min-h-9 min-w-9 items-center justify-center px-3 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
										>
											{isBusy ? 'Denying…' : 'Confirm deny'}
										</button>
									</div>
								</div>
								<label class="sr-only" for="triage-deny-reason-{item.id}">
									Optional reason for {item.actorName}
								</label>
								<input
									bind:this={denyInputEl}
									value={declineReason}
									oninput={(e) => onDeclineReasonChange(item.id, e.currentTarget.value)}
									onkeydown={handleDenyInputKeydown}
									disabled={controlsDisabled}
									id="triage-deny-reason-{item.id}"
									type="text"
									autocomplete="off"
									placeholder="Optional reason for {item.actorName}… (Enter to confirm, Esc to cancel)"
									class="triage-input min-h-9 w-full px-3 py-1.5 text-xs disabled:cursor-not-allowed disabled:opacity-60 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
								/>
							</div>
						{/if}
					{/if}
				</div>
			</div>

			<!-- Activity: Interactive Comments + Request History Timeline -->
			<section class="triage-activity" aria-labelledby="triage-activity-title">
				<h3 id="triage-activity-title">Activity</h3>

				<form class="triage-comment-form" onsubmit={handleCommentSubmit}>
					<label class="sr-only" for="triage-comment">Comment on this request</label>
					<textarea
						bind:this={commentInputEl}
						id="triage-comment"
						name="comment"
						rows="2"
						maxlength={MAX_INBOX_COMMENT_LENGTH}
						placeholder="Leave a comment…"
						value={commentDraft}
						oninput={(e) => updateCommentDraft(e.currentTarget.value)}
						required></textarea>
					<button type="submit" aria-label="Send comment" title="Send comment" class="triage-send">
						<Send class="size-4" aria-hidden="true" />
					</button>
				</form>
				<p id="triage-comment-status" class="sr-only" role="status">{commentStatus}</p>

				{#if comments.length > 0}
					<section class="triage-comments" aria-labelledby="triage-comments-title">
						<h4 id="triage-comments-title">Comments</h4>
						<ol class="triage-comment-list" role="list" aria-labelledby="triage-comments-title">
							{#each comments as entry (entry.id)}
								<li>
									<span class="triage-comment-av" aria-hidden="true">Y</span>
									<div>
										<p>
											<strong>You</strong>
											<time class="triage-event-time" datetime={new Date(entry.at).toISOString()}>
												{formatCommentTime(entry.at)}
											</time>
										</p>
										<p class="triage-comment-text">{entry.text}</p>
									</div>
								</li>
							{/each}
						</ol>
					</section>
				{/if}

				<div class="triage-history">
					<h4 id="triage-history-title">Request history</h4>
					<ol class="triage-activity-log" role="list" aria-labelledby="triage-history-title">
						<li>
							<span class="triage-event-marker" aria-hidden="true">
								<Inbox class="size-4" />
							</span>
							<p>
								<strong>{item.actorName}</strong> opened this request
								<span class="triage-event-time">{item.createdAgeLabel}</span>
							</p>
						</li>

						{#if item.state === 'open'}
							<li>
								<span class="triage-event-marker" aria-hidden="true">
									<Clock class="size-4" />
								</span>
								<p>
									{item.category === 'approval' ? 'Waiting for approval' : 'Waiting for your input'}
								</p>
							</li>
						{/if}

						{#each item.auditChecks as chk, idx (`${idx}-${chk}`)}
							<li>
								<span class="triage-event-marker" aria-hidden="true">
									<CheckCircle2 class="size-4" />
								</span>
								<p><span class="sr-only">Audit entry. </span>{chk}</p>
							</li>
						{/each}

						{#if item.state === 'handled' && item.handled}
							<li>
								<span class="triage-event-marker" aria-hidden="true">
									<CheckCircle2 class="size-4" />
								</span>
								<p>
									{item.handled.outcome}{item.handled.by ? ` · by ${item.handled.by}` : ''}
									<time class="triage-event-time" datetime={item.handled.atIso}>
										{item.handled.atLabel}
									</time>
								</p>
							</li>
						{/if}
					</ol>
				</div>
			</section>
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
		min-height: 36px;
		color: var(--exec-ink);
		font-weight: 600;
		text-decoration: none;
	}
	.triage-from-link:is(:hover, :focus-visible) {
		text-decoration: underline;
	}
	/* Outer popover sits flush against trigger with 4px padding bridge (never clipped by card scroll) */
	.triage-from-popover {
		position: absolute;
		z-index: 10;
		inset-block-start: 100%;
		inset-inline-start: 0;
		display: none;
		padding-block-start: 4px;
		transform: translate(var(--preview-shift-x, 0px), var(--preview-shift-y, 0px));
	}
	.triage-from-popover[data-flip-above] {
		inset-block-start: auto;
		inset-block-end: 100%;
		padding-block-start: 0;
		padding-block-end: 4px;
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
	.triage-detail-title:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.triage-description {
		max-width: 75ch;
		color: var(--exec-body);
		overflow-wrap: anywhere;
	}
	.triage-context,
	.triage-decision {
		display: grid;
		min-width: 0;
		gap: 12px;
	}
	.triage-context {
		padding-block: 12px 14px;
		border-block: 1px solid var(--exec-border);
	}
	.triage-context h3 {
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 600;
	}
	.triage-fields {
		display: grid;
		gap: 10px 16px;
	}
	.triage-fields > div {
		display: grid;
		min-width: 0;
		align-content: start;
		gap: 2px;
	}
	@container inbox-reading (max-width: 29.99rem) {
		.triage-fields > div {
			grid-template-columns: minmax(5.25rem, 24%) minmax(0, 1fr);
			align-items: baseline;
			gap: 8px;
		}
	}
	@container inbox-reading (min-width: 30rem) {
		.triage-fields {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
	}
	@container inbox-reading (min-width: 47rem) {
		.triage-fields {
			grid-template-columns: repeat(3, minmax(0, 1fr));
		}
	}
	.triage-fields dt {
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.triage-fields dd {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 4px 8px;
		min-width: 0;
		color: var(--exec-ink);
		font-size: 0.875rem;
		overflow-wrap: anywhere;
	}
	.triage-fields dd.triage-expiry {
		color: var(--tone-warn-ink);
	}
	.triage-link {
		display: inline-flex;
		align-items: center;
		min-height: 36px;
		color: var(--link);
		text-decoration: none;
	}
	.triage-link:is(:hover, :focus-visible) {
		text-decoration: underline;
	}
	.triage-copy {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		min-height: 36px;
		padding: 0 10px;
		border: 1px solid var(--exec-border-hi);
		border-radius: 6px;
		background: transparent;
		color: var(--exec-ink);
		font-size: 0.75rem;
		font-weight: 500;
		cursor: pointer;
	}
	.triage-copy:hover {
		background: var(--exec-btn-ghost-bg-hover);
	}
	.triage-copy.is-copied {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		border-color: var(--exec-ok-ring);
	}
	.recommendation {
		--recommendation-ink: var(--exec-body);
		--recommendation-ring: var(--exec-border-hi);
		--recommendation-fill: var(--exec-well);
		display: flex;
		align-items: flex-start;
		gap: 10px;
		padding: 12px 14px;
		border: 1px solid var(--recommendation-ring);
		border-inline-start: 3px solid var(--recommendation-ink);
		border-radius: var(--exec-inset-radius);
		background: var(--recommendation-fill);
		color: var(--exec-ink);
		font-size: 0.875rem;
		line-height: 1.5;
		overflow-wrap: anywhere;
	}
	.recommendation p {
		max-width: 64ch;
	}
	.recommendation[data-tone='ok'] {
		--recommendation-ink: var(--tone-ok-ink);
		--recommendation-ring: var(--tone-ok-ring);
		--recommendation-fill: var(--tone-ok-bg);
	}
	.recommendation[data-tone='warn'] {
		--recommendation-ink: var(--tone-warn-ink);
		--recommendation-ring: var(--tone-warn-ring);
		--recommendation-fill: var(--tone-warn-bg);
	}
	.recommendation[data-tone='bad'] {
		--recommendation-ink: var(--tone-bad-ink);
		--recommendation-ring: var(--tone-bad-ring);
		--recommendation-fill: var(--tone-bad-bg);
	}
	.recommendation-ico,
	.recommendation-label {
		color: var(--recommendation-ink);
	}
	.recommendation-ico {
		display: inline-flex;
		flex: none;
		margin-top: 1px;
	}
	.recommendation-label {
		margin-bottom: 3px;
		font-size: 0.75rem;
		font-weight: 650;
		letter-spacing: 0.04em;
		text-transform: uppercase;
	}
	.triage-actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
	}
	.triage-ink {
		color: var(--exec-ink);
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
	.triage-deny-guard {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-deny-guard-bg);
		box-shadow: inset 0 0 0 1px var(--exec-deny-guard-ring);
	}
	.triage-deny-label {
		color: var(--exec-deny-ink);
	}
	.triage-keep-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
	}
	.triage-keep-btn:hover:not(:disabled) {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.triage-deny-confirm {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-deny-confirm-bg);
		color: var(--exec-deny-confirm-ink);
		cursor: pointer;
	}
	.triage-deny-confirm:hover:not(:disabled) {
		background: var(--exec-deny-confirm-hover);
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
	.triage-hit {
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
		border-radius: 3px;
		padding: 0 2px;
		font-weight: 600;
	}

	/* Activity: Comments + Request History Timeline */
	.triage-activity {
		padding-block-start: 24px;
		border-top: 1px solid var(--exec-border);
	}
	.triage-activity > h3 {
		margin-block-end: 14px;
		color: var(--exec-ink);
		font-size: 1.125rem;
		font-weight: 600;
	}
	.triage-activity h4 {
		margin-block-end: 8px;
		color: var(--exec-ink);
		font-size: 1rem;
		font-weight: 600;
	}
	.triage-comments,
	.triage-history {
		margin-block-start: 24px;
	}
	.triage-comment-form {
		display: grid;
		justify-items: end;
		gap: 6px;
		padding: 10px 12px;
		border: 1px solid var(--exec-border-hi);
		border-radius: var(--exec-inset-radius);
		background: var(--exec-well);
	}
	.triage-comment-form:has(textarea:focus-visible) {
		border-color: var(--focus);
		outline: 2px solid var(--focus);
		outline-offset: -1px;
	}
	.triage-comment-form textarea {
		width: 100%;
		min-width: 0;
		min-height: calc(2lh + 4px);
		resize: vertical;
		padding: 2px 0;
		border: 0;
		outline: none;
		background: transparent;
		-webkit-backdrop-filter: none;
		backdrop-filter: none;
		box-shadow: none;
		color: var(--exec-ink);
		font: inherit;
	}
	@supports (field-sizing: content) {
		.triage-comment-form textarea {
			field-sizing: content;
			resize: none;
		}
	}
	.triage-comment-form textarea::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	.triage-send {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 36px;
		min-height: 36px;
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-send:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
	}
	.triage-activity-log li,
	.triage-comment-list li {
		position: relative;
		display: grid;
		grid-template-columns: 24px minmax(0, 1fr);
		align-items: start;
		gap: 10px;
		min-height: 42px;
		padding-block: 8px;
		color: var(--exec-body);
		font-size: 0.875rem;
		overflow-wrap: anywhere;
	}
	.triage-activity-log li:not(:last-child)::after {
		content: '';
		position: absolute;
		inset-block: 33px 1px;
		inset-inline-start: 11px;
		width: 1px;
		background: var(--exec-border-hi);
	}
	.triage-event-marker {
		display: grid;
		place-items: center;
		width: 24px;
		height: 24px;
		color: var(--exec-muted);
	}
	.triage-comment-av {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 24px;
		height: 24px;
		border-radius: 9999px;
		background: var(--exec-avatar-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.6875rem;
		font-weight: 700;
	}
	.triage-activity strong {
		color: var(--exec-ink);
		font-weight: 600;
	}
	.triage-event-time {
		margin-inline-start: 8px;
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.triage-comment-text {
		margin-block-start: 4px;
		white-space: pre-wrap;
	}

	@media (max-width: 720px), (pointer: coarse) {
		.triage-from-link,
		.triage-link,
		.triage-copy,
		.triage-send {
			min-height: 44px;
		}
		.triage-copy,
		.triage-send {
			min-width: 44px;
		}
		.triage-send {
			width: 44px;
		}
	}

	.triage-back:focus-visible,
	.triage-from-link:focus-visible,
	.triage-link:focus-visible,
	.triage-copy:focus-visible,
	.triage-send:focus-visible,
	.triage-secondary-btn:focus-visible,
	.triage-deny-btn:focus-visible,
	.triage-keep-btn:focus-visible,
	.triage-deny-confirm:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	@media (forced-colors: active) {
		.recommendation,
		.triage-deny-guard,
		.triage-copy,
		.triage-send,
		.triage-back,
		.triage-secondary-btn,
		.triage-deny-btn,
		.triage-keep-btn,
		.triage-deny-confirm,
		.triage-actions :global(.exec-cta) {
			border: 1px solid ButtonText;
		}
		.triage-input,
		.triage-comment-form {
			border: 1px solid CanvasText;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.triage-copy,
		.triage-send,
		.triage-secondary-btn,
		.triage-deny-btn {
			transition: none;
		}
	}
</style>
