<script lang="ts">
	import Logo from '$lib/components/Logo.svelte';
	import { Toaster } from '$lib/components/ui/sonner';

	let { children } = $props();
</script>

<!-- Browser onboarding layout. -->
<div class="fixed inset-0 flex flex-col overflow-y-auto bg-surface">
	<div class="h-12 shrink-0"></div>
	<main class="mx-auto flex w-full max-w-2xl flex-1 flex-col justify-center px-6 pb-16">
		<div class="mb-10 flex items-center gap-x-3">
			<Logo class="h-9 w-auto" />
			<span class="text-lg/7 font-semibold text-fg">Agentic Org</span>
		</div>
		{@render children()}
	</main>
</div>
<Toaster position="bottom-right" />
