<script lang="ts">
	let { current }: { current: 'agents' | 'org-chart' } = $props();
	const views = [
		{ id: 'agents', label: 'Agents', href: '/agents' },
		{ id: 'org-chart', label: 'Org chart', href: '/org-chart' }
	] as const;
</script>

<nav aria-label="Agents views" class="flex gap-x-6 overflow-x-auto border-b border-white/10 px-4 sm:px-6 lg:px-8">
	{#each views as view (view.id)}
		{@const selected = current === view.id}
		<a href={view.href} aria-current={selected ? 'page' : undefined} class="-mb-px border-b-2 py-4 text-sm/6 font-semibold whitespace-nowrap focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 {selected ? 'border-indigo-400 text-white' : 'border-transparent text-gray-400 hover:border-white/20 hover:text-white'}">{view.label}</a>
	{/each}
</nav>
