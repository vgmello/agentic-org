<script lang="ts">
	import ExecutiveCommandBar from '$lib/platform/shell/ExecutiveCommandBar.svelte';
	import ExecutiveSidebar from '$lib/platform/shell/ExecutiveSidebar.svelte';
	import MobileNavigation from '$lib/platform/shell/MobileNavigation.svelte';
	import ShortcutsDialog from '$lib/platform/shell/ShortcutsDialog.svelte';
	import { provideNotifications } from '$lib/platform/shell/notifications.svelte';
	import HomeScrollHint from '$lib/modules/home/features/HomeScrollHint.svelte';
	import TessPanel from '$lib/modules/tess/TessPanel.svelte';
	import { provideTessPanel } from '$lib/modules/tess/tess-panel.svelte';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import LiveBanner from '$lib/components/LiveBanner.svelte';
	import { Toaster } from '$lib/components/ui/sonner';

	import { onDestroy, onMount, untrack } from 'svelte';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { selectDesktopSection } from '$lib/platform/shell/desktop-navigation';
	import { navigating, page } from '$app/state';
	import { inboxApi } from '$lib/modules/inbox/api';
	import { orgApi } from '$lib/modules/organization/api';
	import { noteChanged } from '$lib/modules/organization/live';
	import { liveState } from '$lib/modules/organization/live-state.svelte';
	import { areaFor } from '$lib/nav';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { focusDestinationHeading } from '$lib/platform/shell/focus-destination';
	import { noteModalBackNavigation } from '$lib/platform/shell/modal-back';
	import { NavKeyController } from '$lib/platform/shell/nav-keys.svelte';
	import {
		createScopedSearchHost,
		setScopedSearchContext
	} from '$lib/platform/shell/scoped-search.svelte';
	import { provideShellAi, type ShellAiAskOptions } from '$lib/platform/shell/shell-ai';
	import { provideShellCommand } from '$lib/platform/shell/shell-command';
	import { provideShellData } from '$lib/platform/shell/shell-data.svelte';
	import { provideDraftStore } from '$lib/modules/messaging/drafts/context';
	import { officesApi } from '$lib/modules/offices/api';
	import { layerOffices } from '$lib/modules/offices/layer';

	let { data, children } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const isHome = $derived(page.url.pathname === '/home');
	const chatDrafts = provideDraftStore(() => organizationId);
	$effect.pre(() => chatDrafts.syncScope());
	onDestroy(() => chatDrafts.dispose());

	const tessPanel = provideTessPanel();
	provideNotifications();
	let shortcutsOpen = $state(false);
	let commandBar = $state<ReturnType<typeof ExecutiveCommandBar> | undefined>();

	provideShellCommand({
		openPalette(prefill?: string) {
			commandBar?.openPalette(prefill);
		},
		toggleTess() {
			tessPanel.toggle(uiPrefs.showTessPill);
		},
		toggleTessExpand() {
			tessPanel.toggleExpand();
		},
		async openTessChat() {
			await commandBar?.openTessChat();
		},
		openShortcuts() {
			shortcutsOpen = true;
		},
		openNewTask() {
			commandBar?.openNewTask();
		}
	});

	function askTess(prompt: string, opts?: ShellAiAskOptions) {
		tessPanel.ask(
			prompt,
			{
				snapshot: shellData.snapshot,
				items: shellData.workItems,
				offices: shellData.offices,
				ownerId: shellData.ownerId,
				domain: { inboxItems: shellData.inboxItems },
				routeLabel: areaFor(page.url.pathname) ?? 'Home'
			},
			opts
		);
	}

	provideShellAi({
		ask(prompt: string, opts?: ShellAiAskOptions) {
			askTess(prompt, opts);
		}
	});
	setScopedSearchContext(createScopedSearchHost());

	$effect(() => {
		document.documentElement.toggleAttribute('data-interior-page', !isHome);
		return () => {
			document.documentElement.removeAttribute('data-interior-page');
		};
	});

	// Route changes reset internal <main> scroll (except hash targets) and focus the destination heading (app-shell-ux §0).
	afterNavigate((move) => {
		noteModalBackNavigation();
		const routeChanged = move.from?.url?.pathname !== move.to?.url?.pathname;
		if (routeChanged) {
			navKeys.cancel(false);
			mobileMenuOpen = false;
			tessPanel.noteRouteChange();
		}
		if (routeChanged && !move.to?.url?.hash && mainEl) {
			mainEl.scrollTop = 0;
		}
		if (move.type !== 'link' && move.type !== 'popstate') return;
		if (!routeChanged) return;
		const focused = document.activeElement;
		if (focused && focused !== document.body && document.contains(focused)) return;
		void focusDestinationHeading();
	});
	onMount(() => void appSession.boot());

	let desktopNavigation = $state<HTMLDivElement | null>(null);
	let mainEl = $state<HTMLElement | null>(null);
	let mobileMenuOpen = $state(false);
	let sidebarHovered = $state(false);
	let sidebarFocused = $state(false);
	let sidebarMenuOpen = $state(false);
	let sidebarHoverTimer: ReturnType<typeof setTimeout> | undefined;
	let sidebarFocusTimer: ReturnType<typeof setTimeout> | undefined;
	const navKeys = new NavKeyController({
		onSidebarOpen() {
			if (typeof window !== 'undefined' && window.innerWidth < 640) {
				mobileMenuOpen = true;
			} else if (uiPrefs.navCollapsed) {
				clearTimeout(sidebarFocusTimer);
				sidebarFocused = true;
			}
		},
		onSidebarCancel() {
			if (typeof window !== 'undefined' && window.innerWidth < 640 && mobileMenuOpen) {
				mobileMenuOpen = false;
			}
		}
	});
	const sidebarPinned = $derived(!uiPrefs.navCollapsed);
	const sidebarCollapsed = $derived(
		uiPrefs.navCollapsed &&
			!sidebarHovered &&
			!sidebarFocused &&
			!sidebarMenuOpen &&
			navKeys.mode !== 'sidebar'
	);

	function handleSidebarMenuOpenChange(open: boolean) {
		clearTimeout(sidebarFocusTimer);
		sidebarMenuOpen = open;
		if (!open) {
			// Recompute focus containment after Bits UI focus restoration or outside click settles
			// without cancelling any pending pointer-leave collapse timer.
			const syncFocus = () => {
				if (sidebarMenuOpen) return;
				const active = document.activeElement;
				sidebarFocused = Boolean(active instanceof Node && desktopNavigation?.contains(active));
			};
			queueMicrotask(syncFocus);
			requestAnimationFrame(syncFocus);
		}
	}

	function handleSidebarEnter() {
		clearTimeout(sidebarHoverTimer);
		sidebarHovered = true;
	}
	function handleSidebarLeave() {
		clearTimeout(sidebarHoverTimer);
		sidebarHoverTimer = setTimeout(() => {
			sidebarHovered = false;
		}, 240);
	}
	function handleSidebarFocusIn(event: FocusEvent) {
		const target = event.target;
		if (target instanceof HTMLElement && target.matches(':focus-visible')) {
			clearTimeout(sidebarFocusTimer);
			sidebarFocused = true;
		}
	}
	function handleSidebarFocusOut(event: FocusEvent) {
		if (sidebarMenuOpen) return;
		const next = event.relatedTarget;
		if (next instanceof Node && desktopNavigation?.contains(next)) return;
		clearTimeout(sidebarFocusTimer);
		sidebarFocusTimer = setTimeout(() => {
			if (!sidebarMenuOpen) sidebarFocused = false;
		}, 160);
	}
	function toggleSidebarPin() {
		clearTimeout(sidebarHoverTimer);
		clearTimeout(sidebarFocusTimer);
		uiPrefs.toggleNavCollapsed();
		if (uiPrefs.navCollapsed) {
			sidebarHovered = false;
			sidebarFocused = false;
		}
	}
	onDestroy(() => {
		clearTimeout(sidebarHoverTimer);
		clearTimeout(sidebarFocusTimer);
	});

	const shellData = provideShellData({
		getOrganizationId: () => organizationId,
		getBaseOffices: () => data.connection.organization.offices,
		loadOffices: (id, baseOffices) =>
			officesApi
				.overview(id)
				.then((overview) => layerOffices(baseOffices, overview))
				.catch(() => baseOffices)
	});

	let leaving = false;

	$effect(() => {
		const id = organizationId;
		const reset = untrack(() => shellData.loadedOrganizationId !== id);
		const { cancel } = shellData.load(id, reset);
		return cancel;
	});
	onDestroy(() => shellData.dispose());

	function focusDesktopNavigation() {
		desktopNavigation?.querySelector<HTMLElement>('a[aria-current="page"]')?.focus();
	}

	async function reload() {
		if (navigating.to) {
			const from = page.url.href;
			while (navigating.to) await new Promise((resolve) => setTimeout(resolve, 50));
			if (page.url.href !== from) return;
		}
		await invalidate('app:org');
		void shellData.refresh();
	}

	let cannotListen = $state(false);
	let retrying = $state(false);
	let listening = $state(false);
	const liveNotices: Record<string, number> = {};
	let retryTimer: ReturnType<typeof setTimeout> | undefined;
	const showPaused = $derived(liveState.paused[organizationId] === true || cannotListen);
	$effect(() => {
		if (!showPaused) retrying = false;
	});

	let stop: (() => void) | undefined;
	function listen() {
		orgApi
			.onFeed((notice) => {
				if (notice.kind === 'live') {
					liveNotices[notice.organization_id] = (liveNotices[notice.organization_id] ?? 0) + 1;
					liveState.paused[notice.organization_id] = !notice.live;
				} else if (notice.organization_id === organizationId) noteChanged(notice, reload);
			})
			.then((unlisten) => {
				if (leaving) unlisten();
				else {
					stop = unlisten;
					cannotListen = false;
					listening = true;
				}
			})
			.catch(() => {
				cannotListen = true;
			});
	}
	$effect(() => {
		const id = organizationId;
		if (!listening) return;
		const noticesBefore = liveNotices[id] ?? 0;
		void orgApi
			.feedLive(id)
			.then((live) => {
				if (live !== undefined && (liveNotices[id] ?? 0) === noticesBefore)
					liveState.paused[id] = !live;
			})
			.catch(() => undefined);
	});
	function retry() {
		retrying = true;
		clearTimeout(retryTimer);
		retryTimer = setTimeout(() => (retrying = false), 3000);
		if (cannotListen) listen();
		void orgApi.retryFeed(organizationId).catch(() => undefined);
	}
	$effect(() => {
		const id = organizationId;
		let cancelled = false;
		let stopInbox: (() => void) | undefined;
		void inboxApi
			.subscribe(id, () => {
				if (!cancelled && !leaving) void shellData.refresh();
			})
			.then((unsub) => {
				if (cancelled || leaving) unsub();
				else stopInbox = unsub;
			})
			.catch(() => undefined);
		return () => {
			cancelled = true;
			stopInbox?.();
		};
	});

	onMount(() => {
		listen();
		return () => {
			leaving = true;
			stop?.();
			clearTimeout(retryTimer);
		};
	});
</script>

<!-- The app fills the window edge to edge. -->
<div class="exec-canvas fixed inset-0 overflow-hidden" {@attach navKeys.attach}>
	<div class="cozy-room" aria-hidden="true">
		<div class="cozy-alcove"></div>
		<div class="cozy-credenza"></div>
		<div class="cozy-plant"></div>
		<div class="cozy-alcove"></div>
	</div>
	<div class="cozy-shell" aria-hidden="true"></div>
	<div
		bind:this={desktopNavigation}
		class={[
			'fixed inset-y-0 z-40 hidden flex-col transition-[width] duration-300 ease-out min-[640px]:flex',
			sidebarCollapsed ? 'w-[68px]' : 'w-[264px]'
		]}
	>
		<ExecutiveSidebar
			connection={data.connection}
			needsYouCount={shellData.needsYouCount}
			projects={shellData.snapshot?.projects ?? []}
			collapsed={sidebarCollapsed}
			pinned={sidebarPinned}
			tessOpen={tessPanel.open}
			offline={showPaused}
			onOpenSearch={() => commandBar?.openPalette()}
			onToggleTess={() => tessPanel.toggle(uiPrefs.showTessPill)}
			onToggleCollapse={toggleSidebarPin}
			onMenuOpenChange={handleSidebarMenuOpenChange}
			onPointerEnter={handleSidebarEnter}
			onPointerLeave={handleSidebarLeave}
			onFocusIn={handleSidebarFocusIn}
			onFocusOut={handleSidebarFocusOut}
			onNavigate={(event, href) => selectDesktopSection(event, href, goto)}
		/>
	</div>
	<div
		class={[
			'exec-shell-main exec-legacy-compat relative z-10 flex h-full flex-col overflow-hidden transition-[padding] duration-300 ease-out',
			sidebarCollapsed ? 'min-[640px]:pl-[68px]' : 'min-[640px]:pl-[264px]'
		]}
	>
		<div data-shell-header class="flex shrink-0 items-center gap-2 px-4 min-[640px]:hidden">
			<MobileNavigation
				bind:open={mobileMenuOpen}
				connection={data.connection}
				needsYouCount={shellData.needsYouCount}
				projects={shellData.snapshot?.projects ?? []}
				onWiden={focusDesktopNavigation}
			/>
			<span class="text-sm font-semibold">{areaFor(page.url.pathname) ?? 'Agentic Org'}</span>
		</div>
		<ExecutiveCommandBar
			bind:this={commandBar}
			connection={data.connection}
			snapshot={shellData.snapshot}
			items={shellData.workItems}
			inboxItems={shellData.inboxItems}
			offices={shellData.offices}
			ownerId={shellData.ownerId}
			offline={showPaused}
			onToggleTess={() => tessPanel.toggle(uiPrefs.showTessPill)}
			onToggleTessExpand={() => tessPanel.toggleExpand()}
			onOpenShortcuts={() => (shortcutsOpen = true)}
			onAskTess={askTess}
		/>
		<div class="relative flex min-h-0 flex-1 flex-row overflow-hidden">
			<div class="relative flex min-h-0 min-w-0 flex-1 flex-col">
				{#if isHome}
					<HomeScrollHint scrollContainer={mainEl} />
				{/if}
				<main
					bind:this={mainEl}
					tabindex="-1"
					aria-label={areaFor(page.url.pathname) ?? 'Agentic Org'}
					class="flex min-h-0 flex-1 flex-col overflow-y-auto focus:outline-none"
				>
					{#if showPaused}<div class="shrink-0">
							<LiveBanner
								loadedAt={liveState.loadedAt[organizationId]}
								{retrying}
								onretry={retry}
							/>
						</div>{/if}
					{@render children()}
				</main>
			</div>
		</div>
		<TessPanel
			panel={tessPanel}
			snapshot={shellData.snapshot}
			items={shellData.workItems}
			inboxItems={shellData.inboxItems}
			offices={shellData.offices}
			ownerId={shellData.ownerId}
			offline={showPaused}
		/>
		<ShortcutsDialog bind:open={shortcutsOpen} />
	</div>
</div>
<Toaster position="bottom-right" />
{#if navKeys.hint}
	<div
		role="status"
		aria-live="polite"
		data-testid="nav-key-hint"
		class="pointer-events-none fixed bottom-4 left-1/2 z-50 -translate-x-1/2 rounded-md bg-surface-overlay px-3 py-1.5 text-xs/5 font-medium text-fg shadow-lg ring-1 ring-line"
	>
		{navKeys.hint}
	</div>
{/if}

<style>
	.exec-shell-main main > :global(:not(.chat-frame)) {
		flex-shrink: 0;
	}
	.exec-shell-main :global(.chat-frame) {
		flex: 1 1 0%;
		min-height: 0;
		height: auto;
	}
	@media (min-height: 640px) {
		.exec-shell-main :global(.activity-workspace),
		.exec-shell-main :global(.project-frame-activity) {
			flex: 1 1 0%;
			min-height: 0;
			height: auto;
		}
	}
	@media (min-width: 960px) and (min-height: 640px) {
		.exec-shell-main main > :global(.triage-page) {
			flex: 1 1 0%;
			min-height: 0;
			height: auto;
		}
	}
	@media (height <= 320px) {
		.exec-shell-main :global(.chat-frame) {
			min-height: 28rem;
		}
	}
</style>
