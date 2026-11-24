<!--
  @component
  Organization Search & "Why is…" Knowledge search (`/search`, decisions-and-tech-debt-ux §5, WHY-3, WHY-4):
  - Searches permitted decisions, evaluations, designs, reports, briefs, and feature definitions
    alongside Projects, Work, and Members.
  - Detects "Why is…" queries (or explicit Knowledge filter) and highlights Decisions/Evaluations/Designs/Features
    with scope, status, date, and labelled body/evaluation excerpts.
  - Explicitly states indexed scope, freshness, partial/title-only indexing, and privacy exclusions.
  - Uses Resource + Loaded for clean loading, unavailable, unsupported (no Retry), error (with Retry), and ready states.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { gutters, inputClass, mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatDate } from '$lib/modules/preferences/time';
	import type { SearchGroup } from '$lib/modules/shell/types';
	import { pageTitle } from '$lib/nav';
	import { searchApi } from '../../api';
	import { WHY_EXAMPLES, coverageStatement, groupKnowledgeHits, isWhyQuery } from '../../present';
	import type { SearchPageResults } from '../../types';

	let { organizationId }: { organizationId: string } = $props();

	const uid = $props.id();

	const GROUPS: { key: SearchGroup; label: string }[] = [
		{ key: 'projects', label: 'Projects' },
		{ key: 'work', label: 'Work' },
		{ key: 'members', label: 'Members' }
	];

	const query = $derived((page.url.searchParams.get('q') ?? '').trim());
	const modeParam = $derived(
		page.url.searchParams.get('scope') === 'knowledge' ? 'knowledge' : 'all'
	);
	const whyMode = $derived(modeParam === 'knowledge' || isWhyQuery(query));

	let draftQuery = $derived(query);

	const searchRead = $derived(
		query ? new Resource<SearchPageResults>(() => searchApi.results(organizationId, query)) : null
	);

	const results = $derived(searchRead?.data);
	const knowledgeGroups = $derived(results ? groupKnowledgeHits(results.knowledge) : []);

	const total = $derived.by(() => {
		if (!results) return 0;
		const knowledgeCount = results.knowledge.length;
		if (modeParam === 'knowledge') return knowledgeCount;
		return knowledgeCount + GROUPS.reduce((n, g) => n + results[g.key].length, 0);
	});

	function formatWhen(iso: string): string {
		const date = new Date(iso);
		if (Number.isNaN(date.getTime())) return iso;
		return formatDate(date, accountZone.zone, new Date());
	}

	async function runSearch(nextQuery: string, scope: 'all' | 'knowledge' = modeParam) {
		const trimmed = nextQuery.trim();
		const parts: string[] = [];
		if (trimmed) parts.push(`q=${encodeURIComponent(trimmed)}`);
		if (scope === 'knowledge') parts.push('scope=knowledge');
		const qs = parts.join('&');
		await goto(resolve((qs ? `/search?${qs}` : '/search') as '/'));
	}

	function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		void runSearch(draftQuery);
	}

	function statusTone(status: string): 'ok' | 'info' | 'neutral' {
		const lower = status.toLowerCase();
		if (lower === 'current' || lower === 'live') return 'ok';
		if (lower === 'draft' || lower === 'in progress') return 'info';
		return 'neutral';
	}
</script>

<svelte:head><title>{pageTitle(query ? `“${query}”` : undefined, 'Search')}</title></svelte:head>

<PageHeader
	title={query ? `Results for “${query}”` : 'Search'}
	count={searchRead?.state === 'ready' ? total : undefined}
	countLabel="results"
	description={query
		? whyMode
			? 'Knowledge search across permitted decisions, evaluations, designs, reports and feature definitions.'
			: undefined
		: 'Search projects, work, members, or ask "Why is…" to search decisions, evaluations, designs and features.'}
/>

<!-- Interactive search bar & scope toggle -->
<div class="border-b border-line-subtle py-4 {gutters}">
	<form class="flex flex-wrap items-end gap-3" onsubmit={handleSubmit} role="search">
		<div class="min-w-56 flex-1 max-[639px]:basis-full">
			<Label for="{uid}-q" class="mb-1 block text-xs/5 font-medium text-fg-secondary">
				Search or ask “Why is…”
			</Label>
			<Input
				id="{uid}-q"
				type="search"
				placeholder="Why is SQLite used for office storage? · ADR-0002 · T-A40"
				bind:value={draftQuery}
				class={inputClass}
			/>
		</div>

		<div
			role="group"
			aria-label="Search scope"
			class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
		>
			<button
				type="button"
				aria-pressed={modeParam === 'all'}
				onclick={() => void runSearch(draftQuery || query, 'all')}
				class="rounded-sm px-3 py-1.5 text-xs/5 font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {modeParam ===
				'all'
					? 'bg-surface-overlay text-fg'
					: 'text-fg-muted hover:text-fg'}"
			>
				Everything
			</button>
			<button
				type="button"
				aria-pressed={modeParam === 'knowledge'}
				onclick={() => void runSearch(draftQuery || query, 'knowledge')}
				class="rounded-sm px-3 py-1.5 text-xs/5 font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {modeParam ===
				'knowledge'
					? 'bg-surface-overlay text-fg'
					: 'text-fg-muted hover:text-fg'}"
			>
				Knowledge (“Why…”)
			</button>
		</div>

		<Button type="submit" size="sm" class={mobileButton}>Search</Button>
	</form>

	<div class="mt-3 flex flex-wrap items-center gap-2 text-xs/5 text-fg-muted">
		<span>Try asking:</span>
		{#each WHY_EXAMPLES as example (example)}
			<button
				type="button"
				onclick={() => void runSearch(example, 'knowledge')}
				class="rounded-md bg-surface-raised px-2.5 py-1 text-left text-fg-secondary ring-1 ring-line hover:bg-surface-overlay hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
			>
				{example}
			</button>
		{/each}
	</div>
</div>

{#if !query || !searchRead}
	<EmptyState
		title="Nothing to search for yet"
		description="Ask a “Why is…” question to find decisions, evaluations, designs and features, or search for a project, work item, or person."
	/>
{:else}
	<Loaded
		resource={searchRead}
		label="search results"
		error="Couldn't search"
		errorLevel={3}
		rows={5}
	>
		{#snippet children(found)}
			<!-- Honest scope, freshness and exclusion notice (WHY-4) -->
			<p
				class="border-b border-line-subtle py-2.5 text-xs/5 text-fg-muted {gutters}"
				data-testid="search-coverage"
			>
				{coverageStatement(found.coverage)}
				{#if found.coverage.scope !== 'unavailable'}
					Indexed <time datetime={found.coverage.indexedAt}
						>{formatWhen(found.coverage.indexedAt)}</time
					>.
				{/if}
			</p>

			{#if total === 0}
				<EmptyState
					level={3}
					title="No results for “{query}”"
					description="Check the spelling, or try a decision key such as ADR-0002, a work reference such as T-A40, or a shorter term."
				/>
			{:else}
				<p class="sr-only" role="status">{total} {total === 1 ? 'result' : 'results'}</p>

				<!-- Knowledge & Why results (shown first in Why/Knowledge mode, or alongside standard groups) -->
				{#if knowledgeGroups.length > 0}
					<div data-testid="knowledge-results">
						{#each knowledgeGroups as group (group.recordType)}
							<section aria-labelledby="{uid}-know-{group.recordType}" class="pt-6">
								<h3
									id="{uid}-know-{group.recordType}"
									class="text-base/7 font-semibold text-fg {gutters}"
								>
									{group.label}
									<span class="text-sm/6 font-normal text-fg-muted">{group.hits.length}</span>
								</h3>
								<ul
									role="list"
									class="mt-3 divide-y divide-line-subtle border-y border-line-subtle"
								>
									{#each group.hits as hit (hit.id)}
										<li
											class="relative py-4 hover:bg-tint/[0.03] {gutters}"
											data-testid="knowledge-hit"
											data-record-type={hit.recordType}
										>
											<div class="flex flex-wrap items-baseline gap-x-2 gap-y-1">
												{#if hit.key}
													<span class="font-mono text-xs/5 font-medium text-fg-muted"
														>{hit.key}</span
													>
												{/if}
												<a
													href={resolve(hit.href as '/')}
													class="text-sm/6 font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
												>
													<span class="absolute inset-0" aria-hidden="true"></span>{hit.title}
												</a>
												<StatusPill label={hit.status} class={tone[statusTone(hit.status)]} />
											</div>
											<p class="mt-1 flex flex-wrap gap-x-3 text-xs/5 text-fg-muted">
												<span>{hit.kindLabel}</span>
												<span>{hit.scopeLabel}</span>
												<span>
													Updated <time datetime={hit.updatedAt}>{formatWhen(hit.updatedAt)}</time>
												</span>
											</p>
											{#if hit.excerpt}
												<p
													class="mt-1.5 text-xs/5 text-fg-body [overflow-wrap:anywhere]"
													data-testid="knowledge-excerpt"
												>
													<span class="font-semibold text-fg-muted"
														>{hit.excerptLabel ?? 'Excerpt'}:</span
													>
													{hit.excerpt}
												</p>
											{/if}
										</li>
									{/each}
								</ul>
							</section>
						{/each}
					</div>
				{/if}

				{#if modeParam !== 'knowledge'}
					{#each GROUPS as group (group.key)}
						{@const hits = found[group.key]}
						{#if hits.length > 0}
							<section aria-labelledby="search-{group.key}" class="pt-6">
								<h3 id="search-{group.key}" class="text-base/7 font-semibold text-fg {gutters}">
									{group.label}
									<span class="text-sm/6 font-normal text-fg-muted">{hits.length}</span>
								</h3>
								<ul
									role="list"
									class="mt-3 divide-y divide-line-subtle border-y border-line-subtle"
								>
									{#each hits as hit (hit.id)}
										<li class="relative py-4 hover:bg-tint/[0.03] {gutters}">
											<a
												href={resolve(hit.href as '/')}
												class="text-sm/6 font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
												><span class="absolute inset-0" aria-hidden="true"></span>{hit.title}</a
											>
											{#if hit.meta}
												<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">{hit.meta}</p>
											{/if}
										</li>
									{/each}
								</ul>
							</section>
						{/if}
					{/each}
				{/if}
			{/if}
		{/snippet}
	</Loaded>
{/if}
