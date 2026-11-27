<!--
  @component
  Profile › Cost (costs-and-insights-ux §1 Agent profile › Cost, COST-7, §4):
  shows the agent's spend by period, project and top tasks in the viewer's time zone, and (for the owner only)
  daily and monthly calendar budget bars and controls. Non-authorized viewers see no amounts.
-->
<script lang="ts">
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { MemberCostView } from '$lib/modules/costs/ui';
	import { accountZone } from '$lib/modules/preferences';
	import { profileContext } from '../profile/context.svelte';

	const ctx = profileContext();
	$effect(() => void accountZone.ensure(ctx.organizationId));
</script>

{#if !ctx.profile.viewer.costVisible}
	<EmptyState
		level={3}
		title="You don't see costs here"
		description="Costs show to the owner, and to project managers for their projects."
	/>
{:else if accountZone.loaded}
	{#key `${ctx.organizationId}|${ctx.profile.id}|${accountZone.zone}`}
		<MemberCostView
			organizationId={ctx.organizationId}
			memberId={ctx.profile.id}
			agentName={ctx.profile.name}
			zone={accountZone.zone}
		/>
	{/key}
{/if}
