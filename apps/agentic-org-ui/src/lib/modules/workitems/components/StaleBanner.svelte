<!--
  @component
  Stale read-only notice when the accepting office or HQ can't be reached (work-items-ux §12, PRJ-4).
-->
<script lang="ts">
	import Callout from '$lib/components/Callout.svelte';
	import { Button } from '$lib/components/ui/button';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { when } from '../present';
	import type { StaleCopy } from '../types';

	let {
		stale,
		retrying = false,
		onretry
	}: {
		stale: StaleCopy;
		retrying?: boolean;
		onretry: () => void;
	} = $props();
</script>

<div class="{gutters} pt-4" data-testid="stale-banner">
	<Callout>
		<div class="flex flex-wrap items-center justify-between gap-3">
			<p class="text-sm/6">
				Can't reach the office. Showing the last copy from {when(stale.copiedAt)} (read-only).
			</p>
			<Button
				type="button"
				variant="outline"
				size="sm"
				class={mobileButton}
				disabled={retrying}
				onclick={onretry}>{retrying ? 'Retrying…' : 'Retry'}</Button
			>
		</div>
	</Callout>
</div>
