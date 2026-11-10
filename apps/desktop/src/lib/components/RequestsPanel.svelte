<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { Input } from '$lib/components/ui/input';
	import { nameOf } from '$lib/work/people';
	import { ageWord, filterActive, filterRequests, narrowed, groupRequests, NO_FILTER, nextAction, REQUEST_GROUPS, requestRef, stateWords, type RequestFilter } from '$lib/work/requests-view';

	/** The Requests queue (feature-requests-ux §2): what needs you first, then what is with agents, then the rest. */
	let {
		items,
		snapshot,
		now,
		onopen,
		onnew
	}: {
		items: WorkItemSummary[];
		snapshot: OrgSnapshot;
		now: string;
		/** Open a request's page; `start` begins one of its steps at once (the Route button of a request not routed). */
		onopen: (id: string, start?: 'route') => void;
		onnew: () => void;
	} = $props();

	let filter = $state<RequestFilter>({ ...NO_FILTER });
	const requests = $derived(items.filter((i) => i.kind === 'request' && !i.deleted));
	const shown = $derived(filterRequests(requests, filter));
	const groups = $derived(groupRequests(shown));
	const total = $derived(requests.length);
	const matching = $derived(shown.length);
	// Who and which projects the requests mention, for the pickers.
	const scopers = $derived([...new Set(requests.flatMap((r) => (r.assignee_id ? [r.assignee_id] : [])))].sort((a, b) => nameOf(snapshot, a).localeCompare(nameOf(snapshot, b))));
	const me = $derived(snapshot.members.find((m) => m.owner)?.id ?? 'owner');
	const raisers = $derived([...new Set(requests.map((r) => r.created_by))].sort((a, b) => nameOf(snapshot, a).localeCompare(nameOf(snapshot, b))));
	const projects = $derived((snapshot.projects ?? []).filter((p) => requests.some((r) => r.project_id === p.id)));
	const selectClass = 'rounded-md bg-gray-800 px-2 py-1.5 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400';
	const projectName = (id: string | undefined) => (id ? ((snapshot.projects ?? []).find((p) => p.id === id)?.name ?? '') : '');
</script>

<div class="space-y-4" data-testid="requests-queue">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<p class="text-sm/6 text-gray-400">Ideas and needs that aren't tasks yet. Someone scopes each one, then you approve it.</p>
		<Button size="sm" onclick={onnew} data-testid="new-request">New request</Button>
	</div>

	{#if total > 0}
		<div class="flex flex-wrap items-center gap-2" role="search" aria-label="Filter requests" data-testid="request-filters">
			<select aria-label="State" bind:value={filter.state} class={selectClass}><option value="open">Open</option><option value="all">All</option></select>
			<select aria-label="Who scopes it" bind:value={filter.who} class={selectClass}>
				<option value="">Anyone</option><option value="none">Not routed</option>
				{#if raisers.includes(me)}<option value="raised:{me}">Raised by me</option>{/if}
				{#each raisers.filter((id) => id !== me) as id (id)}<option value="raised:{id}">Raised by {nameOf(snapshot, id)}</option>{/each}
				{#each scopers as id (id)}<option value="scoped:{id}">Scoped by {nameOf(snapshot, id)}</option>{/each}
			</select>
			<select aria-label="Project" bind:value={filter.project} class={selectClass}>
				<option value="">All projects</option><option value="none">No project</option>
				{#each projects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
			</select>
			<Input type="search" aria-label="Search requests" placeholder="Search" bind:value={filter.query} class="max-w-56" autocomplete="off" />
			{#if filterActive(filter)}<Button size="sm" variant="outline" onclick={() => (filter = { ...NO_FILTER })}>Clear</Button>{/if}
			<span class="text-sm/6 text-gray-400" aria-live="polite" data-testid="request-count">{matching === total ? `${total} ${total === 1 ? 'request' : 'requests'}` : `${matching} of ${total}`}</span>
		</div>
	{/if}

	{#if total === 0}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-gray-300" data-testid="no-requests">No requests yet. Capture one when something comes to mind: write what you need and choose who scopes it.</p>
	{:else if matching === 0}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-gray-300" data-testid="no-request-match">No requests match. <button type="button" class="underline" onclick={() => (filter = { ...NO_FILTER })}>Clear the filters</button></p>
	{:else}
		{#each REQUEST_GROUPS as group (group.id)}
			{@const list = groups[group.id]}
			{#if list.length > 0}
				<details open={!group.collapsed || narrowed(filter)} class="group rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10" data-testid="request-group" data-group={group.id}>
					<summary class="flex cursor-pointer items-center gap-2 px-4 py-2 text-xs/5 font-semibold tracking-wide text-gray-300 uppercase focus-visible:outline-2 focus-visible:outline-indigo-400">{group.label} · {list.length}</summary>
					<ul class="divide-y divide-white/5">
						{#each list as r (r.id)}
							<li class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2 text-sm/6" data-testid="request-row" data-item={r.id}>
								<span class="w-16 shrink-0 text-gray-400">{requestRef(r)}</span>
								<button type="button" class="min-w-0 flex-1 text-left font-semibold text-white hover:underline focus-visible:outline-2 focus-visible:outline-indigo-400 [overflow-wrap:anywhere]" onclick={() => onopen(r.id)}>{r.title}</button>
								<span class="text-gray-300" data-testid="request-words-state">{stateWords(r, snapshot)}</span>
								{#if projectName(r.project_id)}<Badge variant="secondary">{projectName(r.project_id)}</Badge>{/if}
								<span class="w-16 shrink-0 text-right text-gray-400">{ageWord(r.created_at, now)}</span>
								{#if nextAction(r)}<Button size="sm" variant="outline" aria-label="{nextAction(r)} {requestRef(r)}" onclick={() => onopen(r.id, r.status === 'new' ? 'route' : undefined)}>{nextAction(r)}</Button>{/if}
							</li>
						{/each}
					</ul>
				</details>
			{/if}
		{/each}
	{/if}
</div>
