<script lang="ts">
	import { goto, pushState } from '$app/navigation';
	import { page } from '$app/state';
	import { tick } from 'svelte';
	import { innerWidth } from 'svelte/reactivity/window';
	import MenuIcon from '@lucide/svelte/icons/menu';
	import type { Connection } from '$lib/proto';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import * as Sheet from '$lib/components/ui/sheet';

	let { connection, onWiden }: { connection: Connection; onWiden: () => void } = $props();
	const menuId = $props.id();
	type MenuHistoryState = App.PageState & { shellMenu?: true };

	let openedHere = $state(false);
	let content = $state<HTMLDivElement | null>(null);
	let trigger = $state<HTMLButtonElement | null>(null);
	let dismissing = false;
	let closeReason: 'dismiss' | 'current-page' | 'navigation' = 'dismiss';
	const hasMenuEntry = $derived((page.state as MenuHistoryState).shellMenu === true);
	const open = $derived(openedHere && hasMenuEntry);

	$effect(() => {
		if (!hasMenuEntry) {
			openedHere = false;
			dismissing = false;
		} else if ((innerWidth.current ?? 0) >= 640 && open) {
			setMenuOpen(false);
		}
	});

	function setMenuOpen(value: boolean) {
		if (value) {
			if (open || dismissing) return;
			closeReason = 'dismiss';
			// A same-URL entry lets Back dismiss the drawer while preserving the page and fragment.
			if (!hasMenuEntry || window.history.state === null) {
				pushState(window.location.href, { ...page.state, shellMenu: true });
			}
			openedHere = true;
		} else if (open && !dismissing) {
			dismissing = true;
			window.history.back();
		}
	}

	function handlePopState(event: PopStateEvent) {
		// Native fragment entries can have no state, so SvelteKit leaves page.state alone.
		if (event.state === null && open) {
			openedHere = false;
			dismissing = false;
		}
	}

	function focusDestinationHeading() {
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
		if (closeReason === 'navigation') return;
		await tick();
		if (open) return;
		if (closeReason === 'current-page') focusDestinationHeading();
		else focusNavigationControl();
	}
	async function select(event: MouseEvent, href: string) {
		if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
		event.preventDefault();
		if (!open || dismissing || closeReason === 'navigation') return;
		const from = window.location.href;
		if (new URL(href, from).href === from) {
			closeReason = 'current-page';
			setMenuOpen(false);
			return;
		}

		closeReason = 'navigation';
		try {
			await goto(href, { replaceState: true });
			await tick();
			focusDestinationHeading();
		} catch {
			// SvelteKit owns route errors. Restore the trigger only if this page stayed put.
			closeReason = 'dismiss';
			if (window.location.href === from && trigger?.isConnected) {
				if (open) setMenuOpen(false);
				else focusNavigationControl();
			}
		}
	}
</script>

<svelte:window onpopstate={handlePopState} />

<Sheet.Root bind:open={() => open, setMenuOpen}>
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
