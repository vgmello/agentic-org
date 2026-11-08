<script lang="ts">
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import { SvelteSet } from 'svelte/reactivity';
	import { initialsOf, reportCount, type AgentStatus, type OrgNode } from '$lib/org-chart/data';

	let {
		root,
		collapsed,
		fit = true
	}: { root: OrgNode; collapsed: SvelteSet<string>; fit?: boolean } = $props();

	// The chart's natural size, and the room it has. When `fit` is on it shrinks (never grows,
	// never below 45%) to the width available; scaling uses a transform, so layout stays natural.
	let chart = $state<HTMLElement>();
	let available = $state(0);
	let natural = $state({ w: 0, h: 0 });
	const scale = $derived(fit && natural.w > 0 && available > 0 ? Math.min(1, Math.max(0.45, available / natural.w)) : 1);

	$effect(() => {
		if (!chart) return;
		const watch = new ResizeObserver(([entry]) => {
			natural = { w: entry.contentRect.width, h: entry.contentRect.height };
		});
		watch.observe(chart);
		return () => watch.disconnect();
	});

	const STATUS: Record<AgentStatus, { label: string; dot: string }> = {
		working: { label: 'Working', dot: 'bg-green-400' },
		idle: { label: 'Idle', dot: 'bg-gray-400' },
		offline: { label: 'Offline', dot: 'bg-gray-600' }
	};

	function toggle(id: string) {
		if (collapsed.has(id)) collapsed.delete(id);
		else collapsed.add(id);
	}
</script>

{#snippet branch(node: OrgNode)}
	{@const kids = node.children ?? []}
	{@const open = !collapsed.has(node.id)}
	<div
		data-testid="org-node"
		data-node={node.id}
		class="flex w-52 flex-col items-center"
	>
		<div class="w-full rounded-xl bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10">
			<div class="flex items-center gap-x-3">
				<span
					class="relative flex size-10 shrink-0 items-center justify-center rounded-full bg-indigo-500/20 text-sm font-semibold text-indigo-300"
					aria-hidden="true"
				>
					{initialsOf(node.name)}
					{#if node.status}
						<span
							class="absolute -right-0.5 -bottom-0.5 size-3 rounded-full ring-2 ring-gray-800 {STATUS[node.status].dot}"
						></span>
					{/if}
				</span>
				<span class="min-w-0">
					<span class="block truncate text-sm/5 font-semibold text-white">{node.name}</span>
					<span class="block truncate text-xs/5 text-gray-400">{node.role}</span>
				</span>
			</div>
			{#if node.status}
				<div class="mt-3 flex items-center justify-between text-xs/5 text-gray-400">
					<span>{STATUS[node.status].label}</span>
					<span>{node.tasks ?? 0} open {node.tasks === 1 ? 'task' : 'tasks'}</span>
				</div>
			{/if}
		</div>
		{#if kids.length > 0}
			<button
				type="button"
				onclick={() => toggle(node.id)}
				aria-expanded={open}
				aria-label="{open ? 'Collapse' : 'Expand'} {node.name}'s reports"
				class="-mt-px flex h-6 items-center gap-x-1 rounded-b-md bg-white/5 px-2 text-xs text-gray-300 outline-1 -outline-offset-1 outline-white/10 hover:bg-white/10 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-500"
			>
				{reportCount(node)}
				<ChevronDown class="size-3.5 transition-transform {open ? '' : '-rotate-90'}" aria-hidden="true" />
			</button>
		{/if}
	</div>
	{#if kids.length > 0 && open}
		<ul>
			{#each kids as child (child.id)}
				<li>{@render branch(child)}</li>
			{/each}
		</ul>
	{/if}
{/snippet}

<div bind:clientWidth={available} class="w-full">
	<div class="mx-auto" style="width: {natural.w * scale}px; height: {natural.h * scale}px;">
		<div bind:this={chart} class="chart" style="transform: scale({scale}); transform-origin: top left;">
			<ul>
				<li>{@render branch(root)}</li>
			</ul>
		</div>
	</div>
</div>

<style>
	/* The tree's connectors are drawn with borders on each list item: a vertical stem down from
	   the parent, and a horizontal bar across the children with a short drop to each. */
	.chart {
		--line: rgb(255 255 255 / 0.18);
		--gap: 28px;
		width: max-content;
	}
	.chart :global(ul) {
		position: relative;
		display: flex;
		justify-content: center;
		padding-top: var(--gap);
	}
	.chart > :global(ul) {
		padding-top: 0;
	}
	.chart :global(li) {
		position: relative;
		display: flex;
		flex-direction: column;
		align-items: center;
		padding: var(--gap) 10px 0;
	}
	.chart > :global(ul) > :global(li) {
		padding-top: 0;
	}
	/* The bar across siblings: each li draws its left and right half. */
	.chart :global(li)::before,
	.chart :global(li)::after {
		content: '';
		position: absolute;
		top: 0;
		right: 50%;
		width: 50%;
		height: var(--gap);
		border-top: 1px solid var(--line);
	}
	.chart :global(li)::after {
		right: auto;
		left: 50%;
		border-left: 1px solid var(--line);
	}
	.chart > :global(ul) > :global(li)::before,
	.chart > :global(ul) > :global(li)::after,
	.chart :global(li:only-child)::before {
		display: none;
	}
	.chart :global(li:only-child)::after {
		/* A lone child hangs straight off the stem. */
		width: 0;
		left: 50%;
		border-top: 0;
	}
	.chart :global(li:first-child)::before {
		border: 0 none;
	}
	.chart :global(li:last-child)::after {
		border: 0 none;
	}
	.chart :global(li:last-child)::before {
		border-right: 1px solid var(--line);
		border-top-right-radius: 8px;
	}
	.chart :global(li:first-child)::after {
		border-top-left-radius: 8px;
	}
	/* The stem from a parent down to its children's bar. */
	.chart :global(ul ul)::before {
		content: '';
		position: absolute;
		top: 0;
		left: 50%;
		height: var(--gap);
		border-left: 1px solid var(--line);
	}
</style>
