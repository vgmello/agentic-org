<script lang="ts">
	import { Background, Controls, MarkerType, SvelteFlow, type Edge, type Node } from '@xyflow/svelte';
	import '@xyflow/svelte/dist/style.css';
	import BlockNode from './lib/BlockNode.svelte';
	import { guardFlags, guardPasses } from './lib/engine';
	import { run } from './lib/run.svelte';
	import { flow } from './lib/workflow';

	// Presentation-only layout; the wiring lives in workflow/dss.flow.json.
	const layout: Record<string, [number, number]> = {
		'project-setup': [0, 0],
		'product-dss-dialogue': [0, 130],
		'publish-product-dss': [0, 260],
		'tech-dss-autonomous': [-180, 400],
		'tech-dss-interactive': [180, 400],
		'architecture-standards': [420, 520],
		'implementation-planning': [0, 640],
		'execute-tasks': [0, 780],
		'architect-consult': [-340, 780],
		'code-review': [0, 920],
		reconcile: [0, 1060],
		converged: [0, 1200]
	};

	const nodeTypes = { block: BlockNode };
	let nodes = $state.raw<Node[]>(
		flow.def.blocks.map((block, i) => {
			const [x, y] = layout[block.id] ?? [700, i * 130];
			return { id: block.id, type: 'block', position: { x, y }, data: { block } };
		})
	);

	const edges = $derived<Edge[]>(
		flow.def.transitions.map((t, i) => {
			const fromActive = run.active?.block.id === t.from;
			const open = fromActive && guardPasses(t.when, run.ctx);
			const last = run.lastEdge?.from === t.from && run.lastEdge.on === t.on && run.lastEdge.to === t.to;
			return {
				id: `e${i}`,
				source: t.from,
				target: t.to,
				label: t.when ? `${t.on} [${t.when}]` : t.on,
				animated: open,
				markerEnd: { type: MarkerType.ArrowClosed },
				class: open ? 'open' : last ? 'last' : fromActive ? 'blocked' : '',
				labelStyle: 'font-size:10px'
			};
		})
	);

	let preview = $state<string | null>(null);
	const shown = $derived(preview ? flow.activate(preview, 'preview') : run.active);
	const events = $derived(
		run.active
			? [...new Set(flow.outgoing(run.active.block.id).map((t) => t.on))].map((on) => ({
					on,
					enabled: flow.outgoing(run.active!.block.id).some((t) => t.on === on && guardPasses(t.when, run.ctx))
				}))
			: []
	);
	const flags = guardFlags(flow.def);
</script>

<main>
	<section class="canvas">
		<header>
			<h1>{flow.def.title}</h1>
			<p>REQ-01 Markdown blocks + JSON wiring · REQ-02 status handoff · REQ-06 DSS factory</p>
			<div class="legend">
				{#each ['setup', 'product', 'factory', 'delivery', 'reconcile'] as s (s)}
					<span style:--c="var(--stage-{s})">{s}</span>
				{/each}
			</div>
		</header>
		<SvelteFlow
			bind:nodes
			{edges}
			{nodeTypes}
			fitView
			onnodeclick={({ node }) => (preview = node.id)}
			onpaneclick={() => (preview = null)}
		>
			<Background />
			<Controls />
		</SvelteFlow>
	</section>

	<aside>
		<h2>Agent commands</h2>
		<div class="row">
			{#each flow.def.triggers as t (t.command)}
				<button onclick={() => ((preview = null), run.command(t.command))}>/{t.command}</button>
			{/each}
		</div>

		<h2>Context</h2>
		{#each flags as f (f)}
			<label><input type="checkbox" bind:checked={run.ctx[f]} /> <code>{f}</code></label>
		{/each}

		{#if run.active}
			<h2>Report event from <code>{run.active.block.id}</code></h2>
			<div class="row">
				{#each events as e (e.on)}
					<button disabled={!e.enabled} onclick={() => ((preview = null), run.fire(e.on))}>{e.on}</button>
				{:else}
					<em>terminal block</em>
				{/each}
			</div>
			{#if run.error}<p class="error">{run.error}</p>{/if}
		{/if}

		{#if shown}
			<h2>
				{preview ? 'Preview' : 'Active block'} · {shown.agent.label}
			</h2>
			{#if !preview && shown.handoff}
				<p class="handoff">
					Handoff: {run.history.at(-2)?.agent.label} → <strong>{shown.agent.label}</strong>
				</p>
			{/if}
			<pre class="md">{shown.markdown}</pre>
		{:else}
			<p class="hint">Run a command to start the flow, or click a block to preview its Markdown.</p>
		{/if}

		{#if run.history.length}
			<h2>Activation log</h2>
			<ol class="log">
				{#each run.history as a, i (i)}
					<li><span>{a.cause}</span> <strong>{a.block.id}</strong> <em>@{a.agent.id}</em></li>
				{/each}
			</ol>
		{/if}

		<details>
			<summary>Flow wiring (JSON)</summary>
			<pre>{JSON.stringify(flow.def.transitions, null, 1)}</pre>
		</details>
	</aside>
</main>

<style>
	:global(:root) {
		--bg: #f6f7f9;
		--surface: #fff;
		--border: #d6d9de;
		--text: #1b1f24;
		--muted: #5b6470;
		--chip: #eef0f3;
		--accent: #6d4aff;
		--accent-soft: #e6e0ff;
		--stage-setup: #94a3b8;
		--stage-product: #0ea5e9;
		--stage-factory: #a855f7;
		--stage-delivery: #f59e0b;
		--stage-reconcile: #10b981;
		color-scheme: light dark;
	}
	@media (prefers-color-scheme: dark) {
		:global(:root) {
			--bg: #0f1115;
			--surface: #181b21;
			--border: #2c313a;
			--text: #e6e8eb;
			--muted: #98a1ad;
			--chip: #252a32;
			--accent-soft: #3a2f6b;
		}
	}
	:global(body) {
		margin: 0;
		background: var(--bg);
		color: var(--text);
		font: 14px/1.4 system-ui, sans-serif;
	}
	main {
		display: grid;
		grid-template-columns: 1fr 420px;
		height: 100vh;
	}
	.canvas {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	header {
		padding: 12px 16px;
		border-bottom: 1px solid var(--border);
	}
	h1 {
		margin: 0;
		font-size: 18px;
	}
	header p {
		margin: 2px 0 6px;
		color: var(--muted);
		font-size: 12px;
	}
	.legend {
		display: flex;
		gap: 10px;
		font-size: 11px;
	}
	.legend span::before {
		content: '';
		display: inline-block;
		width: 10px;
		height: 10px;
		border-radius: 2px;
		margin-right: 4px;
		background: var(--c);
		vertical-align: -1px;
	}
	.canvas :global(.svelte-flow) {
		flex: 1;
		--xy-background-color: var(--bg);
		--xy-edge-label-background-color: var(--surface);
		--xy-edge-label-color: var(--text);
	}
	.canvas :global(.svelte-flow__node-block) {
		padding: 0;
		border: 0;
		background: none;
	}
	.canvas :global(.svelte-flow__edge.open path) {
		stroke: var(--accent);
		stroke-width: 2;
	}
	.canvas :global(.svelte-flow__edge.last path) {
		stroke: var(--stage-reconcile);
		stroke-width: 2.5;
	}
	.canvas :global(.svelte-flow__edge.blocked path) {
		stroke-dasharray: 4 4;
		opacity: 0.5;
	}
	aside {
		border-left: 1px solid var(--border);
		background: var(--surface);
		padding: 12px 16px;
		overflow-y: auto;
	}
	h2 {
		font-size: 12px;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--muted);
		margin: 16px 0 6px;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	button {
		font: inherit;
		font-size: 12px;
		padding: 4px 10px;
		border-radius: 6px;
		border: 1px solid var(--border);
		background: var(--chip);
		color: var(--text);
		cursor: pointer;
	}
	button:hover:not(:disabled) {
		border-color: var(--accent);
	}
	button:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	label {
		display: block;
		font-size: 12px;
	}
	.md,
	details pre {
		white-space: pre-wrap;
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: 6px;
		padding: 10px;
		font-size: 12px;
	}
	.handoff {
		background: var(--accent-soft);
		padding: 6px 10px;
		border-radius: 6px;
		font-size: 12px;
	}
	.error {
		color: #dc2626;
		font-size: 12px;
	}
	.hint {
		color: var(--muted);
	}
	.log {
		padding-left: 18px;
		font-size: 11px;
	}
	.log span,
	.log em {
		color: var(--muted);
	}
</style>
