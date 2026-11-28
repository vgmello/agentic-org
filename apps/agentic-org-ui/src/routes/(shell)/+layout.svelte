<script lang="ts">
	import ExecutiveCommandBar from '$lib/platform/shell/ExecutiveCommandBar.svelte';
	import ExecutiveSidebar from '$lib/platform/shell/ExecutiveSidebar.svelte';
	import MobileHeaderBar from '$lib/platform/shell/MobileHeaderBar.svelte';
	import ShortcutsDialog from '$lib/platform/shell/ShortcutsDialog.svelte';
	import { SidebarDockController } from '$lib/platform/shell/sidebar-dock.svelte';
	import { provideNotifications } from '$lib/platform/workspace/notifications.svelte';
	import { personPanel } from '$lib/platform/workspace/person-panel.svelte';
	import TessPanel from '$lib/modules/tess/TessPanel.svelte';
	import { createHttpTessApi } from '$lib/modules/tess/api';
	import { provideTessPanel, TessPanelState } from '$lib/modules/tess/tess-panel.svelte';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';
	import { captureOverlayOpener } from '$lib/components/focus';
	import LiveBanner from '$lib/components/LiveBanner.svelte';
	import { Toaster } from '$lib/components/ui/sonner';

	import { onDestroy, onMount, untrack } from 'svelte';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { selectDesktopSection } from '$lib/platform/shell/desktop-navigation';
	import { navigating, page } from '$app/state';
	import { inboxApi } from '$lib/modules/inbox/api';
	import { clearInboxDeviceComments } from '$lib/modules/inbox/device-comments';
	import { orgApi } from '$lib/modules/organization/api';
	import { noteChanged } from '$lib/modules/organization/live';
	import { liveState } from '$lib/modules/organization/live-state.svelte';
	import { areaFor } from '$lib/nav';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { focusDestinationHeading } from '$lib/platform/workspace/focus-destination';
	import { noteModalBackNavigation } from '$lib/platform/workspace/modal-back';
	import { NavKeyController } from '$lib/platform/shell/nav-keys.svelte';
	import {
		createScopedSearchHost,
		setScopedSearchContext
	} from '$lib/platform/workspace/scoped-search.svelte';
	import { provideShellAi, type ShellAiAskOptions } from '$lib/platform/workspace/shell-ai';
	import { provideShellCommand } from '$lib/platform/workspace/shell-command';
	import { provideShellData } from '$lib/platform/shell/shell-data.svelte';
	import { provideDraftStore } from '$lib/modules/messaging/drafts/context';
	import { officesApi } from '$lib/modules/offices/api';
	import { layerOffices } from '$lib/modules/offices/layer';
	import type PersonSidePanelType from '$lib/modules/shell/features/person-panel/PersonSidePanel.svelte';
	import type { PersonPanelDataSource } from '$lib/modules/shell/features/person-panel/types';

	let { data, children } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const isHome = $derived(page.url.pathname === '/home');
	const isChat = $derived(page.url.pathname === '/chat' || page.url.pathname.startsWith('/chat/'));
	const chatDrafts = provideDraftStore(() => organizationId);
	$effect.pre(() => chatDrafts.syncScope());
	onDestroy(() => chatDrafts.dispose());

	const tessPanel = provideTessPanel(
		new TessPanelState(
			createHttpTessApi({
				session: {
					submit: (path, body, signal) => appSession.controller().submit(path, body, signal),
					openSseStream: (path, signal) => appSession.controller().openSseStream(path, signal)
				}
			})
		)
	);
	$effect(() => {
		tessPanel.setOrganization(organizationId);
		personPanel.setOrganization(organizationId);
	});
	onDestroy(() => tessPanel.dispose());
	const stopCommentCleanup = appSession.onSessionEnded(() => clearInboxDeviceComments());
	onDestroy(() => stopCommentCleanup());
	provideNotifications();
	let shortcutsOpen = $state(false);
	let commandBar = $state<ReturnType<typeof ExecutiveCommandBar> | undefined>();

	let PersonPanel = $state.raw<typeof PersonSidePanelType | undefined>(undefined);
	let personPanelSource = $state.raw<PersonPanelDataSource | undefined>(undefined);
	let personPanelLoading = false;
	function fallbackToMemberRoute() {
		if (leaving || PersonPanel) return;
		const fallbackId = personPanel.memberId;
		personPanel.close(false);
		if (fallbackId) {
			void goto(resolve(`/members/${encodeURIComponent(fallbackId)}` as '/teams/members'));
		}
	}

	async function ensurePersonPanelLoaded() {
		if ((PersonPanel && personPanelSource) || personPanelLoading) return;
		personPanelLoading = true;
		try {
			const [panelMod, sourceMod] = await Promise.all([
				import('$lib/modules/shell/features/person-panel/PersonSidePanel.svelte'),
				import('$lib/platform/shell/person-panel-source')
			]);
			if (!leaving) {
				PersonPanel = panelMod.default;
				personPanelSource = sourceMod.createPersonPanelDataSource(() =>
					shellData.personPanelContext()
				);
			}
		} catch {
			fallbackToMemberRoute();
		} finally {
			personPanelLoading = false;
		}
	}
	const stopPersonPanelSession = personPanel.bindSessionEnd((cb) => appSession.onSessionEnded(cb));
	const stopPersonPanelOpen = personPanel.onOpen(() => {
		if (!personPanel.opener) personPanel.captureOpener(captureOverlayOpener());
		void ensurePersonPanelLoaded();
	});
	onDestroy(() => {
		stopPersonPanelSession();
		stopPersonPanelOpen();
		personPanel.close(false);
	});

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
				organizationId,
				snapshot: shellData.snapshot,
				items: shellData.workItems,
				offices: shellData.offices,
				ownerId: shellData.ownerId,
				domain: { inboxItems: shellData.inboxItems },
				routeLabel: areaFor(page.url.pathname) ?? 'Home',
				routePath: page.url.pathname,
				offline: showPaused
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
			if (move.type !== 'popstate') mobileMenuOpen = false;
			personPanel.close(false);
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
	const navKeys = new NavKeyController({
		onSidebarOpen() {
			if (typeof window !== 'undefined' && window.innerWidth < 640) {
				mobileMenuOpen = true;
			} else {
				sidebarDock.openForKeyMode();
			}
		},
		onSidebarCancel() {
			if (typeof window !== 'undefined' && window.innerWidth < 640 && mobileMenuOpen) {
				mobileMenuOpen = false;
			}
		}
	});
	const sidebarDock = new SidebarDockController({
		isSidebarKeyMode: () => navKeys.mode === 'sidebar'
	});
	onDestroy(() => sidebarDock.dispose());

	const shellData = provideShellData({
		getOrganizationId: () => organizationId,
		getBaseOffices: () => data.connection.organization.offices,
		loadOffices: (id, baseOffices, signal) =>
			officesApi
				.overview(id, signal)
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

	let stop: (() => void) | undefined;
	function listen() {
		orgApi
			.onFeed((notice) => {
				if (notice.kind === 'live') {
					liveNotices[notice.organization_id] = (liveNotices[notice.organization_id] ?? 0) + 1;
					liveState.paused[notice.organization_id] = !notice.live;
					if (notice.live && notice.organization_id === organizationId) retrying = false;
				} else if (notice.organization_id === organizationId) noteChanged(notice, reload);
			})
			.then((unlisten) => {
				if (leaving) unlisten();
				else {
					stop = unlisten;
					cannotListen = false;
					retrying = false;
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

<!-- The app fills the window edge to edge. At 200% text (32px root), 960–1279px viewports (< 40rem)
  reflow the sidebar to a 4rem (128px) rail so page content stays >= 400px without horizontal scroll (SHELL-30). -->
<div class="exec-canvas fixed inset-0 @container overflow-hidden" {@attach navKeys.attach}>
	<div class="cozy-room" aria-hidden="true">
		<div class="cozy-alcove"></div>
		<div class="cozy-credenza"></div>
		<div class="cozy-plant"></div>
		<div class="cozy-alcove"></div>
	</div>
	<div class="cozy-shell" aria-hidden="true"></div>
	<div
		bind:this={desktopNavigation}
		{@attach sidebarDock.attachReflow}
		class={[
			'fixed inset-y-0 z-40 hidden flex-col overflow-clip transition-[width] duration-300 ease-out motion-reduce:transition-none min-[640px]:flex @max-[40rem]:transition-none',
			sidebarDock.collapsed ? 'w-[68px]' : 'w-16 @min-[40rem]:w-[264px]'
		]}
	>
		<ExecutiveSidebar
			connection={data.connection}
			needsYouCount={shellData.needsYouCount}
			projects={shellData.snapshot?.projects ?? []}
			workItems={shellData.workItems}
			inboxItems={shellData.inboxItems}
			projectTeams={shellData.validProjectTeams}
			projectHealthById={shellData.projectHealthById}
			collapsed={sidebarDock.collapsed || sidebarDock.textReflowRail}
			pinned={sidebarDock.pinned}
			tessOpen={tessPanel.open}
			offline={showPaused}
			onOpenSearch={() => commandBar?.openPalette()}
			onToggleTess={() => tessPanel.toggle(uiPrefs.showTessPill)}
			onToggleCollapse={() => sidebarDock.togglePin()}
			onMenuOpenChange={(open) => sidebarDock.onMenuOpenChange(open, desktopNavigation)}
			onPointerEnter={() => sidebarDock.onPointerEnter()}
			onPointerLeave={() => sidebarDock.onPointerLeave()}
			onFocusIn={(event) => sidebarDock.onFocusIn(event, desktopNavigation)}
			onFocusOut={(event) => sidebarDock.onFocusOut(event, desktopNavigation)}
			onNavigate={(event, href) => selectDesktopSection(event, href, goto)}
		/>
	</div>
	<div
		class={[
			'exec-shell-main exec-legacy-compat relative z-10 flex h-full flex-col overflow-hidden transition-[padding] duration-300 ease-out motion-reduce:transition-none @max-[40rem]:transition-none',
			sidebarDock.collapsed
				? 'min-[640px]:pl-[68px]'
				: 'min-[640px]:pl-16 min-[640px]:@min-[40rem]:pl-[264px]'
		]}
	>
		<MobileHeaderBar
			bind:open={mobileMenuOpen}
			areaLabel={areaFor(page.url.pathname) ?? 'Agentic Org'}
			connection={data.connection}
			needsYouCount={shellData.needsYouCount}
			projects={shellData.snapshot?.projects ?? []}
			workItems={shellData.workItems}
			inboxItems={shellData.inboxItems}
			projectTeams={shellData.validProjectTeams}
			projectHealthById={shellData.projectHealthById}
			tessOpen={tessPanel.open}
			offline={showPaused}
			onOpenSearch={() => commandBar?.openPalette()}
			onToggleTess={() => tessPanel.toggle(uiPrefs.showTessPill)}
			onWiden={focusDesktopNavigation}
		/>
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
				<main
					bind:this={mainEl}
					tabindex="-1"
					aria-label={areaFor(page.url.pathname) ?? 'Agentic Org'}
					class="flex min-h-0 flex-1 flex-col overflow-y-auto focus:outline-none"
				>
					{#if showPaused}<div class="shrink-0">
							<LiveBanner
								loadedAt={liveState.loadedAt[organizationId]}
								retrying={retrying && showPaused}
								onretry={retry}
							/>
						</div>{/if}
					{#if isChat}
						<h1 id="shell-page-title" tabindex="-1" class="sr-only focus:outline-none">Chat</h1>
					{/if}
					{@render children()}
				</main>
			</div>
		</div>
		{#if PersonPanel && personPanelSource}
			{@const PanelComp = PersonPanel}
			<PanelComp
				open={personPanel.isOpen}
				memberId={personPanel.memberId}
				openSeq={personPanel.openSeq}
				opener={personPanel.opener}
				restoreFocusOnClose={personPanel.restoreFocusOnClose}
				{organizationId}
				snapshot={shellData.snapshot}
				source={personPanelSource}
				onClose={(restore) => personPanel.close(restore)}
				onMessageMember={(id) => {
					personPanel.close(false);
					void commandBar?.openMemberDm(id);
				}}
			/>
		{/if}
		<TessPanel
			panel={tessPanel}
			{organizationId}
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
