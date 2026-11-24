<script lang="ts">
	import { onDestroy, tick, type Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Copy from '@lucide/svelte/icons/copy';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import FileCheck2 from '@lucide/svelte/icons/file-check-2';
	import PauseCircle from '@lucide/svelte/icons/pause-circle';
	import X from '@lucide/svelte/icons/x';
	import { toast } from 'svelte-sonner';
	import type { WorkStatusRingTone } from '$lib/modules/work/compact';

	let {
		itemId,
		refLabel,
		kindLabel,
		ariaLabel,
		fullPageHref,
		statusTone,
		needsAttention = false,
		priorityLevel,
		priorityLabel,
		priorityStrong = false,
		onclose,
		children
	}: {
		itemId: string;
		refLabel: string;
		kindLabel: 'Task' | 'Bug' | 'Request';
		ariaLabel: string;
		fullPageHref?: string;
		statusTone?: WorkStatusRingTone;
		needsAttention?: boolean;
		priorityLevel?: 0 | 1 | 2 | 3;
		priorityLabel?: string;
		priorityStrong?: boolean;
		onclose: () => void;
		children: Snippet;
	} = $props();

	let panelEl = $state<HTMLElement | null>(null);
	let isOpening = $state(true);
	let copiedForId = $state<string | null>(null);
	const copied = $derived(copiedForId === itemId);
	let copyTimer: ReturnType<typeof setTimeout> | undefined;

	onDestroy(() => {
		clearTimeout(copyTimer);
	});

	$effect(() => {
		void itemId;
		let cancelled = false;
		void tick().then(() => {
			if (cancelled || !panelEl) return;
			panelEl.scrollTop = 0;
			const scroller = panelEl.closest<HTMLElement>('main');
			if (scroller) scroller.scrollTop = 0;
			if (!panelEl.contains(document.activeElement)) {
				const titleEl = panelEl.querySelector<HTMLElement>('#work-detail-title');
				titleEl?.focus({ preventScroll: true });
				titleEl?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
			}
		});
		return () => {
			cancelled = true;
		};
	});

	async function copyRef() {
		const targetId = itemId;
		const targetRef = refLabel;
		try {
			await navigator.clipboard.writeText(targetRef);
			if (targetId !== itemId) return;
			copiedForId = targetId;
			clearTimeout(copyTimer);
			copyTimer = setTimeout(() => {
				if (copiedForId === targetId) copiedForId = null;
			}, 3000);
		} catch {
			toast.error("Couldn't copy reference.");
		}
	}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
<aside
	bind:this={panelEl}
	class={['work-reading', isOpening && 'is-opening']}
	role="dialog"
	aria-modal="false"
	aria-label={ariaLabel}
	data-testid="work-reading-panel"
	onanimationend={(event) => {
		if (event.target === event.currentTarget) isOpening = false;
	}}
>
	<article class="work-document">
		<div class="work-detail-bar">
			<button type="button" class="work-back-btn" onclick={onclose}>
				<ArrowLeft class="size-4 shrink-0" aria-hidden="true" />
				<span>Back to Work</span>
			</button>
			<p class="work-detail-id">
				{#if priorityLevel !== undefined && priorityLabel}
					<span
						class="work-priority-mark"
						data-level={priorityLevel}
						data-priority-level={priorityLevel}
						data-tone={priorityStrong ? 'strong' : undefined}
						role="img"
						aria-label={priorityLabel}
						title={priorityLabel}
					>
						<span></span><span></span><span></span>
					</span>
					{#if priorityStrong}
						<span class="work-detail-urgent">Urgent</span>
					{/if}
				{/if}
				<span class="work-detail-ref">{refLabel}</span>
				{#if statusTone}
					<span
						class="work-reading-status"
						data-tone={statusTone}
						data-attention={needsAttention ? '' : undefined}
						aria-hidden="true"
					>
						{#if statusTone === 'done'}
							<CheckCircle2 class="size-3.5" />
						{:else if statusTone === 'blocked'}
							<PauseCircle class="size-3.5" />
						{:else if statusTone === 'review'}
							<FileCheck2 class="size-3.5" />
						{:else}
							<span class="work-reading-status-ring"></span>
						{/if}
					</span>
				{/if}
				<span class="work-kind">{kindLabel}</span>
			</p>

			<Tooltip.Provider delayDuration={150} ignoreNonKeyboardFocus>
				<div class="work-detail-actions">
					{#if fullPageHref}
						<Tooltip.Root ignoreNonKeyboardFocus>
							<Tooltip.Trigger>
								{#snippet child({ props })}
									<a
										{...props}
										href={resolve(fullPageHref as '/')}
										class="work-detail-btn"
										aria-label="Open full page for {refLabel}"
									>
										<ExternalLink class="size-4" aria-hidden="true" />
									</a>
								{/snippet}
							</Tooltip.Trigger>
							<Tooltip.Portal>
								<Tooltip.Content
									side="bottom"
									sideOffset={6}
									escapeKeydownBehavior="defer-otherwise-close"
									class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
									>Open full page</Tooltip.Content
								>
							</Tooltip.Portal>
						</Tooltip.Root>
					{/if}
					<Tooltip.Root ignoreNonKeyboardFocus>
						<Tooltip.Trigger>
							{#snippet child({ props })}
								<button
									{...props}
									type="button"
									class="work-detail-btn"
									aria-label={copied ? `Copied ${refLabel}` : `Copy ${refLabel}`}
									onclick={copyRef}
								>
									{#if copied}
										<Check class="size-4" aria-hidden="true" />
									{:else}
										<Copy class="size-4" aria-hidden="true" />
									{/if}
								</button>
							{/snippet}
						</Tooltip.Trigger>
						<Tooltip.Portal>
							<Tooltip.Content
								side="bottom"
								sideOffset={6}
								escapeKeydownBehavior="defer-otherwise-close"
								class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
								>{copied ? 'Copied' : `Copy ${refLabel}`}</Tooltip.Content
							>
						</Tooltip.Portal>
					</Tooltip.Root>
					<Tooltip.Root ignoreNonKeyboardFocus>
						<Tooltip.Trigger>
							{#snippet child({ props })}
								<button
									{...props}
									type="button"
									class="work-detail-btn"
									aria-label="Close work details"
									onclick={onclose}
								>
									<X class="size-4" aria-hidden="true" />
								</button>
							{/snippet}
						</Tooltip.Trigger>
						<Tooltip.Portal>
							<Tooltip.Content
								side="bottom"
								sideOffset={6}
								escapeKeydownBehavior="defer-otherwise-close"
								class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
								>Close</Tooltip.Content
							>
						</Tooltip.Portal>
					</Tooltip.Root>
				</div>
			</Tooltip.Provider>
		</div>

		<p class="sr-only" role="status">{copied ? `${refLabel} copied.` : ''}</p>

		<div class="work-detail-body">
			{@render children()}
		</div>
	</article>
</aside>

<style>
	.work-reading {
		min-width: 0;
		width: 100%;
		max-width: 100%;
		background: var(--exec-surface);
		color: var(--exec-ink);
		container: work-reading / inline-size;
		overflow-x: clip;
		overflow-y: visible;
	}
	.work-document {
		width: 100%;
		max-width: none;
		min-width: 0;
		margin-inline: 0;
		padding: clamp(12px, 2cqi, 24px) var(--work-gutter, 24px) 36px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.work-detail-bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px 12px;
		min-width: 0;
	}
	.work-back-btn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-height: 36px;
		padding: 0 8px 0 0;
		border-radius: var(--exec-ctrl-radius);
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.work-back-btn:hover {
		color: var(--exec-ink);
		background: var(--exec-btn-ghost-bg-hover);
	}
	.work-detail-id {
		display: inline-flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		min-width: 0;
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-variant-numeric: tabular-nums;
	}
	.work-detail-ref {
		font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
		font-weight: 600;
		color: var(--exec-ink);
		overflow-wrap: anywhere;
	}
	.work-detail-urgent {
		color: var(--tone-bad-ink);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.work-kind {
		padding-inline-start: 8px;
		border-inline-start: 1px solid var(--exec-border-hi);
		color: var(--exec-muted);
		font-weight: 500;
	}
	.work-detail-actions {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		flex-shrink: 0;
		margin-inline-start: auto;
	}
	@container work-reading (max-width: 22rem) {
		.work-detail-id {
			order: 1;
			flex-basis: 100%;
		}
	}
	.work-detail-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 36px;
		min-height: 36px;
		border-radius: var(--exec-ctrl-radius);
		background: transparent;
		color: var(--exec-body);
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.work-detail-btn:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.work-detail-btn:focus-visible,
	.work-back-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.work-detail-body {
		min-width: 0;
		max-width: 100%;
	}
	/* WCAG 1.4.11 >= 3:1 non-text boundaries for inline editor inputs, selects, and textareas */
	.work-reading :global(input:not([type='checkbox']):not([type='radio'])),
	.work-reading :global(select),
	.work-reading :global(textarea) {
		border-color: var(--field-border);
		outline-color: var(--field-border);
	}
	.work-reading :global(input:not([type='checkbox']):not([type='radio']):focus-visible),
	.work-reading :global(select:focus-visible),
	.work-reading :global(textarea:focus-visible) {
		border-color: var(--focus);
		outline-color: var(--focus);
	}
	.work-priority-mark {
		position: relative;
		display: inline-flex;
		flex: none;
		align-items: flex-end;
		justify-content: center;
		gap: 2px;
		width: 1rem;
		height: 1rem;
		padding-bottom: 3px;
		color: var(--exec-muted);
	}
	.work-priority-mark[data-tone='strong'] {
		color: var(--exec-ink);
	}
	.work-priority-mark span {
		width: 0.15rem;
		height: 0.28rem;
		border-radius: 1px;
		background: currentColor;
	}
	.work-priority-mark span:nth-child(2) {
		height: 0.44rem;
	}
	.work-priority-mark span:nth-child(3) {
		height: 0.6rem;
	}
	.work-priority-mark[data-level='1'] span:nth-child(n + 2),
	.work-priority-mark[data-level='2'] span:nth-child(3) {
		opacity: 0.32;
	}
	.work-priority-mark[data-level='0'] {
		color: var(--fg-muted);
	}
	.work-priority-mark[data-level='0'] span {
		width: 3px;
		border: 1px solid currentColor;
		background: transparent;
	}
	.work-reading-status {
		position: relative;
		display: inline-flex;
		flex: none;
		align-items: center;
		justify-content: center;
		width: 1rem;
		height: 1rem;
		color: var(--exec-muted);
	}
	.work-reading-status[data-tone='active'] {
		color: var(--tone-warn-ink);
	}
	.work-reading-status[data-tone='done'] {
		color: var(--tone-ok-ink);
	}
	.work-reading-status[data-tone='blocked'] {
		color: var(--tone-bad-ink);
	}
	.work-reading-status[data-tone='review'] {
		color: var(--tone-accent-ink);
	}
	.work-reading-status[data-attention]::after {
		content: '';
		position: absolute;
		inset: 0 0 auto auto;
		width: 0.35rem;
		height: 0.35rem;
		border-radius: 50%;
		background: var(--tone-warn-ink);
		box-shadow: 0 0 0 1.5px var(--exec-surface);
	}
	.work-reading-status-ring {
		display: inline-grid;
		place-items: center;
		width: 0.875rem;
		height: 0.875rem;
		border: 1.5px solid currentColor;
		border-radius: 50%;
	}
	.work-reading-status[data-tone='active'] .work-reading-status-ring::before {
		content: '';
		height: 75%;
		aspect-ratio: 1 / 2;
		border-radius: 0 100% 100% 0;
		border-inline-end: 0.24rem solid currentColor;
		transform: translateX(50%);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.work-detail-btn {
			min-width: 44px;
			min-height: 44px;
		}
		.work-back-btn {
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: no-preference) {
		.work-reading.is-opening {
			animation: work-reveal 280ms cubic-bezier(0.25, 0.7, 0.25, 1);
		}
		@keyframes work-reveal {
			from {
				clip-path: inset(0 100% 0 0);
			}
			to {
				clip-path: inset(0);
			}
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.work-detail-btn,
		.work-back-btn {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.work-detail-btn,
		.work-back-btn {
			border: 1px solid ButtonText;
		}
		.work-reading :global(:is(input, textarea, select):focus-visible) {
			outline: 2px solid Highlight;
			outline-offset: 1px;
		}
		.work-priority-mark span,
		.work-reading-status[data-attention]::after {
			forced-color-adjust: none;
			background: CanvasText;
		}
		.work-priority-mark[data-tone='strong'] span {
			background: Highlight;
		}
		.work-detail-urgent {
			color: Highlight;
		}
		.work-priority-mark[data-level='0'] span {
			background: transparent;
			border-color: CanvasText;
		}
		.work-reading-status[data-attention]::after {
			box-shadow: 0 0 0 1px Canvas;
		}
	}
</style>
