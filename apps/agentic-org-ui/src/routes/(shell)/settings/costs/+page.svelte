<script lang="ts">
	// Organization settings › Costs: the route composes the costs module with the viewer's time zone and the member
	// profile's Cost tab, where an agent's budgets are set; the module itself depends on neither.
	import { pageTitle } from '$lib/nav';
	import CostsSettingsPage from '$lib/modules/costs/features/settings-page/CostsSettingsPage.svelte';
	import { tabHref } from '$lib/modules/members';
	import { owner } from '$lib/modules/organization/model';
	import { accountZone } from '$lib/modules/preferences';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { useShellData } from '$lib/platform/workspace/shell-data-context';

	let { data } = $props();
	const shellData = useShellData();
	const organizationId = $derived(data.connection.organization.id);
	const viewerIsOwner = $derived.by(() => {
		if (appSession.state.kind !== 'signed-in') return undefined;
		const ownerId = shellData.snapshot ? owner(shellData.snapshot)?.id : undefined;
		if (!ownerId) return undefined;
		const link = appSession.state.session.organizations.find((o) => o.org_id === organizationId);
		return link?.employee_id === ownerId;
	});
	$effect(() => void accountZone.ensure(organizationId));
</script>

<svelte:head><title>{pageTitle('Costs', 'Organization settings')}</title></svelte:head>

{#if accountZone.loaded}
	{#key `${organizationId}|${accountZone.zone}`}<CostsSettingsPage
			{organizationId}
			zone={accountZone.zone}
			{viewerIsOwner}
			memberHref={(memberId) => tabHref(memberId, 'cost')}
		/>{/key}
{/if}
