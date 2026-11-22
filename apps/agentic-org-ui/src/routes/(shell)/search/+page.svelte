<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import SearchIcon from '@lucide/svelte/icons/search';
	import X from '@lucide/svelte/icons/x';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { Button } from '$lib/components/ui/button';
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
	let draft = $derived(query);
	let inputEl = $state<HTMLInputElement | null>(null);
	let phase = $state<'idle' | 'loading' | 'ready' | 'error'>('idle');
	let results = $state.raw<SearchResults | undefined>();
	let failure = $state<string | undefined>();
	let request = 0;

	async function run() {
		const asked = query;
		if (!asked) {
			++request;
			failure = undefined;
			phase = 'idle';
			results = undefined;
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

	function submit(event: SubmitEvent) {
		event.preventDefault();
		const trimmed = draft.trim();
		const target = trimmed ? `/search?q=${encodeURIComponent(trimmed)}` : '/search';
		void goto(resolve(target as '/'), { keepFocus: true });
	}

	function clear() {
		draft = '';
		if (query) void goto(resolve('/search'), { keepFocus: true });
		inputEl?.focus();
	}

	const total = $derived(results ? GROUPS.reduce((n, g) => n + results![g.key].length, 0) : 0);
</script>

<svelte:head><title>{pageTitle(query ? `“${query}”` : undefined, 'Search')}</title></svelte:head>

<PageHeader
	title={query ? `Results for “${query}”` : 'Search'}
	count={phase === 'ready' ? total : undefined}
	countLabel="results"
	description={query
		? undefined
		: 'Search projects, work items, and members across your organization.'}
/>

<div class="border-b border-line-subtle px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
	<form onsubmit={submit} class="flex max-w-2xl flex-wrap items-center gap-2.5" role="search">
		<label for="search-page-query" class="sr-only">Search organization</label>
		<div
			class="exec-well flex min-h-9 min-w-0 flex-1 basis-56 items-center gap-2 px-3 focus-within:outline-2 focus-within:-outline-offset-2 focus-within:outline-focus max-[639px]:min-h-11"
		>
			<SearchIcon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
			<input
				bind:this={inputEl}
				id="search-page-query"
				type="search"
				bind:value={draft}
				placeholder="Search projects, work references (T-14), or members…"
				autocomplete="off"
				class="min-w-0 flex-1 bg-transparent py-1.5 text-sm/6 text-fg placeholder:text-fg-muted focus:outline-none [&::-webkit-search-cancel-button]:hidden"
			/>
			{#if draft}
				<button
					type="button"
					onclick={clear}
					aria-label="Clear search"
					class="inline-flex size-6 items-center justify-center rounded-md text-fg-muted hover:bg-surface-overlay hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
				>
					<X class="size-3.5" aria-hidden="true" />
				</button>
			{/if}
		</div>
		<Button type="submit" class="min-h-9 max-[639px]:min-h-11">Search</Button>
	</form>
</div>

{#if phase === 'idle'}
	<EmptyState
		title="Nothing to search for yet"
		description="Enter a project name, a work reference such as T-14, or a person's name above."
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
	<div class="flex flex-col gap-6 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
		{#each GROUPS as group (group.key)}
			{@const hits = results[group.key]}
			{#if hits.length > 0}
				<section aria-labelledby="search-{group.key}" class="max-w-4xl">
					<h3
						id="search-{group.key}"
						class="flex items-center gap-2 text-xs/5 font-semibold tracking-wider text-fg-subtle uppercase"
					>
						{group.label}
						<span
							class="rounded-full bg-surface-overlay px-2 py-0.5 text-xs/4 font-medium tracking-normal text-fg-secondary tabular-nums"
							>{hits.length}</span
						>
					</h3>
					<ul role="list" class="exec-card mt-2.5 divide-y divide-line-subtle overflow-hidden">
						{#each hits as hit (hit.id)}
							<li
								class="relative flex items-center justify-between gap-4 px-4 py-3.5 transition-colors duration-150 hover:bg-surface-raised/70 motion-reduce:transition-none sm:px-5"
							>
								<div class="min-w-0 flex-1">
									<a
										href={resolve(hit.href as '/')}
										class="text-sm/6 font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
										><span class="absolute inset-0" aria-hidden="true"></span>{hit.title}</a
									>
									{#if hit.meta}<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
											{hit.meta}
										</p>{/if}
								</div>
								<ChevronRight class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
							</li>
						{/each}
					</ul>
				</section>
			{/if}
		{/each}
	</div>
{/if}
