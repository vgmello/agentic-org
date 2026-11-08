<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import OrgChart from '$lib/components/OrgChart.svelte';
	import { Button } from '$lib/components/ui/button';
	import { SAMPLE_ORG, flatten, parentIds } from '$lib/org-chart/data';

	const collapsed = new SvelteSet<string>();
	let fit = $state(true);
	const agents = flatten(SAMPLE_ORG).filter((n) => n.status);
	const working = agents.filter((n) => n.status === 'working').length;
	const allParents = parentIds(SAMPLE_ORG).filter((id) => id !== SAMPLE_ORG.id);
</script>

<svelte:head><title>Org chart · agentic.org</title></svelte:head>

<div class="flex flex-wrap items-center justify-between gap-4 px-8 pt-6">
	<p class="text-sm/6 text-gray-400">
		<span class="font-semibold text-white">{agents.length}</span> agents ·
		<span class="font-semibold text-white">{working}</span> working now · sample data
	</p>
	<div class="flex items-center gap-x-2">
		<Button variant="outline" size="sm" aria-pressed={fit} onclick={() => (fit = !fit)}>
			{fit ? 'Actual size' : 'Fit to width'}
		</Button>
		<Button variant="outline" size="sm" onclick={() => collapsed.clear()}>Expand all</Button>
		<Button variant="outline" size="sm" onclick={() => allParents.forEach((id) => collapsed.add(id))}>
			Collapse all
		</Button>
	</div>
</div>

<div class="overflow-x-auto px-8 py-10">
	<OrgChart root={SAMPLE_ORG} {collapsed} {fit} />
</div>
