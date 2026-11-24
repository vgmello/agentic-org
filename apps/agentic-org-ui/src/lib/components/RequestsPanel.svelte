<script lang="ts">
	import FileCheck from '@lucide/svelte/icons/file-check';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { Input } from '$lib/components/ui/input';
	import { nameOf } from '$lib/work/people';
	import { matchesWorkSearch } from '$lib/work/work-toolbar';
	import {
		ageWord,
		filterActive,
		filterRequests,
		narrowed,
		groupRequests,
		NO_FILTER,
		nextAction,
		STATE_FILTERS,
		REQUEST_GROUPS,
		requestRef,
		stateWords,
		type RequestFilter
	} from '$lib/work/requests-view';

	/** The Requests queue (feature-requests-ux §2): what needs you first, then what is with agents, then the rest. */
	let {
		items,
		snapshot,
		me,
		now,
		organizationId,
		externalQuery = '',
		onClearExternalQuery,
		hideNewButton = false,
		onopen,
		onnew
	}: {
		items: WorkItemSummary[];
		/** The organization, so each one remembers its own filters. */
		organizationId: string;
		snapshot: OrgSnapshot;
		/** The owner's id: "Raised by me". */
		me: string;
		now: string;
		/** Optional live query from the Work header search bar. */
		externalQuery?: string;
		/** Clear the Work header search query when the user clears Requests filters. */
		onClearExternalQuery?: () => void;
		/** Hide the inline New request button when the Work toolbar owns the primary CTA. */
		hideNewButton?: boolean;
		/** Open a request's page; `start` begins one of its steps at once (the Route button of a request not routed). */
		onopen: (id: string, start?: 'route') => void;
		onnew: () => void;
	} = $props();

	const KEY = (id: string) => `agentic-org.requests.filter.${id}`;
	// The filters are remembered per organization (RQS-15): what was left is read back when the queue opens.
	function remembered(): RequestFilter {
		try {
			const saved = JSON.parse(
				localStorage.getItem(KEY(organizationId)) ?? 'null'
			) as Partial<RequestFilter> | null;
			if (saved && typeof saved === 'object')
				return {
					state: STATE_FILTERS.some((s) => s.id === saved.state)
						? (saved.state as RequestFilter['state'])
						: 'open',
					who: typeof saved.who === 'string' ? saved.who : '',
					project: typeof saved.project === 'string' ? saved.project : '',
					query: typeof saved.query === 'string' ? saved.query.slice(0, 200) : ''
				};
		} catch {
			// unreadable storage: open on the defaults
		}
		return { ...NO_FILTER };
	}
	let filter = $state<RequestFilter>(remembered());
	$effect(() => {
		const now = JSON.stringify({
			state: live.state,
			who: live.who,
			project: live.project,
			query: live.query
		});
		try {
			localStorage.setItem(KEY(organizationId), now);
		} catch {
			// kept for this visit only
		}
	});
	const requests = $derived(items.filter((i) => i.kind === 'request' && !i.deleted));
	// Who and which projects the requests mention, for the pickers.
	const scopers = $derived(
		[...new Set(requests.flatMap((r) => (r.assignee_id ? [r.assignee_id] : [])))].sort((a, b) =>
			nameOf(snapshot, a).localeCompare(nameOf(snapshot, b))
		)
	);
	const raisers = $derived(
		[...new Set(requests.map((r) => r.created_by))].sort((a, b) =>
			nameOf(snapshot, a).localeCompare(nameOf(snapshot, b))
		)
	);
	// Every project that is not archived, and the one a request is in even if it has since been archived.
	const allProjects = $derived(
		(snapshot.projects ?? []).filter(
			(p) => !p.archived || requests.some((r) => r.project_id === p.id)
		)
	);
	// A remembered person or project that no request has any more is not a filter worth keeping: it would hide everything with nothing to show for it.
	// (Nothing is dropped while there are no requests at all: they may simply not have arrived yet.)
	const live = $derived.by(() => {
		if (requests.length === 0) return filter;
		const people = new Set([
			'',
			...requests.flatMap((r) => [
				`raised:${r.created_by}`,
				...(r.assignee_id ? [`scoped:${r.assignee_id}`] : [])
			])
		]);
		const projectsHere = new Set(['', 'none', ...allProjects.map((p) => p.id)]);
		return {
			...filter,
			who: people.has(filter.who) ? filter.who : '',
			project: projectsHere.has(filter.project) ? filter.project : ''
		};
	});
	const projectName = (id: string | undefined) =>
		id ? ((snapshot.projects ?? []).find((p) => p.id === id)?.name ?? '') : '';
	const hasExternalQuery = $derived(externalQuery.trim().length > 0);
	const shown = $derived(
		filterRequests(requests, live).filter((r) =>
			matchesWorkSearch(r, externalQuery, {
				refText: requestRef(r),
				projectName: projectName(r.project_id),
				assigneeName: nameOf(snapshot, r.assignee_id)
			})
		)
	);
	const groups = $derived(groupRequests(shown));
	const total = $derived(requests.length);
	const matching = $derived(shown.length);
	function clearFilters() {
		filter = { ...NO_FILTER };
		onClearExternalQuery?.();
	}
	const selectClass =
		'min-h-9 max-w-full min-w-0 rounded-md bg-surface px-2 py-1.5 text-sm/6 text-fg outline-1 -outline-offset-1 outline-field-border focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11';
</script>

<div class="space-y-4" data-testid="requests-queue">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div class="min-w-0">
			<p class="text-xs/5 font-medium text-fg-muted">Requests across every project.</p>
			<p class="text-sm/6 text-fg-secondary">
				Ideas and needs that aren't tasks yet. Someone scopes each one, then you approve it.
			</p>
		</div>
		{#if !hideNewButton}
			<button
				type="button"
				class="req-cta inline-flex min-h-9 items-center justify-center rounded-lg px-3.5 text-sm/5 font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
				onclick={onnew}
				data-testid="new-request"
			>
				New request
			</button>
		{/if}
	</div>

	{#if total > 0}
		<div
			class="flex flex-wrap items-center gap-2"
			role="search"
			aria-label="Filter requests"
			data-testid="request-filters"
		>
			<select aria-label="State" bind:value={filter.state} class={selectClass}
				>{#each STATE_FILTERS as s (s.id)}<option value={s.id}>{s.label}</option>{/each}</select
			>
			<select
				aria-label="People"
				bind:value={() => live.who, (v) => (filter.who = v)}
				class={selectClass}
			>
				<option value="">Anyone</option>
				{#if raisers.includes(me)}<option value="raised:{me}">Raised by me</option>{/if}
				{#each raisers.filter((id) => id !== me) as id (id)}<option value="raised:{id}"
						>Raised by {nameOf(snapshot, id)}</option
					>{/each}
				{#each scopers as id (id)}<option value="scoped:{id}"
						>Scoped by {nameOf(snapshot, id)}</option
					>{/each}
			</select>
			<select
				aria-label="Project"
				bind:value={() => live.project, (v) => (filter.project = v)}
				class={selectClass}
			>
				<option value="">All projects</option><option value="none">No project</option>
				{#each allProjects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
			</select>
			<Input
				type="search"
				aria-label="Search requests"
				placeholder="Search"
				bind:value={filter.query}
				class="min-h-9 max-w-56 focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
				autocomplete="off"
			/>
			{#if filterActive(live) || hasExternalQuery}<Button
					size="sm"
					variant="outline"
					class="min-h-9 min-w-9 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11"
					onclick={clearFilters}>Clear</Button
				>{/if}
			<span class="text-sm/6 text-fg-muted" aria-live="polite" data-testid="request-count"
				>{matching === total
					? `${total} ${total === 1 ? 'request' : 'requests'}`
					: `${matching} of ${total}`}</span
			>
		</div>
	{/if}

	{#if total === 0}
		<p
			class="rounded-xl bg-surface-raised px-6 py-8 text-center text-sm/6 text-fg-secondary"
			data-testid="no-requests"
		>
			No requests yet. Capture one when something comes to mind: write what you need and choose who
			scopes it.
		</p>
	{:else if matching === 0 && live.state === 'open' && !narrowed(live) && !hasExternalQuery}
		<p
			class="rounded-xl bg-surface-raised px-6 py-8 text-center text-sm/6 text-fg-secondary"
			data-testid="nothing-open"
		>
			Nothing is open. <button
				type="button"
				class="inline-flex min-h-9 items-center underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
				onclick={() => (filter.state = 'all')}>Show all requests</button
			>
		</p>
	{:else if matching === 0}
		<p
			class="rounded-xl bg-surface-raised px-6 py-8 text-center text-sm/6 text-fg-secondary"
			data-testid="no-request-match"
		>
			No requests match. <button
				type="button"
				class="inline-flex min-h-9 items-center underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11"
				onclick={clearFilters}>Clear filters</button
			>
		</p>
	{:else}
		{#each REQUEST_GROUPS as group (group.id)}
			{@const list = groups[group.id]}
			{#if list.length > 0}
				<details
					open={!group.collapsed || narrowed(live) || hasExternalQuery || live.state === group.id}
					class="group rounded-xl bg-surface-raised outline-1 -outline-offset-1 outline-line"
					data-testid="request-group"
					data-group={group.id}
				>
					<summary
						class="flex min-h-11 cursor-pointer items-center gap-2 px-4 py-2 text-xs/5 font-semibold tracking-wide text-fg-secondary uppercase focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
						>{group.label} · {list.length}</summary
					>
					<ul class="divide-y divide-line-subtle">
						{#each list as r (r.id)}
							<li
								class="flex flex-col gap-x-3 gap-y-1 px-4 py-2 text-sm/6 sm:flex-row sm:flex-wrap sm:items-center"
								data-testid="request-row"
								data-item={r.id}
							>
								<span class="w-16 shrink-0 text-fg-muted">{requestRef(r)}</span>
								<button
									type="button"
									class="min-h-9 min-w-9 flex-1 text-left font-semibold text-fg hover:underline focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:break-word] [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11"
									onclick={() => onopen(r.id)}>{r.title}</button
								>
								<span class="text-fg-secondary" data-testid="request-words-state"
									>{#if r.status === 'scoped'}<Badge
											variant="secondary"
											class="gap-1 bg-quiet/10 text-fg-secondary ring-1 ring-quiet/20 ring-inset"
											><FileCheck class="size-3.5" aria-hidden="true" />Scoped</Badge
										> by {nameOf(snapshot, r.assignee_id)}{:else}{stateWords(
											r,
											snapshot
										)}{/if}</span
								>
								{#if projectName(r.project_id)}<Badge
										variant="secondary"
										class="h-auto min-h-5 max-w-full whitespace-normal [overflow-wrap:anywhere]"
										>{projectName(r.project_id)}</Badge
									>{/if}
								<span class="w-16 shrink-0 text-right text-fg-muted"
									>{ageWord(r.created_at, now)}</span
								>
								{#if nextAction(r)}<Button
										size="sm"
										variant="outline"
										class="min-h-9 min-w-9 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11"
										aria-label="{nextAction(r)} {requestRef(r)}"
										onclick={() => onopen(r.id, r.status === 'new' ? 'route' : undefined)}
										>{nextAction(r)}</Button
									>{/if}
							</li>
						{/each}
					</ul>
				</details>
			{/if}
		{/each}
	{/if}
</div>

<style>
	/* Primary Requests CTA: >= 4.5:1 WCAG AA contrast at rest & hover across all 4 themes */
	.req-cta {
		background: var(--primary);
		color: var(--on-emphasis);
		cursor: pointer;
	}
	.req-cta:hover {
		background: color-mix(in srgb, var(--primary) 84%, var(--surface));
	}
	:global(html[data-ui-theme='ocean']) .req-cta {
		background: var(--primary-hover);
	}
	:global(html[data-ui-theme='ocean']) .req-cta:hover {
		background: color-mix(in srgb, var(--primary-hover) 84%, var(--surface));
	}
	:global(html[data-ui-theme='report']) .req-cta {
		border-radius: var(--exec-ctrl-radius);
	}
	:global(html[data-ui-theme='report']) .req-cta:hover {
		background: var(--primary-hover);
	}
	@media (forced-colors: active) {
		.req-cta {
			border: 1px solid ButtonText;
		}
	}
</style>
