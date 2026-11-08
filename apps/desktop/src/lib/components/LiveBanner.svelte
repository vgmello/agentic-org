<script lang="ts">
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import { Button } from '$lib/components/ui/button';

	let {
		loadedAt,
		retrying = false,
		onretry
	}: {
		/** When the screens last loaded the organization (undefined: they have not). */
		loadedAt: Date | undefined;
		retrying?: boolean;
		onretry: () => void;
	} = $props();

	const time = $derived(loadedAt?.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }));
</script>

<!-- Live updates are off: say so, and say how old what is shown is, instead of looking current. -->
<div role="status" data-testid="live-paused" class="flex items-center gap-x-3 border-b border-amber-400/20 bg-amber-400/10 px-8 py-2 text-sm/6 text-amber-200">
	<CircleAlert class="size-4 shrink-0" aria-hidden="true" />
	<p class="min-w-0 flex-1">Live updates paused.{#if time} Showing what was loaded at {time}.{/if}</p>
	<Button size="sm" variant="outline" disabled={retrying} onclick={onretry}>{retrying ? 'Retrying…' : 'Retry'}</Button>
</div>
