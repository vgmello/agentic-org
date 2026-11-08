<script lang="ts">
	import '../app.css';
	import { goto, invalidate } from '$app/navigation';
	import { onMount } from 'svelte';
	import { isAppPath } from '$lib/navigation';
	import { inTauri } from '$lib/office/api';

	let { children } = $props();

	// The menu bar asks the window to go somewhere (an organization was picked, "Create
	// organization…"): the Rust side sends a `navigate` event with one of a few known paths.
	onMount(() => {
		if (!inTauri()) return;
		let stop: (() => void) | undefined;
		let gone = false;
		import('@tauri-apps/api/event').then(async ({ listen }) => {
			const unlisten = await listen<string>('navigate', async (event) => {
				if (!isAppPath(event.payload)) return;
				await invalidate('app:connections');
				await goto(event.payload);
			});
			if (gone) unlisten();
			else stop = unlisten;
		});
		return () => {
			gone = true;
			stop?.();
		};
	});
</script>

{@render children()}
