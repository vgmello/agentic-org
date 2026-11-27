<script lang="ts">
	import type { ThroughputDay, ThroughputSource } from './assembly-model';

	let {
		days,
		source,
		ariaLabel,
		unavailableMessage = undefined
	}: {
		days: readonly ThroughputDay[];
		source: ThroughputSource;
		ariaLabel: string;
		unavailableMessage?: string;
	} = $props();
</script>

{#if source === 'unavailable'}
	<p class="ops-note is-warn">
		{unavailableMessage ?? 'Throughput data is unavailable.'}
	</p>
{:else if source === 'loading'}
	<p class="ops-note">Loading 7-day activity…</p>
{:else}
	<ol class="ops-throughput" aria-label={ariaLabel}>
		{#each days as day (day.key)}
			<li class={['ops-day', day.isToday && 'is-today']}>
				<span class="ops-day-count tabular-nums">{day.count}</span>
				<div class="ops-day-track" aria-hidden="true">
					<span
						class="ops-day-bar"
						style:height={`${day.count > 0 ? Math.max(16, day.heightPct) : 6}%`}
						data-empty={day.count === 0 ? 'true' : undefined}
					></span>
				</div>
				<span class="ops-day-label">{day.label}</span>
			</li>
		{/each}
	</ol>
{/if}

<style>
	.ops-note {
		margin: 8px 0 0;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		line-height: 1.4;
	}

	.ops-note.is-warn {
		color: var(--exec-warn);
	}

	.ops-throughput {
		display: grid;
		grid-template-columns: repeat(7, minmax(0, 1fr));
		gap: 6px;
		margin: 8px 0 0;
		padding: 0;
		list-style: none;
	}

	.ops-day {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
	}

	.ops-day-count {
		color: var(--fg-secondary);
		font-size: 0.6875rem;
	}

	.ops-day.is-today .ops-day-count,
	.ops-day.is-today .ops-day-label {
		color: var(--tone-accent-ink);
		font-weight: 650;
	}

	.ops-day-track {
		display: flex;
		align-items: flex-end;
		width: 100%;
		height: 48px;
		padding-bottom: 2px;
		border-bottom: 1px solid var(--line);
	}

	.ops-day-bar {
		display: block;
		width: 100%;
		border-radius: 3px 3px 0 0;
		background: var(--tone-accent);
	}

	.ops-day-bar[data-empty='true'] {
		background: var(--bar-track);
	}

	.ops-day-label {
		color: var(--fg-secondary);
		font-size: 0.625rem;
		white-space: nowrap;
	}
</style>
