<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { tick } from 'svelte';
	import { innerWidth } from 'svelte/reactivity/window';
	import MenuIcon from '@lucide/svelte/icons/menu';
	import type { Connection, ProjectSummary } from '$lib/proto';
	import ExecutiveSidebar from './ExecutiveSidebar.svelte';
	import * as Sheet from '$lib/components/ui/sheet';
	import { registerModalBack } from './modal-back';
	import { focusDestinationHeading } from './focus-destination';

	let {
		connection,
		needsYouCount = 0,
		projects = [],
		onWiden
	}: {
		connection: Connection;
		needsYouCount?: number;
		projects?: ProjectSummary[];
		onWiden: () => void;
	} = $props();
	const menuId = $props.id();
	let open = $state(false);
	let content = $state<HTMLDivElement | null>(null);
	let trigger = $state<HTMLButtonElement | null>(null);
	let closingForNavigation = false;

	// A viewport change must release the modal before the desktop navigation takes over.
	$effect(() => {
		if ((innerWidth.current ?? 0) >= 640 && open) open = false;
	});

	$effect.pre(() => {
		if (open)
			return registerModalBack(() => {
				open = false;
			});
	});

	function menuOpenChanged(value: boolean) {
		if (!value) return;
		closingForNavigation = false;
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
		if (
			event.defaultPrevented ||
			event.button !== 0 ||
			event.metaKey ||
			event.ctrlKey ||
			event.shiftKey ||
			event.altKey
		)
			return;
		event.preventDefault();
		if (!open) return;
		const from = window.location.href;
		closeForNavigation();
		try {
			if (new URL(href, from).href !== from) await goto(resolve(href as '/'));
			await focusDestinationHeading();
		} catch {
			if (window.location.href === from && trigger?.isConnected && !open) focusNavigationControl();
		}
	}
</script>

<Sheet.Root bind:open onOpenChange={menuOpenChanged}>
	<Sheet.Trigger
		bind:ref={trigger}
		aria-controls={menuId}
		class="-ml-2 inline-flex size-11 shrink-0 items-center justify-center rounded-md text-fg hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus min-[640px]:hidden"
	>
		<MenuIcon class="size-6" aria-hidden="true" strokeWidth={1.5} /><span class="sr-only">Menu</span
		>
	</Sheet.Trigger>
	<Sheet.Content
		id={menuId}
		bind:ref={content}
		side="left"
		onOpenAutoFocus={focusCurrent}
		onCloseAutoFocus={restoreFocus}
		class="gap-0 border-0 bg-surface p-0 data-[side=left]:w-72 data-[side=left]:max-w-[calc(100vw-56px)] [&_[data-slot=sheet-close]]:size-11"
	>
		<Sheet.Title class="sr-only">Main navigation</Sheet.Title>
		<Sheet.Description class="sr-only">Choose an area of the organization.</Sheet.Description>
		<ExecutiveSidebar {connection} {needsYouCount} {projects} onNavigate={select} />
	</Sheet.Content>
</Sheet.Root>
