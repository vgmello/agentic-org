<script lang="ts">
	import Bug from '@lucide/svelte/icons/bug';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import { buttonVariants } from '$lib/components/ui/button';
	import { Toaster } from '$lib/components/ui/sonner';

	import { page } from '$app/state';
	import { titleFor } from '$lib/nav';

	let { data, children } = $props();
</script>

<!-- The app fills the window edge to edge. -->
<div class="fixed inset-0 overflow-hidden bg-gray-800">
	<div class="fixed inset-y-0 z-50 flex w-72 flex-col">
		<Sidebar connection={data.connection} />
	</div>
	<div class="h-full overflow-y-auto pl-72">
		<!-- The header strip drags the window; only the report link sits in it. -->
		<div data-tauri-drag-region class="sticky top-0 z-40 flex h-16 shrink-0 items-center justify-between border-b border-white/5 bg-gray-800 px-8">
			<h1 data-tauri-drag-region class="text-base/7 font-semibold text-white">{titleFor(page.url.pathname)}</h1>
			{#if page.url.pathname !== '/report-bug'}
				<a href="/report-bug" class={buttonVariants({ variant: 'ghost', size: 'sm' })}><Bug aria-hidden="true" /> Report a bug</a>
			{/if}
		</div>
		{@render children()}
	</div>
</div>
<Toaster position="bottom-right" />
