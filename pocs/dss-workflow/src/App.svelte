<script lang="ts">
	import { Background, Controls, MarkerType, SvelteFlow, type Edge, type Node } from '@xyflow/svelte';
	import '@xyflow/svelte/dist/style.css';
	import BlockNode from './lib/BlockNode.svelte';
	import { CAPABILITY_CATALOG, type Transition } from './lib/engine';
	import { TASK_STATUSES } from './lib/host';
	import { SUBJECT_TASK, sim } from './lib/run.svelte';

	// Presentation-only layout; the wiring lives in workflow/dss.flow.yaml.
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
		sim.flow.def.blocks.map((block, i) => {
			const [x, y] = layout[block.id] ?? [700, i * 130];
			return { id: block.id, type: 'block', position: { x, y }, data: { block } };
		})
	);

	const label = (t: Transition) =>
		(t.on ? t.on : '⟳') + (t.where ? ` · ${t.where}` : '') + (t.when ? ` · ${t.when}` : '') + (t.mode === 'fallback' ? ' (fallback)' : '');

	const edges = $derived<Edge[]>(
		sim.flow.transitions.map((t, i) => {
			const fromActive = sim.step?.block === t.from;
			return {
				id: `e${i}-${sim.capabilities.join()}`,
				source: t.from,
				target: t.to,
				label: label(t),
				animated: fromActive,
				markerEnd: { type: MarkerType.ArrowClosed },
				class: [t.mode, t.on ? 'edge' : 'level', fromActive ? 'open' : ''].join(' '),
				labelStyle: 'font-size:10px'
			};
		})
	);

	const outcomes = $derived(sim.step ? sim.flow.outcomes(sim.step.block) : []);
	const stepFlags = $derived(sim.step ? [...new Set(outcomes.flatMap((o) => sim.flow.flagsFor(sim.step!.block, o)))] : []);
	let flagValues = $state<Record<string, boolean>>({});
	const activation = $derived(sim.step ? sim.flow.activate(sim.step.block) : undefined);
	const fallbacks = $derived(sim.flow.transitions.filter((t) => t.mode === 'fallback').length);

	function report(outcome: string) {
		const allowed = sim.flow.flagsFor(sim.step!.block, outcome);
		sim.report(outcome, Object.fromEntries(allowed.filter((f) => flagValues[f]).map((f) => [f, true])));
		flagValues = {};
	}

	function toggleCapability(name: string, on: boolean) {
		sim.reset(on ? [...sim.capabilities, name] : sim.capabilities.filter((c) => c !== name));
	}
</script>

<main>
	<section class="canvas">
		<header>
			<h1>{sim.flow.def.title} <small>v{sim.flow.def.version}</small></h1>
			<p>CEL conditions · optional host integrations · runner with step states and redispatch</p>
			<div class="legend">
				<span class="key edge-key">event (on)</span>
				<span class="key level-key">⟳ level (when)</span>
				<span class="key fallback-key">fallback</span>
				{#each ['setup', 'product', 'factory', 'delivery', 'reconcile'] as s (s)}
					<span class="stage" style:--c="var(--stage-{s})">{s}</span>
				{/each}
			</div>
		</header>
		<SvelteFlow bind:nodes {edges} {nodeTypes} fitView>
			<Background />
			<Controls />
		</SvelteFlow>
	</section>

	<aside>
		<h2>Host integrations</h2>
		{#each CAPABILITY_CATALOG as c (c.name)}
			<label>
				<input type="checkbox" checked={sim.capabilities.includes(c.name)} onchange={(e) => toggleCapability(c.name, e.currentTarget.checked)} />
				<code>{c.name}</code> <span class="muted">{Object.keys(c.schema).map((k) => `${c.name}.${k}`).join(', ')}</span>
			</label>
		{/each}
		<p class="muted small">
			{fallbacks ? `${fallbacks} transition(s) use their fallback.` : 'All transitions native.'} Changing this reloads the flow and resets the run.
		</p>

		<h2>Host: task {SUBJECT_TASK}</h2>
		<div class="row">
			{#each TASK_STATUSES as s (s)}
				<button class:selected={sim.host.tasks[SUBJECT_TASK].status === s} onclick={() => sim.moveTask(s)}>{s}</button>
			{/each}
		</div>
		{#if !sim.capabilities.includes('task')}<p class="muted small">No task integration: the runner can't see this board.</p>{/if}
		{#if sim.capabilities.includes('project')}
			<label><input type="checkbox" checked={sim.host.project.standards_defined} onchange={(e) => sim.setStandards(e.currentTarget.checked)} /> project has standards (<code>project.standards_defined</code>)</label>
		{/if}

		<h2>Start</h2>
		<div class="row">
			{#each sim.flow.def.triggers as t (t.command)}
				<button onclick={() => sim.start(t.command)}>/{t.command}</button>
			{/each}
		</div>

		{#if sim.run && sim.step}
			<h2>Run {sim.run.id} · step {sim.step.id}</h2>
			{#if sim.run.status === 'needs_attention'}<p class="error">Needs attention: {sim.run.attention}</p>{/if}
			<p class="stepline">
				<strong>{sim.step.block}</strong> @{sim.step.agent} · <span class="badge {sim.step.state}">{sim.step.state}</span> · attempts {sim.step.attempts}
			</p>
			{#if sim.step.state === 'dispatched'}
				<button onclick={() => sim.begin()}>{sim.step.agent}: start turn</button>
			{:else if sim.step.state === 'running'}
				{#each stepFlags as f (f)}
					<label><input type="checkbox" bind:checked={flagValues[f]} /> flag <code>{f}</code></label>
				{/each}
				<div class="row">
					{#each outcomes as o (o)}<button onclick={() => report(o)}>report {o}</button>{:else}<em class="muted">no agent outcomes — waits for a host fact/event</em>{/each}
				</div>
			{:else if sim.step.state === 'ready'}
				<p class="muted small">Open step with no turn: the reconcile loop will dispatch it.</p>
			{/if}
			{#if sim.lastResult}<p class="result">{sim.lastResult}</p>{/if}
		{/if}

		<h2>Failures</h2>
		<label><input type="checkbox" bind:checked={sim.loseNextDispatch} /> lose the next dispatch</label>
		<div class="row">
			<button onclick={() => sim.tick()}>tick (time passes)</button>
			<button onclick={() => sim.restart()}>restart runner from snapshot</button>
		</div>
		<p class="muted small">clock {sim.view.clock} · restarts {sim.restarts}</p>

		{#if activation}
			<details>
				<summary>Block Markdown sent to {activation.agent.label}</summary>
				<pre class="md">{activation.markdown}</pre>
			</details>
		{/if}

		{#if sim.log.length}
			<h2>Runner log</h2>
			<ol class="log">
				{#each sim.log as l, i (i)}
					<li><span>t{l.at}{l.run ? ` ${l.run}` : ''}</span> {l.text}</li>
				{/each}
			</ol>
		{/if}
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
	.legend .stage::before {
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
	.canvas :global(.svelte-flow__edge.level path) {
		stroke-dasharray: 2 3;
	}
	.canvas :global(.svelte-flow__edge.fallback path) {
		stroke: var(--stage-factory);
		stroke-dasharray: 6 4;
	}
	.canvas :global(.svelte-flow__edge.open path) {
		stroke: var(--accent);
		stroke-width: 2;
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
	.key {
		color: var(--muted);
	}
	.key::before {
		content: '';
		display: inline-block;
		width: 18px;
		margin-right: 4px;
		vertical-align: 3px;
		border-top: 2px solid var(--muted);
	}
	.level-key::before {
		border-top-style: dotted;
	}
	.fallback-key::before {
		border-top: 2px dashed var(--stage-factory);
	}
	h1 small {
		color: var(--muted);
		font-weight: 400;
	}
	.muted {
		color: var(--muted);
	}
	.small {
		font-size: 11px;
	}
	button.selected {
		border-color: var(--accent);
		background: var(--accent-soft);
	}
	.stepline {
		font-size: 12px;
		margin: 4px 0 8px;
	}
	.badge {
		padding: 1px 6px;
		border-radius: 999px;
		background: var(--chip);
		font-weight: 600;
	}
	.badge.dispatched {
		background: var(--stage-delivery);
		color: #fff;
	}
	.badge.running {
		background: var(--stage-reconcile);
		color: #fff;
	}
	.result {
		font-size: 12px;
		background: var(--accent-soft);
		padding: 6px 10px;
		border-radius: 6px;
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
	.log span {
		color: var(--muted);
	}
</style>
