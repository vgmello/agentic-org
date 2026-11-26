<!--
  @component
  Native Entity Mini-Rows & Metrics Blocks for Embedded Tess (doc-97 §6 TESS-UX-7, TASK-80.5).
  Navigates <main> cleanly while keeping the Tess panel open.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import type { TessEntityRow, TessMetricCell } from './types';

	let {
		title,
		items = [],
		metrics = [],
		onNavigate
	}: {
		title?: string;
		items?: readonly TessEntityRow[];
		metrics?: readonly TessMetricCell[];
		onNavigate?: () => void;
	} = $props();
</script>

<div class="tess-artifact" data-testid="tess-entity-block">
	{#if title}
		<div class="tess-artifact-title">{title}</div>
	{/if}

	{#if metrics.length > 0}
		<dl class="tess-metrics-grid">
			{#each metrics as m (m.label)}
				<div class="tess-metric-cell">
					<dt class="tess-metric-lbl">{m.label}</dt>
					<dd class={['tess-metric-val tabular-nums', m.tone && `tone-${m.tone}`]}>{m.value}</dd>
					{#if m.note}
						<dd class="tess-metric-note">{m.note}</dd>
					{/if}
				</div>
			{/each}
		</dl>
	{/if}

	{#if items.length > 0}
		<ul class="tess-entity-list" role="list">
			{#each items as row, idx (`${row.id ?? row.ref ?? row.title}-${idx}`)}
				<li>
					{#if row.href}
						<a
							href={resolve(row.href as '/')}
							onclick={() => onNavigate?.()}
							class="tess-entity-row is-link"
						>
							<span class="min-w-0 flex-1">
								<span class="flex items-center gap-1.5">
									{#if row.ref}
										<span class="tess-entity-ref">{row.ref}</span>
									{/if}
									<span class="tess-entity-title truncate">{row.title}</span>
								</span>
								{#if row.subtitle}
									<span class="tess-entity-sub block truncate">{row.subtitle}</span>
								{/if}
							</span>
							{#if row.badge}
								<span class={['tess-entity-badge', `tone-${row.tone ?? 'muted'}`]}>
									{row.badge}
								</span>
							{/if}
							<ChevronRight class="size-3.5 shrink-0 opacity-60" aria-hidden="true" />
						</a>
					{:else}
						<div class="tess-entity-row">
							<span class="min-w-0 flex-1">
								<span class="flex items-center gap-1.5">
									{#if row.ref}
										<span class="tess-entity-ref">{row.ref}</span>
									{/if}
									<span class="tess-entity-title truncate">{row.title}</span>
								</span>
								{#if row.subtitle}
									<span class="tess-entity-sub block truncate">{row.subtitle}</span>
								{/if}
							</span>
							{#if row.badge}
								<span class={['tess-entity-badge', `tone-${row.tone ?? 'muted'}`]}>
									{row.badge}
								</span>
							{/if}
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.tess-artifact {
		display: flex;
		flex-direction: column;
		gap: 6px;
		margin-top: 8px;
	}
	.tess-artifact-title {
		font-size: 0.7rem;
		font-weight: 650;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.tess-metrics-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
		gap: 6px;
	}
	.tess-metric-cell {
		padding: 8px 10px;
		border-radius: var(--exec-sm-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.tess-metric-lbl {
		font-size: 0.68rem;
		font-weight: 600;
		color: var(--exec-muted);
	}
	.tess-metric-val {
		font-size: 0.9375rem;
		font-weight: 700;
		color: var(--exec-ink);
	}
	.tess-metric-val.tone-ok {
		color: var(--tone-ok-ink);
	}
	.tess-metric-val.tone-warn {
		color: var(--tone-warn-ink);
	}
	.tess-metric-val.tone-bad {
		color: var(--tone-bad-ink);
	}
	.tess-metric-val.tone-accent {
		color: var(--tone-accent-ink);
	}
	.tess-metric-note {
		font-size: 0.68rem;
		color: var(--exec-dim);
	}
	.tess-entity-list {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.tess-entity-row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 7px 10px;
		border-radius: var(--exec-sm-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-body);
		text-decoration: none;
	}
	.tess-entity-row.is-link:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.tess-entity-ref {
		font-family: var(--font-mono);
		font-size: 0.7rem;
		color: var(--exec-muted);
		flex: none;
	}
	.tess-entity-title {
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-ink);
	}
	.tess-entity-sub {
		font-size: 0.72rem;
		color: var(--exec-muted);
	}
	.tess-entity-badge {
		flex: none;
		padding: 1px 7px;
		border-radius: 999px;
		font-size: 0.68rem;
		font-weight: 600;
	}
	.tess-entity-badge.tone-ok {
		background: var(--tone-ok-bg);
		color: var(--tone-ok-ink);
	}
	.tess-entity-badge.tone-warn {
		background: var(--tone-warn-bg);
		color: var(--tone-warn-ink);
	}
	.tess-entity-badge.tone-bad {
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
	}
	.tess-entity-badge.tone-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
	}
	.tess-entity-badge.tone-muted {
		background: var(--exec-surface);
		color: var(--exec-muted);
	}
</style>
