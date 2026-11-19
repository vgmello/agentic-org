<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { searchApi } from '$lib/modules/search/api';
	import type { SearchGroup, SearchResults } from '$lib/modules/shell/types';
	import { pageTitle } from '$lib/nav';

	let { data } = $props();
	const GROUPS: { key: SearchGroup; label: string }[] = [
		{ key: 'projects', label: 'Projects' },
		{ key: 'work', label: 'Work' },
		{ key: 'members', label: 'Members' }
	];
	const query = $derived((page.url.searchParams.get('q') ?? '').trim());
	let phase = $state<'idle' | 'loading' | 'ready' | 'error'>('idle');
	let results = $state.raw<SearchResults | undefined>();
	let failure = $state<string | undefined>();
	let request = 0;

	async function run() {
		const asked = query;
		if (!asked) {
			phase = 'idle';
			return;
		}
		const mine = ++request;
		phase = 'loading';
		try {
			const found = await searchApi.results(data.connection.organization.id, asked);
			if (mine !== request) return;
			results = found;
			phase = 'ready';
		} catch (e) {
			if (mine !== request) return;
			failure = e instanceof Error ? e.message : undefined;
			phase = 'error';
		}
	}
	$effect(() => {
		void query;
		void run();
	});
	const total = $derived(results ? GROUPS.reduce((n, g) => n + results![g.key].length, 0) : 0);
</script>

<svelte:head><title>{pageTitle(query ? `“${query}”` : undefined, 'Search')}</title></svelte:head>

<PageHeader
	title={query ? `Results for “${query}”` : 'Search'}
	count={phase === 'ready' ? total : undefined}
	countLabel="results"
	description={query
		? undefined
		: 'Search projects, work and members from the field at the top of the page.'}
/>
{#if phase === 'idle'}
	<EmptyState
		title="Nothing to search for yet"
		description="Type in the Search field at the top, then choose See all results."
	/>
{:else if phase === 'loading'}
	<SkeletonRows label="results" rows={5} />
{:else if phase === 'error'}
	<ErrorState
		title="Couldn't search"
		message={failure ?? 'The office did not answer. Try again.'}
		onretry={run}
	/>
{:else if results && total === 0}
	<EmptyState
		title="No results for “{query}”"
		description="Check the spelling, or try a project name, a work reference such as T-14, or a person's name."
	/>
{:else if results}
	<p class="sr-only" role="status">{total} {total === 1 ? 'result' : 'results'}</p>
	{#each GROUPS as group (group.key)}
		{@const hits = results[group.key]}
		{#if hits.length > 0}
			<section aria-labelledby="search-{group.key}" class="pt-6">
				<h3
					id="search-{group.key}"
					class="px-4 text-base/7 font-semibold text-white min-[640px]:px-6 min-[1024px]:px-8"
				>
					{group.label} <span class="text-sm/6 font-normal text-gray-400">{hits.length}</span>
				</h3>
				<ul role="list" class="mt-3 divide-y divide-white/5 border-y border-white/5">
					{#each hits as hit (hit.id)}
						<li class="relative px-4 py-4 hover:bg-white/[0.03] min-[640px]:px-6 min-[1024px]:px-8">
							<a
								href={resolve(hit.href as '/')}
								class="text-sm/6 font-semibold text-white focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]"
								><span class="absolute inset-0" aria-hidden="true"></span>{hit.title}</a
							>
							{#if hit.meta}<p class="text-xs/5 text-gray-400 [overflow-wrap:anywhere]">
									{hit.meta}
								</p>{/if}
						</li>
					{/each}
				</ul>
			</section>
		{/if}
	{/each}
{/if}
