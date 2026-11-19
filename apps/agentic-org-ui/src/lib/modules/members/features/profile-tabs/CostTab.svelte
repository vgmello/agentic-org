<!--
  @component
  Profile › Cost (COST-7): built with the cost screens (plan step 7). Until then, the honest placeholder.
-->
<script lang="ts">
	import EmptyState from '$lib/components/EmptyState.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import { profileContext } from '../profile/context.svelte';

	const ctx = profileContext();
</script>

{#if ctx.profile.viewer.costVisible}
	<NotBuilt title="Cost" place="Cost · {ctx.profile.name}" area="Teams" />
{:else}
	<EmptyState
		level={3}
		title="You don't see costs here"
		description="Costs show to the owner, and to project managers for their projects."
	/>
{/if}
