<!--
  @component
  Zero-Reflow Bottom-Right Floating Tess Companion (TASK-145 / TASK-80.5 / doc-97).
  Rests as a compact floating pill ([✦ Tess ⌘/]) and lazy-loads `TessDrawerSurface.svelte`
  on first open to keep shell first-paint JS lightweight.
-->
<script lang="ts">
	import { tick, type Component } from 'svelte';
	import { page } from '$app/state';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import { useNotifications } from '$lib/platform/workspace/notifications.svelte';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';
	import type { TessPanelState } from './tess-panel.svelte';

	interface Props {
		panel: TessPanelState;
		organizationId?: string;
		snapshot?: OrgSnapshot;
		items?: readonly WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: readonly OfficeInfo[];
		ownerId?: string;
		offline?: boolean;
		onCloseRestoreFocus?: () => void;
	}

	let {
		panel,
		organizationId,
		snapshot,
		items = [],
		inboxItems = [],
		offices = [],
		ownerId = 'owner',
		offline = false,
		onCloseRestoreFocus
	}: Props = $props();

	const notifications = useNotifications();
	const activeTessJobs = $derived(notifications?.activeTessJobs ?? []);
	const isMac = $derived(
		typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/i.test(navigator.platform || '')
	);
	const toggleShortcut = $derived(isMac ? '⌘/' : 'Ctrl /');

	const isTessChatActive = $derived(
		page.url.pathname === '/chat' &&
			(page.url.searchParams.get('c')?.toLowerCase().includes('tess') ?? false)
	);

	const effectiveMode = $derived(
		panel.mode === 'hidden' && uiPrefs.showTessPill
			? 'pill'
			: panel.mode === 'pill' && !uiPrefs.showTessPill
				? 'hidden'
				: panel.mode
	);

	const visible = $derived(!isTessChatActive && effectiveMode !== 'hidden');
	const isPill = $derived(visible && effectiveMode === 'pill');
	const isOpen = $derived(visible && effectiveMode === 'open');
	const isExpanded = $derived(visible && effectiveMode === 'expanded');
	const isCompact = $derived(isOpen && panel.turns.length === 0);

	type DrawerSurfaceComponent = Component<{
		panel: TessPanelState;
		organizationId?: string;
		snapshot?: OrgSnapshot;
		items?: readonly WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: readonly OfficeInfo[];
		ownerId?: string;
		offline?: boolean;
		isExpanded: boolean;
		onClose: (restoreFocus?: boolean) => void;
	}>;

	let DrawerSurface = $state.raw<DrawerSurfaceComponent | undefined>(undefined);
	let drawerLoading = false;
	let drawerLoadFailed = $state(false);
	let pillBtnEl = $state<HTMLButtonElement | null>(null);

	async function ensureDrawerLoaded() {
		if (DrawerSurface || drawerLoading) return;
		drawerLoading = true;
		drawerLoadFailed = false;
		try {
			const mod = await import('./TessDrawerSurface.svelte');
			DrawerSurface = mod.default;
		} catch {
			drawerLoadFailed = true;
		} finally {
			drawerLoading = false;
		}
	}

	$effect(() => {
		if (isOpen || isExpanded) void ensureDrawerLoaded();
	});

	$effect(() => {
		if (typeof document === 'undefined') return;
		document.body.classList.toggle('companion-open', isOpen);
		document.body.classList.toggle('companion-minimized', isPill);
		return () => {
			document.body.classList.remove('companion-open', 'companion-minimized');
		};
	});

	function closeCompanion(restoreFocus = true) {
		panel.close(uiPrefs.showTessPill);
		if (!restoreFocus) return;
		void tick().then(() => {
			if (uiPrefs.showTessPill && pillBtnEl) pillBtnEl.focus({ preventScroll: true });
			else onCloseRestoreFocus?.();
		});
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (!isOpen && !isExpanded) return;
		if (event.key !== 'Escape') return;
		if (document.querySelector('[role="alertdialog"], [role="menu"], [role="tooltip"]')) return;
		if (!isExpanded && document.querySelector('[role="dialog"]:not([data-testid="tess-panel"])')) {
			return;
		}
		event.preventDefault();
		if (isExpanded) {
			panel.toggleExpand();
		} else {
			closeCompanion(true);
		}
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if visible}
	{#if isExpanded}
		<button
			type="button"
			aria-label="Restore Tess companion size"
			tabindex={-1}
			onclick={() => panel.toggleExpand()}
			class="tess-panel-scrim"
		></button>
	{/if}

	<aside
		role={isExpanded ? 'dialog' : 'complementary'}
		aria-modal={isExpanded ? 'true' : undefined}
		aria-label="Tess Chief of Staff"
		data-testid="tess-panel"
		class={[
			'tess-panel',
			isPill && 'is-minimized',
			isCompact && 'is-compact',
			isExpanded && 'is-expanded'
		]}
	>
		{#if isPill}
			<button
				bind:this={pillBtnEl}
				type="button"
				data-testid="tess-dock-pill"
				onclick={() => panel.openPanel()}
				aria-label={activeTessJobs.length > 0
					? `Tess refining (${activeTessJobs.length}) · ${toggleShortcut}`
					: `Ask Tess · ${toggleShortcut}`}
				aria-keyshortcuts="Meta+/ Control+/"
				title={activeTessJobs.length > 0
					? `Tess refining (${activeTessJobs.length}) · ${toggleShortcut}`
					: `Ask Tess · ${toggleShortcut}`}
				class="cmp-dock-btn"
			>
				{#if offline}
					<WifiOff class="cmp-dock-ico size-3.5 shrink-0" aria-hidden="true" />
				{:else}
					<Sparkles class="cmp-dock-ico size-3.5 shrink-0" aria-hidden="true" />
				{/if}
				<strong>Tess</strong>
				{#if activeTessJobs.length > 0}
					<span class="cmp-pill-st">Refining ({activeTessJobs.length})</span>
					<span class="tess-live-dot" aria-hidden="true"></span>
				{:else if offline}
					<span class="cmp-pill-st">Offline</span>
				{:else}
					<kbd class="cmp-pill-kbd" aria-hidden="true">{toggleShortcut}</kbd>
				{/if}
			</button>
		{:else if DrawerSurface}
			<DrawerSurface
				{panel}
				{organizationId}
				{snapshot}
				{items}
				{inboxItems}
				{offices}
				{ownerId}
				{offline}
				{isExpanded}
				onClose={closeCompanion}
			/>
		{:else if drawerLoadFailed}
			<div class="p-4 text-xs text-[var(--exec-muted)]">
				Could not load Tess drawer.
				<button type="button" class="ml-2 underline" onclick={() => void ensureDrawerLoaded()}>
					Retry
				</button>
			</div>
		{:else}
			<div class="p-4 text-xs text-[var(--exec-muted)]" role="status">Loading Tess…</div>
		{/if}
	</aside>
{/if}

<style>
	.tess-panel {
		position: fixed;
		right: 20px;
		bottom: 20px;
		z-index: 84;
		width: 420px;
		max-width: calc(100vw - 32px);
		max-height: min(560px, calc(100dvh - 108px));
		display: flex;
		flex-direction: column;
		overflow: hidden;
		border: 1px solid var(--exec-tess-border, var(--exec-border-hi, var(--line-strong)));
		border-radius: var(--exec-tess-radius, 16px);
		background: var(--exec-tess-bg, var(--exec-cmdk-menu-bg, var(--surface)));
		-webkit-backdrop-filter: var(--exec-tess-blur, var(--exec-cmdk-menu-blur, none));
		backdrop-filter: var(--exec-tess-blur, var(--exec-cmdk-menu-blur, none));
		color: var(--exec-ink, var(--fg));
		box-shadow: var(--exec-tess-shadow, var(--exec-cmdk-menu-shadow, none));
		transform-origin: bottom right;
		will-change: transform, opacity;
		animation: cmp-open 0.18s cubic-bezier(0.16, 1, 0.3, 1) both;
	}
	.tess-panel.is-minimized {
		width: auto;
		max-width: min(260px, calc(100vw - 32px));
		height: 42px;
		border-radius: 999px;
		overflow: visible;
		background: var(
			--exec-tess-pill-bg,
			var(--exec-tess-bg, var(--exec-cmdk-menu-bg, var(--surface)))
		);
		border-color: var(--exec-tess-pill-border, var(--exec-tess-border, var(--exec-border-hi)));
		box-shadow: var(--exec-tess-pill-shadow, var(--exec-cmdk-menu-shadow, none));
		animation: cmp-pill 0.16s cubic-bezier(0.16, 1, 0.3, 1) both;
	}
	:global(body:has(.activity-tools)) .tess-panel:not(.is-expanded) {
		right: 68px;
	}
	@keyframes cmp-open {
		from {
			opacity: 0;
			transform: translate3d(0, 14px, 0) scale(0.94);
		}
		to {
			opacity: 1;
			transform: translate3d(0, 0, 0) scale(1);
		}
	}
	@keyframes cmp-pill {
		from {
			opacity: 0.65;
			transform: translate3d(0, -8px, 0) scale(1.06);
		}
		to {
			opacity: 1;
			transform: translate3d(0, 0, 0) scale(1);
		}
	}
	.cmp-dock-btn {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		height: 40px;
		padding: 0 13px;
		border-radius: 999px;
		color: var(--exec-ink, var(--fg));
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition: background-color 0.14s ease;
	}
	.cmp-dock-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
	}
	.cmp-dock-btn :global(.cmp-dock-ico) {
		color: var(--tone-accent-ink, var(--link));
	}
	.cmp-pill-st {
		font-size: 0.75rem;
		color: var(--exec-muted, var(--fg-muted));
		font-weight: 500;
	}
	.cmp-pill-kbd {
		padding: 1px 6px;
		border-radius: 5px;
		background: var(--exec-well, var(--surface-raised));
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		color: var(--exec-muted, var(--fg-muted));
		font-family: inherit;
		font-size: 0.75rem;
		font-weight: 600;
	}
	.tess-live-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--tone-accent, var(--brand));
		box-shadow: 0 0 0 2px var(--tone-accent-bg);
		animation: tess-pulse 1.1s ease-in-out infinite;
	}
	@keyframes tess-pulse {
		0%,
		100% {
			opacity: 0.4;
			transform: scale(0.85);
		}
		50% {
			opacity: 1;
			transform: scale(1.15);
		}
	}
	@media (max-width: 639px) {
		.tess-panel:not(.is-minimized) {
			inset: auto 0 0 0;
			width: 100vw;
			max-width: 100vw;
			height: auto;
			max-height: 82dvh;
			border-radius: 16px 16px 0 0;
			border-bottom: 0;
			transform: none;
		}
	}
	.tess-panel-scrim {
		position: fixed;
		inset: 0;
		z-index: 82;
		background: var(--exec-scrim-bg, color-mix(in srgb, var(--surface-sunken) 72%, transparent));
		animation: cmp-scrim-in 0.15s ease-out both;
	}
	.tess-panel.is-compact {
		max-height: min(320px, calc(100dvh - 108px));
	}
	.tess-panel.is-expanded {
		top: 50%;
		left: 50%;
		right: auto;
		bottom: auto;
		z-index: 86;
		width: min(680px, calc(100vw - 48px));
		height: min(74dvh, 660px);
		max-height: calc(100dvh - 48px);
		transform: translate(-50%, -50%);
		transform-origin: center;
		animation: cmp-expand 0.2s cubic-bezier(0.16, 1, 0.3, 1) both;
	}
	@keyframes cmp-scrim-in {
		from {
			opacity: 0;
		}
		to {
			opacity: 1;
		}
	}
	@keyframes cmp-expand {
		from {
			opacity: 0;
			transform: translate3d(-50%, -47%, 0) scale(0.95);
		}
		to {
			opacity: 1;
			transform: translate3d(-50%, -50%, 0) scale(1);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.tess-panel,
		.tess-panel-scrim,
		.tess-live-dot {
			animation: none;
		}
	}
</style>
