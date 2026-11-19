<!--
  @component
  A quiet line for screens that list event times when no valid time zone is known: times then show in unsaved
  UTC, and this says how to fix it (app-shell-ux "Account time display"). Renders nothing otherwise.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { accountZone } from './account-zone.svelte';
</script>

{#if accountZone.status === 'unsaved-utc'}
	<p
		class="px-4 pt-4 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
		data-testid="zone-notice"
	>
		Times show in unsaved UTC. Time zone not detected: set it in
		<a
			href={resolve('/preferences')}
			class="rounded-sm text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
			>Your preferences</a
		>.
	</p>
{/if}
