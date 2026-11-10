<script lang="ts">
	import Sidebar from '$lib/components/Sidebar.svelte';
	import MobileNavigation from '$lib/platform/shell/MobileNavigation.svelte';
	import LiveBanner from '$lib/components/LiveBanner.svelte';
	import { Toaster } from '$lib/components/ui/sonner';

	import { onMount } from 'svelte';
	import { invalidate } from '$app/navigation';
	import { navigating, page } from '$app/state';
	import { orgApi } from '$lib/org/api';
	import { noteChanged } from '$lib/org/live';
	import { liveState } from '$lib/org/live-state.svelte';
	import { navFor } from '$lib/nav';

	let { data, children } = $props();
	const organizationId = $derived(data.connection.organization.id);
	let desktopNavigation = $state<HTMLDivElement | null>(null);
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
				}
				else if (notice.organization_id === organizationId) noteChanged(notice, reload);
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
				if (live !== undefined && (liveNotices[id] ?? 0) === noticesBefore) liveState.paused[id] = !live;
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
<div class="fixed inset-0 overflow-hidden bg-gray-800">
	<div bind:this={desktopNavigation} class="fixed inset-y-0 z-50 hidden w-16 flex-col min-[640px]:flex min-[960px]:w-72">
		<Sidebar connection={data.connection} />
	</div>
	<main aria-labelledby="shell-page-title" class="h-full overflow-y-auto min-[640px]:pl-16 min-[960px]:pl-72">
		<!-- Persistent page heading. -->
		<div data-shell-header class="sticky top-0 z-40 flex h-14 shrink-0 items-center gap-3 border-b border-white/5 bg-gray-800 px-4 min-[640px]:h-16 min-[640px]:px-8">
			<MobileNavigation connection={data.connection} onWiden={focusDesktopNavigation} />
			<h1 id="shell-page-title" tabindex="-1" class="min-w-0 text-base/7 font-semibold text-white focus:outline-none">{navFor(page.url.pathname)?.name}</h1>
		</div>
		{#if showPaused}<LiveBanner loadedAt={liveState.loadedAt[organizationId]} {retrying} onretry={retry} />{/if}
		{@render children()}
	</main>
</div>
<Toaster position="bottom-right" />
