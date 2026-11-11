<script lang="ts">
	import { beforeNavigate, goto } from '$app/navigation';
	import { onMount, tick } from 'svelte';
	import { innerWidth } from 'svelte/reactivity/window';
	import MenuIcon from '@lucide/svelte/icons/menu';
	import type { Connection } from '$lib/proto';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import * as Sheet from '$lib/components/ui/sheet';

	let { connection, onWiden }: { connection: Connection; onWiden: () => void } = $props();
	const menuId = $props.id();
	let open = $state(false);
	let content = $state<HTMLDivElement | null>(null);
	let trigger = $state<HTMLButtonElement | null>(null);
	let closingForNavigation = false;
	let traversal: { from: string; delta: number } | undefined;
	let restoreFocusAt: string | undefined;

	// A viewport change must release the modal before the desktop navigation takes over.
	$effect(() => {
		if ((innerWidth.current ?? 0) >= 640 && open) open = false;
	});

	beforeNavigate((navigation) => {
		if (!open || navigation.type !== 'popstate' || (navigation.delta ?? 0) >= 0) return;
		restoreFocusAt = traversal?.from ?? navigation.from?.url.href;
		navigation.cancel();
		open = false;
	});

	onMount(() => {
		const navigation = window.navigation;
		if (!navigation) return;
		function rememberTraversal(event: NavigateEvent) {
			if (!open) return;
			const from = navigation.currentEntry?.index ?? -1;
			const to = event.destination.index;
			traversal = event.navigationType === 'traverse' && from >= 0 && to >= 0
				? { from: window.location.href, delta: to - from }
				: undefined;
		}
		navigation.addEventListener('navigate', rememberTraversal);
		return () => navigation.removeEventListener('navigate', rememberTraversal);
	});

	function returnFromBack(from: string, delta: number) {
		restoreFocusAt = from;
		window.history.go(-delta);
		open = false;
	}

	function handleFragmentChange({ oldURL, newURL }: HashChangeEvent) {
		if (!open) return;
		const from = new URL(oldURL);
		const to = new URL(newURL);
		if (from.pathname !== to.pathname || from.search !== to.search) return;

		// SvelteKit can update fragment history without invoking beforeNavigate.
		if (traversal?.from === oldURL && traversal.delta < 0) {
			returnFromBack(oldURL, traversal.delta);
		} else if (!window.navigation && oldURL.includes('#') && !newURL.includes('#')) {
			// A native hash addition always has a #. This handles Back to the unfragmented page.
			returnFromBack(oldURL, -1);
		}
	}

	function handlePopState(event: PopStateEvent) {
		if (restoreFocusAt) {
			const url = restoreFocusAt;
			restoreFocusAt = undefined;
			if (url !== window.location.href) return;
			// The browser can restore document focus as it returns to the original entry.
			requestAnimationFrame(() => {
				if (!open && trigger?.isConnected) focusNavigationControl();
			});
		} else if (open && event.state === null && traversal && traversal.delta < 0) {
			// A native fragment entry can lack SvelteKit state, even on a previous route.
			returnFromBack(traversal.from, traversal.delta);
		}
	}

	function menuOpenChanged(value: boolean) {
		if (!value) return;
		closingForNavigation = false;
		traversal = undefined;
		restoreFocusAt = undefined;
	}

	function closeForNavigation() {
		closingForNavigation = true;
		open = false;
	}
	async function focusDestinationHeading() {
		await tick();
		const heading = document.querySelector<HTMLElement>('main h1');
		if (heading) {
			heading.tabIndex = -1;
			heading.focus({ preventScroll: true });
		}
	}
	function focusNavigationControl() {
		if ((innerWidth.current ?? 0) >= 640) onWiden();
		else trigger?.focus({ preventScroll: true });
	}
	async function focusCurrent(event: Event) {
		event.preventDefault();
		await tick();
		if (open) content?.querySelector<HTMLElement>('nav [aria-current="page"]')?.focus();
	}
	async function restoreFocus(event: Event) {
		event.preventDefault();
		if (closingForNavigation) return;
		await tick();
		if (!open) focusNavigationControl();
	}
	async function select(event: MouseEvent, href: string) {
		if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
		event.preventDefault();
		if (!open) return;
		const from = window.location.href;
		closeForNavigation();
		try {
			if (new URL(href, from).href !== from) await goto(href);
			await focusDestinationHeading();
		} catch {
			// SvelteKit owns route errors. Restore the trigger only if this page stayed put.
			if (window.location.href === from && trigger?.isConnected && !open) focusNavigationControl();
		}
	}
</script>

<svelte:window onhashchange={handleFragmentChange} onpopstate={handlePopState} />

<Sheet.Root bind:open onOpenChange={menuOpenChanged}>
	<Sheet.Trigger bind:ref={trigger} aria-controls={menuId} class="-ml-2 inline-flex size-11 shrink-0 items-center justify-center rounded-md text-white hover:bg-white/5 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring min-[640px]:hidden">
		<MenuIcon class="size-6" aria-hidden="true" strokeWidth={1.5} /><span class="sr-only">Menu</span>
	</Sheet.Trigger>
	<Sheet.Content id={menuId} bind:ref={content} side="left" onOpenAutoFocus={focusCurrent} onCloseAutoFocus={restoreFocus}
		class="gap-0 border-0 bg-gray-800 p-0 data-[side=left]:w-72 data-[side=left]:max-w-[calc(100vw-56px)] [&_[data-slot=sheet-close]]:size-11">
		<Sheet.Title class="sr-only">Main navigation</Sheet.Title>
		<Sheet.Description class="sr-only">Choose an area of the organization.</Sheet.Description>
		<Sidebar {connection} presentation="drawer" onNavigate={select} />
	</Sheet.Content>
</Sheet.Root>
