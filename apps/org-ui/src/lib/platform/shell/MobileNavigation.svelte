<script lang="ts">
	import { goto } from '$app/navigation';
	import { tick } from 'svelte';
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
	let closeReason: 'dismiss' | 'navigate' | 'resize' = 'dismiss';

	// A viewport change must release the modal before the desktop navigation takes over.
	$effect(() => {
		if ((innerWidth.current ?? 0) >= 640 && open) {
			closeReason = 'resize';
			open = false;
		}
	});

	async function focusCurrent(event: Event) {
		event.preventDefault();
		await tick();
		if (open) content?.querySelector<HTMLElement>('nav [aria-current="page"]')?.focus();
	}
	async function restoreFocus(event: Event) {
		if (closeReason === 'navigate') event.preventDefault();
		if (closeReason !== 'resize') return;
		event.preventDefault();
		await tick();
		if ((innerWidth.current ?? 0) >= 640) onWiden();
		else trigger?.focus();
	}
	async function select(event: MouseEvent, href: string) {
		if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
		event.preventDefault();
		if (!open) return;
		const from = window.location.href;
		closeReason = 'navigate';
		open = false;
		try {
			await goto(href);
			await tick();
			const heading = document.querySelector<HTMLElement>('main h1');
			if (heading) {
				heading.tabIndex = -1;
				heading.focus({ preventScroll: true });
			}
		} catch {
			// SvelteKit owns route errors. Restore the trigger only if this page stayed put.
			if (window.location.href === from && trigger?.isConnected) trigger.focus();
		}
	}
</script>

<Sheet.Root bind:open onOpenChange={(value) => { if (value) closeReason = 'dismiss'; }}>
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
