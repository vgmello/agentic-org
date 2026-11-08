<script lang="ts">
	import './org-tree.css';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Crown from '@lucide/svelte/icons/crown';
	import { SvelteSet } from 'svelte/reactivity';
	import MemberTile from './MemberTile.svelte';
	import { kindWord, teamsOfMember } from '$lib/org/model';
	import { directReports, type ReportNode } from '$lib/org/reporting';
	import type { OrgSnapshot } from '$lib/proto';

	let {
		snapshot,
		root,
		collapsed,
		selected,
		onselect,
		fit = true
	}: {
		snapshot: OrgSnapshot;
		root: ReportNode;
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

{#snippet branch(node: ReportNode)}
	{@const member = node.member}
	{@const teams = teamsOfMember(snapshot, member.id)}
	{@const reports = directReports(node)}
	{@const open = !collapsed.has(member.id)}
	<div class="flex w-52 flex-col items-center" data-testid="report-node" data-member={member.id}>
		<div class="flex w-full items-start gap-x-3 rounded-xl bg-gray-800/50 p-3 text-left outline-1 -outline-offset-1 outline-white/10 {selected === member.id ? 'ring-2 ring-white' : ''}">
			{#if member.owner}
				<span class="flex size-10 shrink-0 items-center justify-center rounded-full bg-amber-400/15 text-amber-200 outline-1 -outline-offset-1 outline-amber-300/30">
					<Crown class="size-5" aria-hidden="true" />
				</span>
			{:else}
				<MemberTile {member} selected={selected === member.id} {onselect} />
			{/if}
			<div class="min-w-0">
				<p class="truncate text-sm/5 font-semibold text-white">{member.name}</p>
				<p class="text-xs/5 text-gray-400">
					{member.owner ? 'Owner (you)' : kindWord(member.kind)}
				</p>
				{#if !member.owner}
					<p class="truncate text-xs/5 text-gray-400" title={teams.map((t) => t.name).join(', ')}>
						{teams.length ? teams.map((t) => t.name).join(', ') : 'Not in a team'}
					</p>
				{/if}
				<p class="text-xs/5 text-gray-500">{reports} {reports === 1 ? 'report' : 'reports'}</p>
			</div>
		</div>
		{#if node.children.length > 0}
			<button
				type="button"
				onclick={() => toggle(member.id)}
				aria-expanded={open}
				aria-label="{reports} {reports === 1 ? 'report' : 'reports'}: {open ? 'collapse' : 'expand'} the people reporting to {member.name}"
				class="-mt-px flex h-6 items-center gap-x-1 rounded-b-md bg-gray-800/70 px-2 text-xs text-gray-300 outline-1 -outline-offset-1 outline-white/10 hover:bg-white/10 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-500"
			>
				{reports}
				<ChevronDown class="size-3.5 transition-transform {open ? '' : '-rotate-90'}" aria-hidden="true" />
			</button>
		{/if}
	</div>
	{#if node.children.length > 0 && open}
		<ul class="t">
			{#each node.children as child (child.member.id)}<li class="t">{@render branch(child)}</li>{/each}
		</ul>
	{/if}
{/snippet}

<div bind:clientWidth={available} class="w-full">
	<div class="mx-auto" style="width: {natural.w * scale}px; height: {natural.h * scale}px;">
		<div bind:this={chart} class="org-tree" data-testid="reporting-chart" style="transform: scale({scale}); transform-origin: top left;">
			<ul class="t">
				<li class="t">{@render branch(root)}</li>
			</ul>
		</div>
	</div>
</div>
