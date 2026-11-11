<script lang="ts">
	import { afterNavigate, beforeNavigate, goto } from '$app/navigation';
	import { tick } from 'svelte';
	import { innerWidth } from 'svelte/reactivity/window';
	import MenuIcon from '@lucide/svelte/icons/menu';
	import type { Connection } from '$lib/proto';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import * as Sheet from '$lib/components/ui/sheet';
	import { registerModalBack } from './modal-back';
	import { focusDestinationHeading } from './focus-destination';

	let { connection, onWiden }: { connection: Connection; onWiden: () => void } = $props();
	const menuId = $props.id();
	let open = $state(false);
	let content = $state<HTMLDivElement | null>(null);
	let trigger = $state<HTMLButtonElement | null>(null);
	let closingForNavigation = false;
	let legacyReturnUrl: string | undefined;
	let legacyHistoryLength = 0;
	let legacyAppendedFragment: string | undefined;
	let openedOnUntrackedLegacyEntry = false;

	// A viewport change must release the modal before the desktop navigation takes over.
	$effect(() => {
		if ((innerWidth.current ?? 0) >= 640 && open) open = false;
	});

	$effect(() => {
		if (open) return registerModalBack(() => { open = false; });
	});

	afterNavigate(() => {
		if (window.navigation) return;
		legacyHistoryLength = window.history.length;
		legacyAppendedFragment = undefined;
	});

	beforeNavigate((navigation) => {
		if (window.navigation || !open || navigation.type !== 'popstate'
			|| (navigation.delta ?? 0) >= 0 || openedOnUntrackedLegacyEntry) return;
		legacyReturnUrl = navigation.from?.url.href;
		navigation.cancel();
		open = false;
	});

	function handleFragmentChange({ oldURL, newURL }: HashChangeEvent) {
		if (!open || legacyAppendedFragment !== oldURL || newURL.includes('#')) return;
		const from = new URL(oldURL);
		const to = new URL(newURL);
		if (from.pathname !== to.pathname || from.search !== to.search) return;

		// Older browsers can safely reverse Back from an observed append to its base URL.
		// Arbitrary native fragment histories need the Navigation API's traversal direction.
		legacyReturnUrl = oldURL;
		window.history.forward();
		open = false;
	}

	function handlePopState(event: PopStateEvent) {
		if (window.navigation) return;
		if (legacyReturnUrl) {
			const url = legacyReturnUrl;
			legacyReturnUrl = undefined;
			requestAnimationFrame(() => {
				if (!open && trigger?.isConnected && window.location.href === url) focusNavigationControl();
			});
		} else if (!open) {
			const length = window.history.length;
			legacyAppendedFragment = event.state === null && length > legacyHistoryLength && window.location.hash
				? window.location.href
				: undefined;
			legacyHistoryLength = length;
		}
	}

	function menuOpenChanged(value: boolean) {
		if (!value) return;
		closingForNavigation = false;
		legacyReturnUrl = undefined;
		openedOnUntrackedLegacyEntry = !window.navigation && window.history.state === null;
	}

	function closeForNavigation() {
		closingForNavigation = true;
		open = false;
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
