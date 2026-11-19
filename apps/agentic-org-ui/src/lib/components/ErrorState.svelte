<!--
  @component
  A blocking load failure, inline where the content would be (app-shell-ux §6a): what failed, why when
  known, and what to do next, with Retry. Never a raw stack trace. `role="alert"` announces it once.
  For a service this release does not serve, show `NotBuilt` / the unavailable page instead.
  Usage: <ErrorState title="Couldn't load the members" message={error.message} onretry={load} retrying={busy} />
-->
<script lang="ts">
	import { Button } from '$lib/components/ui/button';

	let {
		title,
		message,
		onretry,
		retrying = false,
		level = 2,
		class: className = ''
	}: {
		title: string;
		message?: string;
		onretry?: () => void;
		retrying?: boolean;
		/** The heading level: 2 for a page, 3 for a section, 4 for a tab under its own h3. */
		level?: 2 | 3 | 4;
		class?: string;
	} = $props();
</script>

<div role="alert" class="px-4 py-12 min-[640px]:px-6 min-[1024px]:px-8 {className}">
	<div class="max-w-xl rounded-md border border-white/10 bg-white/[0.03] p-4">
		<svelte:element this={`h${level}`} class="text-sm/6 font-semibold text-red-300"
			>{title}</svelte:element
		>
		{#if message}<p class="mt-1 text-sm/6 text-gray-300 [overflow-wrap:anywhere]">{message}</p>{/if}
		{#if onretry}
			<Button
				variant="outline"
				class="mt-4 min-w-20 max-[639px]:h-11"
				disabled={retrying}
				aria-busy={retrying}
				onclick={onretry}>{retrying ? 'Retrying…' : 'Retry'}</Button
			>
		{/if}
	</div>
</div>
