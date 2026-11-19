<!--
  @component
  An absolute time in the viewing account's zone, "16 Nov, 14:02", with the full form for assistive technology
  (app-shell-ux "Account time display").
-->
<script lang="ts">
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatClock, formatDate, formatFull } from '$lib/modules/preferences/time';

	let { at }: { at: string } = $props();
	const date = $derived(new Date(at));
	const full = $derived(formatFull(date, accountZone.zone));
</script>

<time datetime={at} title={full}
	><span aria-hidden="true"
		>{formatDate(date, accountZone.zone, new Date())}, {formatClock(date, accountZone.zone)}</span
	><span class="sr-only">{full}</span></time
>
