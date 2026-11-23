<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Ban from '@lucide/svelte/icons/ban';
	import BellRing from '@lucide/svelte/icons/bell-ring';
	import Check from '@lucide/svelte/icons/check';
	import CheckCheck from '@lucide/svelte/icons/check-check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import Copy from '@lucide/svelte/icons/copy';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import FolderGit2 from '@lucide/svelte/icons/folder-git-2';
	import Send from '@lucide/svelte/icons/send';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import { isDenyAction } from '$lib/home/model';
	import { highlightSegments, isDeniedOutcome, splitHref, type TriageItem } from './triage-model';

	let {
		item,
		searchQuery,
		busyId,
		actionLocked,
		shortcutsEnabled,
		decliningId,
		declineReason,
		replyText,
		onDeclineReasonChange,
		onReplyTextChange,
		onBackMobile,
		onFilterProject,
		onPrimaryAction,
		onSecondaryAction,
		onArmDeny,
		onCancelDeny,
		onChoose,
		onReply
	}: {
		item?: TriageItem;
		searchQuery: string;
		busyId?: string;
		actionLocked: boolean;
		shortcutsEnabled: boolean;
		decliningId?: string;
		declineReason: string;
		replyText: string;
		onDeclineReasonChange: (itemId: string, value: string) => void;
		onReplyTextChange: (itemId: string, value: string) => void;
		onBackMobile: () => void;
		onFilterProject: (projectId: string) => void;
		onPrimaryAction: (item: TriageItem) => void;
		onSecondaryAction: (item: TriageItem) => void;
		onArmDeny: (id: string) => void;
		onCancelDeny: () => void;
		onChoose: (item: TriageItem, choiceId: string, choiceLabel: string) => void;
		onReply: (item: TriageItem, text: string) => void;
	} = $props();

	let denyInputEl = $state<HTMLInputElement | null>(null);
	let denyBtnEl = $state<HTMLButtonElement | null>(null);
	let copiedKey = $state<string | undefined>(undefined);
	let copyTimer: ReturnType<typeof setTimeout> | undefined;

	const isBusy = $derived(Boolean(item && busyId === item.id));
	const controlsDisabled = $derived(isBusy || actionLocked);
	const isDenying = $derived(Boolean(item && decliningId === item.id));
	const openParts = $derived(item?.openHref ? splitHref(item.openHref) : undefined);
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

	$effect(() => {
		if (isDenying) {
			void tick().then(() => denyInputEl?.focus());
		}
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
</script>

{#snippet highlighted(text: string)}
	{#each highlightSegments(text, searchQuery) as seg, i (`${i}-${seg.match}`)}
		{#if seg.match}<mark class="triage-hit">{seg.text}</mark>{:else}{seg.text}{/if}
	{/each}
{/snippet}

<section
	class="triage-reading flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto"
	aria-label="Item details"
>
	{#if !item}
		<div class="flex flex-1 flex-col items-center justify-center gap-3 px-6 py-16 text-center">
			<span class="triage-empty-badge flex size-12 items-center justify-center">
				<CheckCheck class="size-6" aria-hidden="true" />
			</span>
			<h2 class="triage-detail-heading text-base font-semibold">Queue clear for this view</h2>
			<p class="triage-detail-muted max-w-sm text-sm leading-relaxed">
				Select an item from the queue on the left or switch tabs to review other approvals and
				decisions.
			</p>
		</div>
	{:else}
		<article
			class="triage-document flex w-full max-w-[820px] flex-col gap-4 px-5 py-5 sm:px-8 sm:py-6"
		>
			<!-- Mobile Back Button (< 960px) -->
			<button
				type="button"
				onclick={onBackMobile}
				class="triage-back inline-flex min-h-11 w-fit items-center gap-1.5 px-3 text-xs font-semibold"
			>
				<ArrowLeft class="size-4" aria-hidden="true" />
				<span>Back to Inbox</span>
			</button>

			<!-- Sender Identity & Status Header -->
			<header class="flex items-start gap-3.5">
				<div class="relative shrink-0">
					<span
						class="triage-avatar-lg inline-flex size-10 items-center justify-center rounded-full text-sm font-bold"
					>
						{item.actorInitials}
					</span>
					{#if item.isAgent}
						<span
							class="triage-agent-dot absolute -right-0.5 -bottom-0.5 size-3 rounded-full"
							title="Agent"
							aria-hidden="true"
						></span>
					{/if}
				</div>

				<div class="min-w-0 flex-1">
					<div class="flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-sm">
						<span class="triage-ink font-semibold">{item.actorName}</span>
						<span class="triage-detail-muted" aria-hidden="true">·</span>
						<span class="triage-detail-muted">{item.actorRole}</span>
					</div>
					<div class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
						{#if item.state === 'open'}
							<span class="triage-status-needs inline-flex items-center gap-1 font-semibold">
								<BellRing class="size-3.5 shrink-0" aria-hidden="true" />
								<span>Needs you</span>
							</span>
						{:else}
							<span
								class={[
									'triage-status-handled inline-flex items-center gap-1 font-semibold',
									deniedHandled ? 'is-denied' : 'is-ok'
								]}
							>
								<CheckCircle2 class="size-3.5 shrink-0" aria-hidden="true" />
								<span>Handled ({item.handled?.outcome ?? 'Resolved'})</span>
							</span>
						{/if}
						<span class="triage-detail-muted" aria-hidden="true">·</span>
						<span class="triage-body">{item.typeLabel}</span>
						<span class="triage-detail-muted" aria-hidden="true">·</span>
						<span class="triage-detail-muted font-mono text-[11px] tabular-nums"
							>{item.ageLabel}</span
						>
					</div>
				</div>
			</header>

			<!-- Display Detail Heading -->
			<h2 id="triage-detail-heading" tabindex="-1" class="triage-title font-semibold">
				{@render highlighted(item.title)}
			</h2>

			<!-- Why Narrative Block -->
			<div class="flex flex-col gap-1.5">
				<h3 class="triage-kicker font-mono text-[11px] font-semibold tracking-wider uppercase">
					Why
				</h3>
				<p class="triage-why max-w-[60ch] text-sm leading-relaxed">
					{@render highlighted(item.why)}
				</p>
			</div>

			<!-- Elevated Recommendation Callout Card -->
			{#if item.recommendation}
				<div
					class={[
						'triage-callout flex max-w-[64ch] items-start gap-3 px-4 py-3.5',
						`tone-${item.recommendationTone}`
					]}
				>
					<span class="triage-callout-icon mt-0.5 inline-flex shrink-0" aria-hidden="true">
						{#if item.recommendationTone === 'warn'}
							<AlertTriangle class="size-4" />
						{:else if item.recommendationTone === 'accent'}
							<Sparkles class="size-4" />
						{:else}
							<CheckCircle2 class="size-4" />
						{/if}
					</span>
					<div class="min-w-0">
						<div
							class="triage-callout-kicker font-mono text-[11px] font-semibold tracking-wider uppercase"
						>
							Recommendation
						</div>
						<p class="triage-ink mt-0.5 text-sm leading-snug font-medium">
							{item.recommendation}
						</p>
					</div>
				</div>
			{/if}

			<!-- Structured Metadata Table -->
			<dl
				class="triage-fields grid max-w-[64ch] grid-cols-[minmax(84px,112px)_minmax(0,1fr)] items-center gap-x-4 gap-y-2 py-3"
			>
				<dt class="triage-field-label text-xs font-medium">Kind</dt>
				<dd class="triage-ink min-w-0 text-sm font-medium [overflow-wrap:anywhere]">
					{item.kindDisplay}
				</dd>

				<dt class="triage-field-label text-xs font-medium">Project</dt>
				<dd class="min-w-0">
					{#if item.projectId}
						<button
							type="button"
							onclick={() => onFilterProject(item.projectId!)}
							class="triage-project-chip inline-flex h-auto min-h-9 min-w-9 max-w-full items-center gap-1.5 px-2.5 py-1.5 text-left text-xs font-medium whitespace-normal [overflow-wrap:anywhere] max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							title="Filter inbox by {item.projectName}"
						>
							<FolderGit2 class="size-3.5 shrink-0" aria-hidden="true" />
							<span class="min-w-0 [overflow-wrap:anywhere]">{item.projectName}</span>
						</button>
					{:else}
						<span class="triage-body text-sm [overflow-wrap:anywhere]">{item.projectName}</span>
					{/if}
				</dd>

				{#if item.teamName}
					<dt class="triage-field-label text-xs font-medium">Team</dt>
					<dd class="triage-body min-w-0 text-sm [overflow-wrap:anywhere]">{item.teamName}</dd>
				{/if}

				{#if item.workRef}
					{@const isCopied = copiedKey === `${item.id}:${item.workRef}`}
					<dt class="triage-field-label text-xs font-medium">Work item</dt>
					<dd class="flex min-w-0 flex-wrap items-center gap-2">
						<button
							type="button"
							onclick={() => void copyWorkRef(item.id, item.workRef!)}
							title="Copy {item.workRef}"
							class={[
								'triage-copy-pill inline-flex min-h-9 max-w-full items-center gap-1.5 px-2.5 font-mono text-xs max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
								isCopied && 'is-copied'
							]}
						>
							{#if isCopied}
								<Check class="size-3.5 shrink-0" aria-hidden="true" />
								<span>Copied {item.workRef}</span>
							{:else}
								<span class="truncate">{item.workRefDisplay ?? item.workRef}</span>
								<Copy class="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
							{/if}
						</button>
					</dd>
				{/if}

				{#if item.state === 'open' && item.expiresLabel}
					<dt class="triage-field-label text-xs font-medium">Expires</dt>
					<dd>
						<span
							class="triage-expiry-pill inline-flex items-center gap-1.5 px-2.5 py-0.5 text-xs font-semibold"
						>
							<Clock class="size-3.5 shrink-0" aria-hidden="true" />
							<span>{item.expiresLabel}</span>
						</span>
					</dd>
				{/if}

				<dt class="triage-field-label text-xs font-medium">Clears</dt>
				<dd class="triage-body min-w-0 text-sm [overflow-wrap:anywhere]">{item.clearsLabel}</dd>
			</dl>

			<!-- Collapsible Automated Checks & Audit Trail -->
			{#if item.auditChecks.length > 0}
				<details class="triage-audit max-w-[64ch]">
					<summary
						class="triage-audit-summary flex min-h-9 items-center gap-2 px-3.5 py-2 text-xs font-semibold max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						<span class="triage-audit-chevron inline-flex shrink-0" aria-hidden="true">
							<ChevronRight class="size-3.5" />
						</span>
						<span class="triage-audit-dot size-2 shrink-0 rounded-full" aria-hidden="true"></span>
						<span class="triage-ink">Automated checks &amp; audit trail</span>
						<span
							class="triage-detail-muted ml-auto font-mono text-[11px] font-normal tabular-nums"
						>
							{item.auditChecks.length}
							{item.auditChecks.length === 1 ? 'check' : 'checks'}
						</span>
					</summary>
					<ul class="triage-audit-list px-4 pb-3" role="list">
						{#each item.auditChecks as chk, idx (`${idx}-${chk}`)}
							<li class="triage-audit-row flex items-center gap-2 py-2 text-xs">
								<span class="triage-audit-ok inline-flex shrink-0" aria-hidden="true">
									<CheckCircle2 class="size-3.5" />
								</span>
								<span class="triage-body">{chk}</span>
							</li>
						{/each}
					</ul>
				</details>
			{/if}

			<!-- Tactile Action Bar -->
			<div class="triage-action-wrap flex max-w-[64ch] flex-col gap-3 pt-4">
				{#if item.state === 'handled' && item.handled}
					<div class="flex flex-wrap items-center justify-between gap-3">
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
						{#if openParts}
							<a
								href="{resolve(openParts.pathname as '/work')}{openParts.suffix}"
								class="triage-ghost-link inline-flex min-h-9 items-center gap-1.5 px-3 text-xs font-semibold max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
							>
								<ExternalLink class="size-3.5 shrink-0" aria-hidden="true" />
								<span>{item.openLabel ?? `Open ${item.workRef ?? 'item'}`}</span>
							</a>
						{/if}
					</div>
				{:else}
					{#if hasReply}
						<form class="flex flex-col gap-2.5" onsubmit={handleReplySubmit}>
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
									class="triage-input min-h-9 min-w-[200px] flex-1 px-3 py-1.5 text-sm max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
								/>
								<button
									type="submit"
									disabled={controlsDisabled || !replyText.trim()}
									class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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

					<div class="flex flex-wrap items-center justify-between gap-3">
						<div class="flex flex-wrap items-center gap-2">
							{#if hasChoices && item.inboxItem?.choices}
								{#each item.inboxItem.choices as choice, idx (choice.id)}
									{#if idx === 0}
										<button
											type="button"
											disabled={controlsDisabled}
											onclick={() => onChoose(item, choice.id, choice.label)}
											class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
											class="triage-secondary-btn inline-flex min-h-9 min-w-9 items-center justify-center gap-1.5 px-3.5 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
									class="exec-cta min-w-9 disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
									class="exec-cta min-w-9 max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
									class="triage-deny-btn inline-flex min-h-9 min-w-9 items-center justify-center gap-1.5 px-3.5 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
								>
									<Ban class="size-3.5 shrink-0" aria-hidden="true" />
									<span>{item.decision?.secondaryAction?.label ?? 'Deny'}</span>
								</button>
							{/if}

							{#if !isOpenOnly && openParts}
								<a
									href="{resolve(openParts.pathname as '/work')}{openParts.suffix}"
									class="triage-ghost-link inline-flex min-h-9 min-w-9 items-center gap-1.5 px-3 text-xs font-semibold max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
								>
									<ExternalLink class="size-3.5 shrink-0" aria-hidden="true" />
									<span>{item.openLabel ?? `Open ${item.workRef ?? 'item'}`}</span>
								</a>
							{/if}
						</div>

						<!-- Desktop keyboard ergonomics hint (hidden when Single-key shortcuts are disabled) -->
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

					<!-- Two-step Inline Deny Confirmation Guard (Escape works from any child control) -->
					{#if canDeny && isDenying}
						<div
							{@attach denyGuardEscape}
							class="triage-deny-guard flex flex-col gap-2.5 p-3.5"
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
										class="triage-keep-btn inline-flex min-h-9 min-w-9 items-center justify-center px-3 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
									>
										Keep
									</button>
									<button
										type="button"
										disabled={controlsDisabled}
										onclick={() => onSecondaryAction(item)}
										class="triage-deny-confirm inline-flex min-h-9 min-w-9 items-center justify-center px-3 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 max-sm:min-h-11 max-sm:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
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
								class="triage-input min-h-9 w-full px-3 py-1.5 text-xs disabled:cursor-not-allowed disabled:opacity-60 max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11"
							/>
						</div>
					{/if}
				{/if}
			</div>
		</article>
	{/if}
</section>

<style>
	.triage-reading {
		overscroll-behavior-y: contain;
	}
	.triage-back {
		display: none;
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
	}
	.triage-back:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	@media (max-width: 959px) {
		.triage-back {
			display: inline-flex;
		}
	}
	.triage-ink {
		color: var(--exec-ink);
	}
	.triage-body {
		color: var(--exec-body);
	}
	.triage-detail-muted,
	.triage-kicker,
	.triage-field-label,
	.triage-kbd-hints {
		color: var(--exec-muted);
	}
	.triage-detail-heading {
		color: var(--exec-ink);
	}
	.triage-empty-badge {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	.triage-avatar-lg {
		background: var(--exec-avatar-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.triage-agent-dot {
		background: var(--tone-warn);
		box-shadow: 0 0 0 2px var(--exec-surface);
	}
	.triage-status-needs {
		color: var(--exec-warn);
	}
	.triage-status-handled.is-ok {
		color: var(--exec-ok);
	}
	.triage-status-handled.is-denied {
		color: var(--exec-bad);
	}
	.triage-title {
		color: var(--exec-ink);
		font-size: clamp(1.25rem, 1.7vw, 1.55rem);
		line-height: 1.28;
		letter-spacing: -0.02em;
		overflow-wrap: anywhere;
	}
	.triage-title:focus {
		outline: none;
	}
	.triage-title:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.triage-why {
		color: var(--exec-body);
		overflow-wrap: anywhere;
	}
	.triage-callout {
		border-radius: var(--exec-inset-radius);
	}
	.triage-callout.tone-ok {
		background: var(--exec-ok-bg);
		box-shadow:
			inset 3px 0 0 var(--tone-ok),
			inset 0 0 0 1px var(--exec-ok-ring);
	}
	.triage-callout.tone-ok .triage-callout-icon,
	.triage-callout.tone-ok .triage-callout-kicker {
		color: var(--exec-ok);
	}
	.triage-callout.tone-warn {
		background: var(--exec-warn-bg);
		box-shadow:
			inset 3px 0 0 var(--tone-warn),
			inset 0 0 0 1px var(--exec-warn-ring);
	}
	.triage-callout.tone-warn .triage-callout-icon,
	.triage-callout.tone-warn .triage-callout-kicker {
		color: var(--exec-warn);
	}
	.triage-callout.tone-accent {
		background: var(--exec-accent-soft);
		box-shadow:
			inset 3px 0 0 var(--tone-accent),
			inset 0 0 0 1px var(--exec-accent-ring);
	}
	.triage-callout.tone-accent .triage-callout-icon,
	.triage-callout.tone-accent .triage-callout-kicker {
		color: var(--exec-accent-ink);
	}
	.triage-fields {
		border-block: 1px solid var(--exec-border);
	}
	.triage-project-chip {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-project-chip:hover {
		background: var(--tone-accent-bg);
		color: var(--exec-ink);
	}
	.triage-copy-pill {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		cursor: pointer;
		max-width: 100%;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out,
			box-shadow 120ms ease-out;
	}
	.triage-copy-pill:hover {
		background: var(--exec-surface-hi);
	}
	.triage-copy-pill.is-copied {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	.triage-expiry-pill {
		border-radius: var(--exec-pill-radius);
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: var(--exec-pill-ring-warn);
	}
	.triage-audit {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.triage-audit-summary {
		cursor: pointer;
		list-style: none;
	}
	.triage-audit-summary::-webkit-details-marker {
		display: none;
	}
	.triage-audit-chevron {
		color: var(--exec-muted);
		transition: transform 140ms ease-out;
	}
	.triage-audit[open] .triage-audit-chevron {
		transform: rotate(90deg);
	}
	.triage-audit-dot {
		background: var(--tone-ok);
	}
	.triage-audit-row {
		border-top: 1px solid var(--exec-border);
	}
	.triage-audit-ok {
		color: var(--exec-ok);
	}
	.triage-action-wrap {
		border-top: 1px solid var(--exec-border);
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
	.triage-ghost-link {
		border-radius: var(--exec-ctrl-radius);
		color: var(--exec-accent-ink);
		transition: background-color 120ms ease-out;
	}
	.triage-ghost-link:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
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

	.triage-back:focus-visible,
	.triage-project-chip:focus-visible,
	.triage-copy-pill:focus-visible,
	.triage-audit-summary:focus-visible,
	.triage-secondary-btn:focus-visible,
	.triage-deny-btn:focus-visible,
	.triage-ghost-link:focus-visible,
	.triage-keep-btn:focus-visible,
	.triage-deny-confirm:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	@media (forced-colors: active) {
		.triage-callout,
		.triage-audit,
		.triage-deny-guard,
		.triage-project-chip,
		.triage-copy-pill,
		.triage-back,
		.triage-ghost-link,
		.triage-secondary-btn,
		.triage-deny-btn,
		.triage-keep-btn,
		.triage-deny-confirm,
		.triage-action-wrap :global(.exec-cta) {
			border: 1px solid ButtonText;
		}
		.triage-input {
			border: 1px solid CanvasText;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.triage-audit-chevron,
		.triage-project-chip,
		.triage-copy-pill,
		.triage-secondary-btn,
		.triage-deny-btn,
		.triage-ghost-link {
			transition: none;
		}
	}
</style>
