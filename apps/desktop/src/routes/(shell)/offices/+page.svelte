<script lang="ts">
	import Plus from '@lucide/svelte/icons/plus';
	import OfficeDetailsSheet from '$lib/components/OfficeDetailsSheet.svelte';
	import OfficeStatusBadge from '$lib/components/OfficeStatusBadge.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { agentsIn, isHq, orderOffices, secondaryText, unplacedAgents } from '$lib/offices/model';

	let { data } = $props();

	const offices = $derived(orderOffices(data.offices));
	const here = $derived(data.connection.office);
	const unplaced = $derived(unplacedAgents(data.snapshot, data.offices));
	const organization = $derived(data.connection.organization.name);

	let detailsId = $state<string | undefined>();
	// The open sheet shows the latest data for its office, not the copy captured when it was clicked.
	const detailsFor = $derived(data.offices.find((o) => o.id === detailsId));
	let detailsOpen = $state(false);
	$effect(() => {
		if (detailsOpen && !detailsFor) detailsOpen = false; // the office went away: close its sheet
	});

	function open(id: string) {
		detailsId = id;
		detailsOpen = true;
	}
</script>

<svelte:head><title>Offices · Agentic Org</title></svelte:head>

<div class="space-y-5 px-8 py-6">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<div class="max-w-xl space-y-2">
			<p class="text-sm/6 text-gray-400">
				Where {organization}'s agents run. HQ keeps the records and approves new offices; every other office keeps a copy.
			</p>
			{#if data.sample}
				<!-- TEMPORARY: agent counts and placement come from the invented organization until the real snapshot is wired in. -->
				<p class="flex items-center gap-x-2 text-xs/5 text-gray-400" data-testid="sample-data-note">
					<Badge variant="secondary">Sample data</Badge> The agents below are invented, not yours.
				</p>
			{/if}
		</div>
		<div class="text-right">
			<!-- Phase 1 has HQ only; more offices arrive with the local satellite. -->
			<Button disabled aria-describedby="add-reason"><Plus aria-hidden="true" /> Add office</Button>
			<p id="add-reason" class="mt-1 text-xs/5 text-gray-400">Available in a later release.</p>
		</div>
	</div>

	<ul class="divide-y divide-white/5 overflow-hidden rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Offices">
		{#each offices as office (office.id)}
			{@const agents = agentsIn(data.snapshot, office.id).length}
			<li data-testid="office-row" data-office={office.id} class="flex items-center gap-x-4 px-4 py-3">
				<span class="flex h-6 w-10 shrink-0 items-center justify-center rounded text-xs font-semibold {isHq(office) ? 'bg-indigo-500/20 text-indigo-200' : 'text-transparent'}" aria-hidden={!isHq(office)}>
					{isHq(office) ? 'HQ' : ''}
				</span>
				<button type="button" class="min-w-0 flex-1 text-left focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => open(office.id)}>
					<span class="block truncate text-sm/6 font-semibold text-white" title={office.name}>{office.name}</span>
					<span class="block truncate text-xs/5 text-gray-400">{secondaryText(office) + (here.id === office.id ? " (you're here)" : '')}</span>
				</button>
				<OfficeStatusBadge status={office.status} />
				<span class="w-20 text-right text-xs/5 text-gray-400">{agents} {agents === 1 ? 'agent' : 'agents'}</span>
			</li>
		{:else}
			<li class="px-4 py-3 text-sm/6 text-gray-400" data-testid="no-offices">No offices to show.</li>
		{/each}
	</ul>

	{#if unplaced.length > 0}
		<p class="rounded-lg bg-amber-500/10 px-4 py-3 text-sm/6 [overflow-wrap:anywhere] text-amber-100 outline-1 -outline-offset-1 outline-amber-400/20" data-testid="unplaced-note">
			{unplaced.length} {unplaced.length === 1 ? 'agent is' : 'agents are'} not placed in an office: {unplaced.map((a) => a.name).join(', ')}.
		</p>
	{/if}
	<p class="text-xs/5 text-gray-500">Shown from this Mac.</p>
</div>

<OfficeDetailsSheet bind:open={detailsOpen} office={detailsFor} snapshot={data.snapshot} sample={data.sample} here={detailsFor?.id === here.id} />
