<script lang="ts">
	import ExecutiveCommandBar from '$lib/platform/shell/ExecutiveCommandBar.svelte';
	import ExecutiveSidebar from '$lib/platform/shell/ExecutiveSidebar.svelte';
	import MobileNavigation from '$lib/platform/shell/MobileNavigation.svelte';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import LiveBanner from '$lib/components/LiveBanner.svelte';
	import { Toaster } from '$lib/components/ui/sonner';

	import { onDestroy, onMount } from 'svelte';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { selectDesktopSection } from '$lib/platform/shell/desktop-navigation';
	import { navigating, page } from '$app/state';
	import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { orgApi } from '$lib/org/api';
	import { owner } from '$lib/org/model';
	import { noteChanged } from '$lib/org/live';
	import { liveState } from '$lib/org/live-state.svelte';
	import { workApi } from '$lib/work';
	import { needsYou } from '$lib/work/saved';
	import { areaFor } from '$lib/nav';
	import { inboxApi, type InboxItem } from '$lib/modules/inbox';
	import { officesApi, layerOffices } from '$lib/modules/offices';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { focusDestinationHeading } from '$lib/platform/shell/focus-destination';
	import { provideDraftStore } from '$lib/modules/messaging';

	let { data, children } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const chatDrafts = provideDraftStore(() => organizationId);
	$effect.pre(() => chatDrafts.syncScope());
	onDestroy(() => chatDrafts.dispose());

	// Route changes focus the destination heading (app-shell-ux §0), unless the page already moved focus.
	afterNavigate((move) => {
		if (move.type !== 'link' && move.type !== 'popstate') return;
		if (move.from?.url.pathname === move.to?.url.pathname) return;
		const focused = document.activeElement;
		if (focused && focused !== document.body && document.contains(focused)) return;
		void focusDestinationHeading();
	});
	onMount(() => void appSession.boot());

	let desktopNavigation = $state<HTMLDivElement | null>(null);
	let sidebarHovered = $state(false);
	let sidebarFocused = $state(false);
	let sidebarLeaveTimer: ReturnType<typeof setTimeout> | undefined;
	const sidebarPinned = $derived(!uiPrefs.navCollapsed);
	const sidebarCollapsed = $derived(uiPrefs.navCollapsed && !sidebarHovered && !sidebarFocused);

	function handleSidebarEnter() {
		clearTimeout(sidebarLeaveTimer);
		sidebarHovered = true;
	}
	function handleSidebarLeave() {
		clearTimeout(sidebarLeaveTimer);
		sidebarLeaveTimer = setTimeout(() => {
			sidebarHovered = false;
		}, 240);
	}
	function handleSidebarFocusIn(event: FocusEvent) {
		const target = event.target;
		if (target instanceof HTMLElement && target.matches(':focus-visible')) {
			clearTimeout(sidebarLeaveTimer);
			sidebarFocused = true;
		}
	}
	function handleSidebarFocusOut(event: FocusEvent) {
		const next = event.relatedTarget;
		if (next instanceof Node && desktopNavigation?.contains(next)) return;
		clearTimeout(sidebarLeaveTimer);
		sidebarLeaveTimer = setTimeout(() => {
			sidebarFocused = false;
		}, 160);
	}
	function toggleSidebarPin() {
		clearTimeout(sidebarLeaveTimer);
		uiPrefs.toggleNavCollapsed();
		if (uiPrefs.navCollapsed) {
			sidebarHovered = false;
			sidebarFocused = false;
		}
	}
	onDestroy(() => clearTimeout(sidebarLeaveTimer));

	let shellSnapshot = $state.raw<OrgSnapshot | undefined>();
	let shellWork = $state.raw<WorkItemSummary[]>([]);
	let shellInbox = $state.raw<InboxItem[]>([]);
	let shellOffices = $state.raw<OfficeInfo[]>([]);
	const shellOwnerId = $derived(shellSnapshot ? (owner(shellSnapshot)?.id ?? 'owner') : 'owner');
	const shellOpenInbox = $derived(shellInbox.filter((i) => i.state === 'open').length);
	const shellNeedsYou = $derived(
		shellOpenInbox > 0 ? shellOpenInbox : needsYou(shellWork, shellOwnerId)
	);
	const activeOffices = $derived(
		shellOffices.length > 0 ? shellOffices : data.connection.organization.offices
	);

	let shellLoadSeq = 0;

	function loadShellData(id: string, reset = false) {
		const seq = ++shellLoadSeq;
		let cancelled = false;
		if (reset) {
			shellSnapshot = undefined;
			shellWork = [];
			shellInbox = [];
			shellOffices = [];
		}
		const baseOffices = data.connection.organization.offices;
		void Promise.allSettled([
			orgApi.view(id),
			workApi.snapshot(id),
			inboxApi.list(id),
			officesApi.overview(id)
		]).then(([orgRes, workRes, inboxRes, officesRes]) => {
			if (cancelled || seq !== shellLoadSeq) return;
			if (orgRes.status === 'fulfilled') shellSnapshot = orgRes.value.snapshot;
			if (workRes.status === 'fulfilled') shellWork = workRes.value.items;
			if (inboxRes.status === 'fulfilled') shellInbox = inboxRes.value;
			const overview = officesRes.status === 'fulfilled' ? officesRes.value : undefined;
			shellOffices = layerOffices(baseOffices, overview);
		});
		return () => {
			cancelled = true;
		};
	}

	$effect(() => loadShellData(organizationId, true));

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
		loadShellData(organizationId);
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
	let leaving = false;
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
<div class="exec-canvas fixed inset-0 overflow-hidden">
	<div
		bind:this={desktopNavigation}
		class={[
			'fixed inset-y-0 z-40 hidden flex-col transition-[width] duration-300 ease-out min-[640px]:flex',
			sidebarCollapsed ? 'w-[68px]' : 'w-[264px]'
		]}
	>
		<ExecutiveSidebar
			connection={data.connection}
			needsYouCount={shellNeedsYou}
			projects={shellSnapshot?.projects ?? []}
			collapsed={sidebarCollapsed}
			pinned={sidebarPinned}
			onToggleCollapse={toggleSidebarPin}
			onPointerEnter={handleSidebarEnter}
			onPointerLeave={handleSidebarLeave}
			onFocusIn={handleSidebarFocusIn}
			onFocusOut={handleSidebarFocusOut}
			onNavigate={(event, href) => selectDesktopSection(event, href, goto)}
		/>
	</div>
	<main
		aria-label={areaFor(page.url.pathname) ?? 'Agentic Org'}
		class={[
			'exec-legacy-compat relative z-10 h-full overflow-y-auto transition-[padding] duration-300 ease-out',
			sidebarCollapsed ? 'min-[640px]:pl-[68px]' : 'min-[640px]:pl-[264px]'
		]}
	>
		<div class="flex items-center gap-2 px-4 min-[640px]:hidden">
			<MobileNavigation
				connection={data.connection}
				needsYouCount={shellNeedsYou}
				projects={shellSnapshot?.projects ?? []}
				onWiden={focusDesktopNavigation}
			/>
			<span class="text-sm font-semibold">{areaFor(page.url.pathname) ?? 'Agentic Org'}</span>
		</div>
		<ExecutiveCommandBar
			snapshot={shellSnapshot}
			items={shellWork}
			inboxItems={shellInbox}
			offices={activeOffices}
			ownerId={shellOwnerId}
		/>
		{#if showPaused}<LiveBanner
				loadedAt={liveState.loadedAt[organizationId]}
				{retrying}
				onretry={retry}
			/>{/if}
		{@render children()}
	</main>
</div>
<Toaster position="bottom-right" />
