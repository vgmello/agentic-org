<script lang="ts">
	import Logo from '$lib/components/Logo.svelte';

	let { children } = $props();
</script>

<!-- Full-bleed window with the native overlay title bar: the top strip drags the window and
     clears the traffic lights. -->
<div class="fixed inset-0 flex flex-col overflow-y-auto bg-gray-800">
	<div data-tauri-drag-region class="h-12 shrink-0"></div>
	<main class="mx-auto flex w-full max-w-2xl flex-1 flex-col justify-center px-6 pb-16">
		<div class="mb-10 flex items-center gap-x-3">
			<Logo class="h-9 w-auto" />
			<span class="text-lg/7 font-semibold text-white">Agentic Org</span>
		</div>
		{@render children()}
	</main>
</div>
