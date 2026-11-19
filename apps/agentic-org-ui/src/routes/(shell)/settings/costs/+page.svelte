<script lang="ts">
	// Organization settings › Costs: the route composes the costs module with the viewer's time zone and the member
	// profile's Cost tab, where an agent's budgets are set; the module itself depends on neither.
	import { pageTitle } from '$lib/nav';
	import { CostsSettingsPage } from '$lib/modules/costs';
	import { tabHref } from '$lib/modules/members';
	import { accountZone } from '$lib/modules/preferences';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	$effect(() => void accountZone.ensure(organizationId));
</script>

<svelte:head><title>{pageTitle('Costs', 'Organization settings')}</title></svelte:head>

{#if accountZone.loaded}
	{#key `${organizationId}|${accountZone.zone}`}<CostsSettingsPage
			{organizationId}
			zone={accountZone.zone}
			memberHref={(memberId) => tabHref(memberId, 'cost')}
		/>{/key}
{/if}
