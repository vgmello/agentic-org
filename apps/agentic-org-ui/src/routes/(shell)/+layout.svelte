<script lang="ts">
	import Sidebar from '$lib/components/Sidebar.svelte';
	import ExecutiveCommandBar from '$lib/platform/shell/ExecutiveCommandBar.svelte';
	import ExecutiveSidebar from '$lib/platform/shell/ExecutiveSidebar.svelte';
	import MobileNavigation from '$lib/platform/shell/MobileNavigation.svelte';
	import ShellTopbar from '$lib/platform/shell/ShellTopbar.svelte';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import LiveBanner from '$lib/components/LiveBanner.svelte';
	import { Toaster } from '$lib/components/ui/sonner';

	import { onDestroy, onMount } from 'svelte';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { selectDesktopSection } from '$lib/platform/shell/desktop-navigation';
	import { navigating, page } from '$app/state';
	import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { orgApi } from '$lib/org/api';
	import { owner } from '$lib/org/model';
	import { noteChanged } from '$lib/org/live';
	import { liveState } from '$lib/org/live-state.svelte';
	import { workApi } from '$lib/work';
	import { needsYou } from '$lib/work/saved';
	import { areaFor } from '$lib/nav';
	import { shellApi } from '$lib/modules/shell/api';
	import type { ShellNavigation } from '$lib/modules/shell/types';
	import { officeApi } from '$lib/office/api';
	import { createProbe } from '$lib/offices/probe';
	import { displayStatus, statusInfo, type LiveCheck } from '$lib/offices/model';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { focusDestinationHeading } from '$lib/platform/shell/focus-destination';
	import { provideDraftStore } from '$lib/modules/messaging';

	let { data, children } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const chatDrafts = provideDraftStore(() => organizationId);
	$effect.pre(() => chatDrafts.syncScope());
	onDestroy(() => chatDrafts.dispose());

	// The navigation's Inbox count and Projects/Teams lists. Refused (not served) leaves them out.
	let navigation = $state.raw<ShellNavigation | undefined>();
	let navigationRequest = 0;
	function loadNavigation(id: string) {
		const mine = ++navigationRequest;
		shellApi
			.navigation(id)
			.then((next) => {
				if (mine === navigationRequest) navigation = next;
			})
			.catch(() => {
				if (mine === navigationRequest) navigation = undefined;
			});
	}
	$effect(() => loadNavigation(organizationId));

	// The footer's office line: the saved status is never shown as live; the office answering hello verifies
	// it (the same probe and words as Offices). Checked on opening and every half minute.
	let officeCheck = $state<LiveCheck>('checking');
	let officeCapabilities = $state.raw<string[] | undefined>();
	const officeProbe = createProbe(officeApi, (next, capabilities) => {
		officeCheck = next;
		if (next === 'up') officeCapabilities = capabilities;
	});
	onMount(() => {
		void officeProbe.run('background');
		const again = setInterval(() => void officeProbe.run('background'), 30_000);
		return () => clearInterval(again);
	});
	const officeStatus = $derived(
		statusInfo(displayStatus(true, officeCheck, data.connection.office.status, officeCapabilities))
	);

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
	let shellSnapshot = $state.raw<OrgSnapshot | undefined>();
	let shellWork = $state.raw<WorkItemSummary[]>([]);
	const shellOwnerId = $derived(shellSnapshot ? (owner(shellSnapshot)?.id ?? 'owner') : 'owner');
	const shellNeedsYou = $derived(needsYou(shellWork, shellOwnerId));

	$effect(() => {
		const id = organizationId;
		if (!uiPrefs.executiveUi) return;
		let cancelled = false;
		void Promise.allSettled([orgApi.view(id), workApi.snapshot(id)]).then(([orgRes, workRes]) => {
			if (cancelled) return;
			if (orgRes.status === 'fulfilled') shellSnapshot = orgRes.value.snapshot;
			if (workRes.status === 'fulfilled') shellWork = workRes.value.items;
		});
		return () => {
			cancelled = true;
		};
	});
	function focusDesktopNavigation() {
		desktopNavigation?.querySelector<HTMLElement>('a[aria-current="page"]')?.focus();
	}

	// Loading the latest now would cancel a navigation in flight (the click would be lost), so wait for it to end: the page
	// it goes to loads the latest itself, and the catch-up in live.ts asks again if that load is still behind. A navigation
	// that was cancelled leaves the page where it was: nothing else will load the latest then, so do it.
	async function reload() {
		if (navigating.to) {
			const from = page.url.href;
			while (navigating.to) await new Promise((resolve) => setTimeout(resolve, 50));
			if (page.url.href !== from) return;
		}
		await invalidate('app:org');
		loadNavigation(organizationId);
	}

	// Whether the live feed is off, by organization (`liveState`): the screens then show what they last loaded, and say so.
	// Listening itself failed (not the office): Retry listens again.
	let cannotListen = $state(false);
	let retrying = $state(false);
	let listening = $state(false);
	// How many `live` notices each organization has sent, to tell an answer from `feedLive` that a newer notice overtook.
	const liveNotices: Record<string, number> = {};
	let retryTimer: ReturnType<typeof setTimeout> | undefined;
	const showPaused = $derived(liveState.paused[organizationId] === true || cannotListen);
	$effect(() => {
		if (!showPaused) retrying = false;
	});

	let stop: (() => void) | undefined;
	let leaving = false;
	// The office says when the organization changes (someone else's command, or ours): the screens load it again, unless
	// they already show it. It also says when it cannot keep telling us.
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
	// What the feed said before this was listening (it went down while the shell was away or starting), and again for each
	// organization the shell switches to: its last notice may have been missed.
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
		// The feed answers by saying it is live; if it is not back, the button comes back after a moment.
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
{#if uiPrefs.executiveUi}
	<div class="exec-canvas fixed inset-0 overflow-hidden">
		<div
			bind:this={desktopNavigation}
			class={[
				'fixed inset-y-0 z-40 hidden flex-col transition-[width] duration-200 min-[640px]:flex',
				uiPrefs.navCollapsed ? 'w-[68px]' : 'w-[264px]'
			]}
		>
			<ExecutiveSidebar
				connection={data.connection}
				needsYouCount={shellNeedsYou}
				projects={shellSnapshot?.projects ?? []}
				collapsed={uiPrefs.navCollapsed}
				onToggleCollapse={() => uiPrefs.toggleNavCollapsed()}
				onNavigate={(event, href) => selectDesktopSection(event, href, goto)}
			/>
		</div>
		<main
			aria-labelledby="shell-page-title"
			class={[
				'exec-legacy-compat relative z-10 h-full overflow-y-auto transition-[padding] duration-200',
				uiPrefs.navCollapsed ? 'min-[640px]:pl-[68px]' : 'min-[640px]:pl-[264px]'
			]}
		>
			<div class="flex items-center gap-2 px-4 min-[640px]:hidden">
				<MobileNavigation
					connection={data.connection}
					{navigation}
					{officeStatus}
					onWiden={focusDesktopNavigation}
				/>
				<span id="shell-page-title" class="text-sm font-semibold"
					>{areaFor(page.url.pathname) ?? 'Agentic Org'}</span
				>
			</div>
			<ExecutiveCommandBar
				snapshot={shellSnapshot}
				items={shellWork}
				offices={data.connection.organization.offices}
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
{:else}
	<div class="exec-legacy-compat fixed inset-0 overflow-hidden bg-surface">
		<div
			bind:this={desktopNavigation}
			class="fixed inset-y-0 z-50 hidden w-16 flex-col min-[640px]:flex min-[960px]:w-72"
		>
			<Sidebar
				connection={data.connection}
				{navigation}
				{officeStatus}
				onNavigate={(event, href) => selectDesktopSection(event, href, goto)}
			/>
		</div>
		<main
			aria-labelledby="shell-page-title"
			class="h-full overflow-y-auto min-[640px]:pl-16 min-[960px]:pl-72"
		>
			<ShellTopbar heading={areaFor(page.url.pathname)} connection={data.connection}>
				<MobileNavigation
					connection={data.connection}
					{navigation}
					{officeStatus}
					onWiden={focusDesktopNavigation}
				/>
			</ShellTopbar>
			{#if showPaused}<LiveBanner
					loadedAt={liveState.loadedAt[organizationId]}
					{retrying}
					onretry={retry}
				/>{/if}
			{@render children()}
		</main>
	</div>
{/if}
<Toaster position="bottom-right" />
