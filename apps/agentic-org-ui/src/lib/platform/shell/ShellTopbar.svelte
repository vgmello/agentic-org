<!--
  @component
  The sticky top bar (app-shell-ux §0.2, §1.3, §6a): the drawer's Menu button below 640 px, the page's `h1`
  (the area name; `main` is named by it), Search and the outline New menu. It owns the single-key shortcuts:
  N / Shift+N opens New and / focuses Search, outside editable text, while the device setting allows them.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { Connection } from '$lib/proto';
	import NewMenu from './NewMenu.svelte';
	import ShellSearch from './ShellSearch.svelte';
	import { isEditableTarget, isPlainKey, singleKeyShortcutsOn } from './shortcuts';

	let {
		heading,
		connection,
		children
	}: { heading?: string; connection: Connection; children: Snippet } = $props();

	let search = $state<ReturnType<typeof ShellSearch> | undefined>();
	let newMenu = $state<ReturnType<typeof NewMenu> | undefined>();
	const shortcuts = $derived(singleKeyShortcutsOn());

	function keydown(event: KeyboardEvent) {
		if (
			!shortcuts ||
			event.defaultPrevented ||
			!isPlainKey(event) ||
			isEditableTarget(event.target)
		)
			return;
		// One overlay at a time: a shortcut never opens over a dialog, sheet, drawer or open menu.
		if (document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"]')) return;
		if (event.key === 'n' || event.key === 'N') {
			event.preventDefault();
			newMenu?.openMenu();
		} else if (event.key === '/') {
			event.preventDefault();
			void search?.focusSearch();
		}
	}
</script>

<svelte:window onkeydown={keydown} />

<div
	data-shell-header
	class="sticky top-0 z-40 flex h-14 shrink-0 items-center gap-x-3 border-b border-white/5 bg-gray-800 px-4 min-[640px]:h-16 min-[640px]:gap-x-6 min-[640px]:px-6 min-[1024px]:px-8"
>
	{@render children()}
	<h1
		id="shell-page-title"
		tabindex="-1"
		class="min-w-0 flex-1 truncate text-base/7 font-semibold text-white focus:outline-none min-[640px]:flex-none min-[640px]:shrink"
	>
		{heading}
	</h1>
	<div class="flex min-w-0 items-center justify-end gap-x-2 min-[640px]:flex-1 min-[640px]:gap-x-4">
		<ShellSearch
			bind:this={search}
			organizationId={connection.organization.id}
			shortcutHint={shortcuts}
		/>
		<NewMenu bind:this={newMenu} {connection} shortcutHint={shortcuts} />
	</div>
</div>
