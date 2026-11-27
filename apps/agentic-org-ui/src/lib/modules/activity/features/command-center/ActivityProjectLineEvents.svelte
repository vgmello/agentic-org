<script lang="ts">
	import { eventTime } from '$lib/modules/preferences';
	import type { LineEventEntry } from './assembly-model';

	let {
		events,
		extraEvents = [],
		zone = 'UTC',
		nowMs = Date.now()
	}: {
		events: readonly LineEventEntry[];
		extraEvents?: readonly LineEventEntry[];
		zone?: string;
		nowMs?: number;
	} = $props();

	const nowDate = $derived(new Date(nowMs));

	const mergedLineEvents = $derived.by<LineEventEntry[]>(() => {
		const seen: Record<string, true> = {};
		const combined: LineEventEntry[] = [];
		for (const item of [...extraEvents, ...events]) {
			const dedupKey = `${item.id}|${item.at}|${item.text}`;
			if (Object.hasOwn(seen, dedupKey)) continue;
			seen[dedupKey] = true;
			combined.push(item);
		}
		combined.sort((a, b) => (Date.parse(b.at) || 0) - (Date.parse(a.at) || 0));
		return combined.slice(0, 30);
	});

	function formatLineTime(iso: string): { text: string; full: string; iso?: string } {
		const ms = Date.parse(iso);
		if (Number.isNaN(ms)) return { text: '—', full: iso, iso: undefined };
		return eventTime(new Date(ms), nowDate, zone, true);
	}
</script>

<section
	class="project-telemetry"
	aria-labelledby="proj-ops-line-events-heading"
	data-testid="project-line-events"
>
	<div class="project-telemetry-header">
		<h3 id="proj-ops-line-events-heading">Line events</h3>
		<span class="ops-section-meta tabular-nums">{mergedLineEvents.length}</span>
	</div>
	{#if mergedLineEvents.length === 0}
		<p class="ops-note">No line events recorded yet.</p>
	{:else}
		<ol class="project-line-log" role="list">
			{#each mergedLineEvents as ev (ev.id)}
				{@const when = formatLineTime(ev.at)}
				<li data-tone={ev.tone || undefined}>
					<time datetime={when.iso} title={when.full} class="project-line-time">
						{when.text}
					</time>
					<span>{ev.text}</span>
				</li>
			{/each}
		</ol>
	{/if}
</section>

<style>
	.project-telemetry {
		margin-block: 12px;
		padding: 12px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--detail-panel-surface, var(--surface-raised));
	}
	.project-telemetry-header {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 4px 8px;
		margin-bottom: 8px;
	}
	.project-telemetry-header h3 {
		margin: 0;
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 650;
	}
	.ops-section-meta {
		color: var(--fg-secondary);
		font-size: 0.75rem;
		text-align: end;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.ops-note {
		margin: 8px 0 0;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		line-height: 1.4;
	}
	.project-line-log {
		display: flex;
		flex-direction: column;
		gap: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.project-line-log li {
		display: grid;
		grid-template-columns: minmax(4.5rem, max-content) minmax(0, 1fr);
		align-items: baseline;
		gap: 8px;
		font-size: 0.75rem;
		line-height: 1.4;
		color: var(--exec-body);
	}
	.project-line-log li span {
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.project-line-time {
		color: var(--fg-secondary);
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}
	.project-line-log li[data-tone='ok'] {
		color: var(--exec-ok);
	}
	.project-line-log li[data-tone='warn'] {
		color: var(--exec-warn);
	}
	.project-line-log li[data-tone='bad'] {
		color: var(--exec-bad);
	}
</style>
