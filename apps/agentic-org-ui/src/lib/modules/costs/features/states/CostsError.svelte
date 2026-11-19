<!--
  @component
  A cost read that failed, inline where its figures would be (costs-and-insights-ux §1 States: "Couldn't load costs.
  Try again." with **Try again**). The same shape as the shared ErrorState, with the cost spec's words on its button.
  `role="alert"` announces it once. A server that doesn't serve costs at all shows the unavailable state instead.
-->
<script lang="ts">
	import { Button } from '$lib/components/ui/button';

	let {
		title,
		message,
		onretry,
		retrying = false,
		level = 4
	}: {
		/** "Couldn't load costs". */
		title: string;
		/** Why, when known. */
		message?: string;
		onretry: () => void;
		retrying?: boolean;
		level?: 3 | 4;
	} = $props();
</script>

<div role="alert" class="px-4 min-[640px]:px-6 min-[1024px]:px-8">
	<div class="max-w-xl rounded-md border border-line bg-tint/[0.03] p-4">
		<svelte:element this={`h${level}`} class="text-sm/6 font-semibold text-danger-text"
			>{title}. Try again.</svelte:element
		>
		{#if message}<p class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
				{message}
			</p>{/if}
		<Button
			variant="outline"
			class="mt-4 min-w-24 max-w-full max-[639px]:min-h-11"
			disabled={retrying}
			aria-busy={retrying}
			onclick={onretry}>{retrying ? 'Trying again…' : 'Try again'}</Button
		>
	</div>
</div>
