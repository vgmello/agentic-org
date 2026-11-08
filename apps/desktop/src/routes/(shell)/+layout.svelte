<script lang="ts">
	import Sidebar from '$lib/components/Sidebar.svelte';
	import { Toaster } from '$lib/components/ui/sonner';

	import { onMount } from 'svelte';
	import { invalidate } from '$app/navigation';
	import { page } from '$app/state';
	import { orgApi } from '$lib/org/api';
	import { noteChanged } from '$lib/org/live';
	import { navFor } from '$lib/nav';

	let { data, children } = $props();

	// The office says when the organization changes (someone else's command, or ours): the screens load it again,
	// unless they already show it.
	onMount(() => {
		let stop: (() => void) | undefined;
		let leaving = false;
		orgApi
			.onChanged((notice) => {
				if (notice.organization_id === data.connection.organization.id) noteChanged(notice, () => invalidate('app:org'));
			})
			.then((unlisten) => {
				if (leaving) unlisten();
				else stop = unlisten;
			})
			.catch(() => {
				// No live changes then: a screen still loads the latest each time it opens.
			});
		return () => {
			leaving = true;
			stop?.();
		};
	});
</script>

<!-- The app fills the window edge to edge. -->
<div class="fixed inset-0 overflow-hidden bg-gray-800">
	<div class="fixed inset-y-0 z-50 flex w-72 flex-col">
		<Sidebar connection={data.connection} />
	</div>
	<div class="h-full overflow-y-auto pl-72">
		<!-- The header strip drags the window; nothing interactive sits in it yet. -->
		<div data-tauri-drag-region class="sticky top-0 z-40 flex h-16 shrink-0 items-center border-b border-white/5 bg-gray-800 px-8">
			<h1 data-tauri-drag-region class="text-base/7 font-semibold text-white">{navFor(page.url.pathname)?.name}</h1>
		</div>
		{@render children()}
	</div>
</div>
<Toaster position="bottom-right" />
