<!--
  @component
  Organization data can't be reached (roles-ux §1): "Can't reach organization data. Showing the last copy from
  <time>." Changes wait until it is reachable; Try again reads it again. One quiet callout below the page header.
-->
<script lang="ts">
	import Callout from '$lib/components/Callout.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatClock, formatDate, formatFull } from '$lib/modules/preferences/time';
	import { staleText } from '../../present';
	import type { StaleCopy } from '../../types';

	let {
		stale,
		retrying = false,
		onretry
	}: { stale: StaleCopy; retrying?: boolean; onretry: () => void } = $props();

	const at = $derived(new Date(stale.copiedAt));
	const shown = $derived(
		`${formatDate(at, accountZone.zone, new Date())}, ${formatClock(at, accountZone.zone)}`
	);
</script>

<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
	<Callout data-testid="stale">
		<p class="text-sm/6 font-semibold text-white">
			{staleText(shown)}<span class="sr-only"> ({formatFull(at, accountZone.zone)})</span>
		</p>
		<p class="mt-1 text-sm/6">Changes can be made again once it's reachable.</p>
		<Button
			variant="outline"
			class="mt-3 min-w-24 {mobileButton}"
			disabled={retrying}
			aria-busy={retrying}
			onclick={onretry}>{retrying ? 'Trying again…' : 'Try again'}</Button
		>
	</Callout>
</div>
