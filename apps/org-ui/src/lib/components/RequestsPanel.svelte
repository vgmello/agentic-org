<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import type { OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import { ageWord, groupRequests, nextAction, REQUEST_GROUPS, requestRef, stateWords } from '$lib/work/requests-view';

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

	const groups = $derived(groupRequests(items));
	const total = $derived(Object.values(groups).reduce((n, g) => n + g.length, 0));
	const projectName = (id: string | undefined) => (id ? ((snapshot.projects ?? []).find((p) => p.id === id)?.name ?? '') : '');
</script>

<div class="space-y-4" data-testid="requests-queue">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<p class="text-sm/6 text-gray-400">Ideas and needs that aren't tasks yet. Someone scopes each one, then you approve it.</p>
		<Button size="sm" onclick={onnew} data-testid="new-request">New request</Button>
	</div>

	{#if total === 0}
		<p class="rounded-xl bg-white/5 px-6 py-8 text-center text-sm/6 text-gray-300" data-testid="no-requests">No requests yet. Capture one when something comes to mind: write what you need and choose who scopes it.</p>
	{:else}
		{#each REQUEST_GROUPS as group (group.id)}
			{@const list = groups[group.id]}
			{#if list.length > 0}
				<details open={!group.collapsed} class="group rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10" data-testid="request-group" data-group={group.id}>
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
