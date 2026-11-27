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
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import Minimize2 from '@lucide/svelte/icons/minimize-2';
	import PauseCircle from '@lucide/svelte/icons/pause-circle';
	import X from '@lucide/svelte/icons/x';
	import { toast } from 'svelte-sonner';
	import type { WorkStatusRingTone } from './pills';

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
	let backBtnEl = $state<HTMLButtonElement | null>(null);
	let isOpening = $state(true);
	let expanded = $state(false);
	let availableWidth = $state(1200);
	let defaultPanelPx = $state(736);
	let copiedForId = $state<string | null>(null);
	const copied = $derived(copiedForId === itemId);
	const canExpand = $derived(availableWidth > defaultPanelPx + 16);
	let copyTimer: ReturnType<typeof setTimeout> | undefined;

	onDestroy(() => {
		clearTimeout(copyTimer);
	});

	$effect(() => {
		if (typeof window === 'undefined') return;
		const measure = () => {
			const rootPx = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
			defaultPanelPx = Math.round(46 * rootPx);
			const sidebar = document.querySelector<HTMLElement>('aside.exec-sidebar');
			const sidebarWidth =
				sidebar && sidebar.offsetParent !== null ? sidebar.getBoundingClientRect().width : 0;
			availableWidth = Math.max(320, Math.round(window.innerWidth - sidebarWidth));
		};
		measure();
		window.addEventListener('resize', measure);
		const sidebar = document.querySelector<HTMLElement>('aside.exec-sidebar');
		const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(measure) : undefined;
		if (sidebar && ro) ro.observe(sidebar);
		// Make every background sibling tree up to document.body inert while this modal reading dialog is open
		// so focus and pointer cannot escape to shell chrome, team/project tabs, or list rows behind the dialog,
		// while preserving nested Bits UI portals/dialogs and Sonner's stable polite live-region container
		// (even when 0 toasts are currently mounted) so later save/close toasts remain announced to screen readers.
		const inertTargets: HTMLElement[] = [];
		const isSonnerLiveRegion = (el: HTMLElement) =>
			el.tagName === 'SECTION' &&
			el.getAttribute('aria-live') === 'polite' &&
			el.getAttribute('aria-relevant') === 'additions text';
		const shouldSkipSibling = (el: HTMLElement) =>
			el.classList.contains('work-panel-backdrop') ||
			el.hasAttribute('data-bits-portal') ||
			el.hasAttribute('data-sonner-toaster') ||
			isSonnerLiveRegion(el) ||
			Boolean(el.querySelector('[data-sonner-toaster]')) ||
			el.getAttribute('role') === 'dialog' ||
			el.getAttribute('role') === 'alertdialog';
		let cur: HTMLElement | null = panelEl;
		while (cur && cur !== document.body && cur.parentElement) {
			for (const sibling of cur.parentElement.children) {
				if (
					sibling instanceof HTMLElement &&
					sibling !== cur &&
					!inertTargets.includes(sibling) &&
					!shouldSkipSibling(sibling)
				) {
					inertTargets.push(sibling);
				}
			}
			cur = cur.parentElement;
		}
		const prevInert = inertTargets.map((el) => el.inert);
		for (const el of inertTargets) el.inert = true;
		// Stop Sonner's document-level Alt+T hotkey at document.body (bubble phase) WITHOUT preventDefault(),
		// so macOS Option+T (`†`) text entry still works inside inputs/textareas while focus never jumps to toasts.
		const blockToastFocusHotkey = (event: KeyboardEvent) => {
			if (event.altKey && !event.ctrlKey && !event.metaKey && event.code === 'KeyT') {
				event.stopPropagation();
			}
		};
		const guardModalFocus = (event: FocusEvent) => {
			const target = event.target;
			if (!(target instanceof Node) || !panelEl || panelEl.contains(target)) return;
			if (
				target instanceof Element &&
				target.closest(
					'[role="dialog"]:not(.work-reading), [role="alertdialog"], [role="menu"], [data-bits-portal]'
				)
			) {
				return;
			}
			const fallback = focusableInside(panelEl)[0] ?? backBtnEl ?? panelEl;
			fallback?.focus({ preventScroll: true });
		};
		document.body.addEventListener('keydown', blockToastFocusHotkey);
		document.addEventListener('focusin', guardModalFocus, true);
		return () => {
			window.removeEventListener('resize', measure);
			document.body.removeEventListener('keydown', blockToastFocusHotkey);
			document.removeEventListener('focusin', guardModalFocus, true);
			ro?.disconnect();
			inertTargets.forEach((el, i) => {
				el.inert = prevInert[i];
			});
		};
	});

	$effect(() => {
		if (!canExpand && expanded) {
			expanded = false;
		}
	});

	function focusableInside(container: HTMLElement): HTMLElement[] {
		const selector =
			'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
		return [...container.querySelectorAll<HTMLElement>(selector)].filter(
			(el) => el.offsetParent !== null && !el.closest('[inert]')
		);
	}

	function handlePanelKeydown(event: KeyboardEvent) {
		if (event.key !== 'Tab' || event.altKey || event.ctrlKey || event.metaKey || !panelEl) return;
		// Yield when a nested modal dialog/menu is open above the panel.
		if (
			document.querySelector(
				'[role="dialog"]:not(.work-reading), [role="alertdialog"], [role="menu"]'
			)
		) {
			return;
		}
		const items = focusableInside(panelEl);
		if (items.length === 0) return;
		const first = items[0];
		const last = items[items.length - 1];
		const active = document.activeElement;
		if (event.shiftKey) {
			if (active === first || !panelEl.contains(active)) {
				event.preventDefault();
				last.focus();
			}
		} else if (active === last || !panelEl.contains(active)) {
			event.preventDefault();
			first.focus();
		}
	}

	$effect(() => {
		void itemId;
		let cancelled = false;
		void tick().then(() => {
			if (cancelled || !panelEl) return;
			panelEl.scrollTop = 0;
			if (!panelEl.contains(document.activeElement)) {
				const target = panelEl.querySelector<HTMLElement>('#work-detail-title') ?? backBtnEl;
				target?.focus({ preventScroll: true });
				target?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
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

<!-- Clickable scrim behind the sliding right-hand Work reading panel (Prototype Release 3c4e3541) -->
<div class="work-panel-backdrop" aria-hidden="true" onclick={onclose}></div>

<!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
<aside
	bind:this={panelEl}
	class={['work-reading side-panel work-panel', isOpening && 'is-opening']}
	role="dialog"
	aria-modal="true"
	aria-label={ariaLabel}
	data-testid="work-reading-panel"
	data-expanded={expanded && canExpand ? '' : undefined}
	style:--work-page-width={`${availableWidth}px`}
	onkeydown={handlePanelKeydown}
	onanimationend={(event) => {
		if (event.target === event.currentTarget) isOpening = false;
	}}
>
	<header class="side-panel-header work-detail-bar">
		<div class="flex min-w-0 flex-wrap items-center gap-2">
			<button bind:this={backBtnEl} type="button" class="work-back-btn" onclick={onclose}>
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
		</div>

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
				{#if canExpand}
					<Tooltip.Root ignoreNonKeyboardFocus>
						<Tooltip.Trigger>
							{#snippet child({ props })}
								<button
									{...props}
									type="button"
									class="work-detail-btn work-panel-expand"
									aria-label={expanded ? 'Restore panel width' : 'Expand panel to full width'}
									aria-pressed={expanded}
									onclick={() => (expanded = !expanded)}
								>
									{#if expanded}
										<Minimize2 class="size-4" aria-hidden="true" />
									{:else}
										<Maximize2 class="size-4" aria-hidden="true" />
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
								>{expanded ? 'Restore panel width' : 'Expand panel'}</Tooltip.Content
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
	</header>

	<article class="work-document">
		<p class="sr-only" role="status">{copied ? `${refLabel} copied.` : ''}</p>

		<div class="work-detail-body">
			{@render children()}
		</div>
	</article>
</aside>

<style>
	.work-panel-backdrop {
		position: fixed;
		inset: 0;
		z-index: 44;
		background: var(--side-panel-scrim);
	}
	.side-panel.work-panel {
		--side-panel-width: 46rem;
		--work-page-width: 100%;
		position: fixed;
		inset: 0 0 0 auto;
		z-index: 45;
		width: min(var(--work-page-width), var(--side-panel-width));
		max-width: 100%;
		height: 100%;
		overflow-x: clip;
		overflow-y: auto;
		overscroll-behavior: contain;
		background: var(--side-panel-surface, var(--exec-surface));
		color: var(--exec-ink);
		border-inline-start: 1px solid var(--line);
		box-shadow: var(--side-panel-shadow);
		container: work-reading / inline-size;
	}
	@media (min-width: 640px) {
		.side-panel.work-panel {
			--work-page-width: calc(100% - 68px);
		}
	}
	@media (min-width: 960px) {
		.side-panel.work-panel {
			--work-page-width: calc(100% - 264px);
		}
	}
	.side-panel.work-panel[data-expanded] {
		width: var(--work-page-width);
	}
	@media (max-width: 780px) {
		.work-panel-expand {
			display: none;
		}
	}
	.side-panel-header {
		position: sticky;
		inset-block-start: 0;
		z-index: 2;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 12px;
		padding: 8px var(--work-gutter, 24px);
		border-block-end: 1px solid var(--line);
		background: var(--side-panel-surface, var(--exec-surface));
	}
	.work-document {
		width: 100%;
		max-width: none;
		min-width: 0;
		margin-inline: 0;
		padding: clamp(16px, 3cqi, 28px) var(--work-gutter, 24px) 36px;
		display: flex;
		flex-direction: column;
		gap: 12px;
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
		box-shadow: 0 0 0 1.5px var(--side-panel-surface, var(--exec-surface));
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
		.side-panel.work-panel {
			transition: width 260ms cubic-bezier(0.2, 0.7, 0.25, 1);
		}
		.side-panel.work-panel.is-opening {
			animation: side-panel-enter 260ms cubic-bezier(0.2, 0.7, 0.25, 1);
		}
		@keyframes side-panel-enter {
			from {
				transform: translateX(100%);
			}
			to {
				transform: translateX(0);
			}
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.work-detail-btn,
		.work-back-btn,
		.side-panel.work-panel {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.side-panel.work-panel {
			border-inline-start: 2px solid CanvasText;
		}
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
