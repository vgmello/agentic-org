<script lang="ts">
	import './org-tree.css';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import { SvelteSet } from 'svelte/reactivity';
	import MemberTile from './MemberTile.svelte';
	import { otherTeamsOf, subTeamCount, type TeamNode } from '$lib/org/model';
	import { peopleCard } from '$lib/org/reporting';
	import type { OrgSnapshot } from '$lib/proto';

	let {
		snapshot,
		tree,
		collapsed,
		selected,
		onselect,
		fit = true
	}: {
		snapshot: OrgSnapshot;
		tree: TeamNode[];
		collapsed: SvelteSet<string>;
		selected: string | undefined;
		onselect: (id: string) => void;
		fit?: boolean;
	} = $props();

	let chart = $state<HTMLElement>();
	let available = $state(0);
	let natural = $state({ w: 0, h: 0 });
	const scale = $derived(fit && natural.w > 0 && available > 0 ? Math.min(1, Math.max(0.45, available / natural.w)) : 1);

	$effect(() => {
		if (!chart) return;
		// Apply the new size on the next frame, not inside the observer callback: changing layout there
		// is what makes browsers log "ResizeObserver loop completed with undelivered notifications".
		let frame = 0;
		const watch = new ResizeObserver(([entry]) => {
			cancelAnimationFrame(frame);
			frame = requestAnimationFrame(() => (natural = { w: entry.contentRect.width, h: entry.contentRect.height }));
		});
		watch.observe(chart);
		return () => {
			cancelAnimationFrame(frame);
			watch.disconnect();
		};
	});

	function toggle(id: string) {
		if (collapsed.has(id)) collapsed.delete(id);
		else collapsed.add(id);
	}
</script>

{#snippet branch(node: TeamNode)}
	{@const card = peopleCard(snapshot, node.team)}
	{@const open = !collapsed.has(node.team.id)}
	<div class="flex flex-col items-center" data-testid="people-card" data-team={node.team.id}>
		<div class="w-56 rounded-xl bg-gray-800/50 p-3 text-left outline-1 -outline-offset-1 outline-white/10">
			<h3 class="truncate text-sm/6 font-semibold text-white">{node.team.name}</h3>
			{#if card.lead}
				<!-- The lead is named whatever its kind; an agent shows here only for that (PEOPLE-1). -->
				<p class="text-xs/5 text-gray-400">Lead: <span class="text-gray-200">{card.lead.name}</span>{card.lead.kind === 'agent' ? ' (agent)' : ''}</p>
			{/if}
			{#if card.people.length === 0}
				<p class="mt-1 text-xs/5 text-gray-500">No people</p>
			{:else}
				<ul class="mt-2 space-y-1.5" aria-label="People in {node.team.name}">
					{#each card.people as person (person.id)}
						<li class="flex items-center gap-x-2">
							<MemberTile member={person} size="sm" selected={selected === person.id} alsoIn={otherTeamsOf(snapshot, person.id, node.team.id)} {onselect} />
							<span class="truncate text-sm/5 text-gray-200" aria-hidden="true">{person.name}</span>{#if card.lead?.id === person.id}<span class="rounded bg-white/10 px-1 text-[0.65rem] leading-4 text-gray-300">lead</span>{/if}
						</li>
					{/each}
				</ul>
			{/if}
		</div>
		{#if node.children.length > 0}
			<button
				type="button"
				onclick={() => toggle(node.team.id)}
				aria-expanded={open}
				aria-label="{subTeamCount(node)} {subTeamCount(node) === 1 ? 'team' : 'teams'}: {open ? 'collapse' : 'expand'} the teams under {node.team.name}"
				class="-mt-px flex h-6 items-center gap-x-1 rounded-b-md bg-gray-800/70 px-2 text-xs text-gray-300 outline-1 -outline-offset-1 outline-white/10 hover:bg-white/10 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-500"
			>
				{subTeamCount(node)} {subTeamCount(node) === 1 ? 'team' : 'teams'}
				<ChevronDown class="size-3.5 transition-transform {open ? '' : '-rotate-90'}" aria-hidden="true" />
			</button>
		{/if}
	</div>
	{#if node.children.length > 0 && open}
		<ul class="t">
			{#each node.children as child (child.team.id)}<li class="t">{@render branch(child)}</li>{/each}
		</ul>
	{/if}
{/snippet}

<div bind:clientWidth={available} class="w-full">
	<div class="mx-auto" style="width: {natural.w * scale}px; height: {natural.h * scale}px;">
		<div bind:this={chart} class="org-tree" data-testid="people-chart" style="transform: scale({scale}); transform-origin: top left;">
			<ul class="t" style="gap: 2.5rem; align-items: flex-start;">
				{#each tree as node (node.team.id)}<li class="t">{@render branch(node)}</li>{/each}
			</ul>
		</div>
	</div>
</div>
