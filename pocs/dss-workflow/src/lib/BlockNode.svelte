<script lang="ts">
	import { Handle, Position } from '@xyflow/svelte';
	import type { BlockDef } from './engine';
	import { sim } from './run.svelte';

	let { data }: { data: { block: BlockDef } } = $props();
	const block = $derived(data.block);
	const current = $derived(sim.step?.block === block.id ? sim.step : undefined);
	const visited = $derived(sim.run?.steps.some((s) => s.block === block.id) ?? false);
	const status = $derived(current ? 'active' : visited ? 'visited' : 'idle');
</script>

<Handle type="target" position={Position.Top} />
<div class="node {status}" style:--stage="var(--stage-{block.stage})">
	<div class="title">{block.title}</div>
	<div class="agent">{sim.flow.def.agents[block.agent].label}</div>
	<div class="tags">
		{#if current}<span class="tag state {current.state}">{current.state}</span>{/if}
		{#if block.interactive}<span class="tag user">talks to user</span>{/if}
		{#each block.produces ?? [] as p (p)}<span class="tag">→ {p}</span>{/each}
	</div>
</div>
<Handle type="source" position={Position.Bottom} />

<style>
	.node {
		width: 210px;
		padding: 8px 10px;
		border-radius: 8px;
		background: var(--surface);
		border: 1px solid var(--border);
		border-left: 5px solid var(--stage);
		font-size: 12px;
		transition: box-shadow 0.2s, opacity 0.2s;
	}
	.node.active {
		box-shadow: 0 0 0 3px var(--accent);
	}
	.node.idle {
		opacity: 0.75;
	}
	.title {
		font-weight: 600;
		font-size: 13px;
	}
	.agent {
		color: var(--muted);
		margin-top: 2px;
	}
	.tags {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		margin-top: 4px;
	}
	.tag {
		font-size: 10px;
		padding: 1px 6px;
		border-radius: 999px;
		background: var(--chip);
	}
	.tag.user {
		background: var(--accent-soft);
	}
	.tag.state {
		font-weight: 600;
		color: #fff;
		background: var(--muted);
	}
	.tag.state.dispatched {
		background: var(--stage-delivery);
	}
	.tag.state.running {
		background: var(--stage-reconcile);
	}
</style>
