<script lang="ts">
	import { resolve } from '$app/paths';
	import type { HomeMilestone } from '$lib/home/model';

	interface Props {
		milestones: readonly HomeMilestone[];
		spotlightIds?: ReadonlySet<string>;
		onSpotJump?: (decisionId: string) => void;
	}

	let { milestones, spotlightIds, onSpotJump }: Props = $props();
</script>

{#if milestones.length > 0}
	<section class="horizon-panel flex flex-col gap-3" aria-labelledby="hb-hor-h">
		<div class="flex flex-wrap items-baseline justify-between gap-2">
			<h2 id="hb-hor-h" class="hor-title">Upcoming milestones · Next 14 days</h2>
			<span class="hor-sub">Key deliverables &amp; sign-off checkpoints</span>
		</div>

		<div class="hor-grid grid grid-cols-1 gap-2.5 md:grid-cols-2 lg:grid-cols-3">
			{#each milestones as m (m.id)}
				{@const canJump = Boolean(m.spotId && spotlightIds?.has(m.spotId) && onSpotJump)}
				{#if canJump}
					<button
						type="button"
						data-spot-jump={m.spotId}
						onclick={() => onSpotJump?.(m.spotId!)}
						class="hor-item flex min-h-14 items-center justify-between gap-2.5 text-left"
					>
						<div class="flex min-w-0 items-center gap-2.5">
							<div class="hor-date flex w-11 shrink-0 flex-col items-center justify-center">
								<span class="hor-month">{m.month}</span>
								<strong class="hor-day">{m.day}</strong>
							</div>
							<div class="flex min-w-0 flex-col gap-0.5">
								<span class="hor-name inline-flex items-center gap-1.5">
									<span class="truncate">{m.title}</span>
									{#if m.releaseTag}
										<span class="rel-tag shrink-0 font-mono text-[11px]">{m.releaseTag}</span>
									{/if}
								</span>
								<span class="hor-note truncate">{m.note}</span>
							</div>
						</div>
						<span class={['hor-pill shrink-0', `is-${m.tone}`]}>{m.statusLabel}</span>
					</button>
				{:else}
					<a
						href={resolve(m.href as '/projects')}
						class="hor-item flex min-h-14 items-center justify-between gap-2.5 text-left"
					>
						<div class="flex min-w-0 items-center gap-2.5">
							<div class="hor-date flex w-11 shrink-0 flex-col items-center justify-center">
								<span class="hor-month">{m.month}</span>
								<strong class="hor-day">{m.day}</strong>
							</div>
							<div class="flex min-w-0 flex-col gap-0.5">
								<span class="hor-name inline-flex items-center gap-1.5">
									<span class="truncate">{m.title}</span>
									{#if m.releaseTag}
										<span class="rel-tag shrink-0 font-mono text-[11px]">{m.releaseTag}</span>
									{/if}
								</span>
								<span class="hor-note truncate">{m.note}</span>
							</div>
						</div>
						<span class={['hor-pill shrink-0', `is-${m.tone}`]}>{m.statusLabel}</span>
					</a>
				{/if}
			{/each}
		</div>
	</section>
{/if}

<style>
	.horizon-panel {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-horizon-pad);
		border-bottom: var(--exec-section-border);
	}
	.hor-title {
		font-size: 0.72rem;
		line-height: 1rem;
		font-weight: 650;
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: var(--exec-horizon-title-ink);
	}
	.hor-sub {
		font-size: 0.75rem;
		color: var(--exec-dim);
	}
	.hor-item {
		padding: 10px 12px;
		border-radius: var(--exec-inset-radius);
		background: var(--exec-hor-item-bg);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		transition:
			background-color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.hor-item:hover {
		background: var(--exec-hor-item-bg-hover);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.hor-date {
		padding: 4px 0;
		border-radius: var(--exec-sm-radius);
		background: var(--exec-hor-date-bg);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		line-height: 1.1;
		font-variant-numeric: tabular-nums;
	}
	.hor-month {
		font-size: 0.625rem;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--exec-hor-date-month-ink);
	}
	.hor-day {
		font-size: 0.875rem;
		font-weight: 700;
		color: var(--exec-hor-date-day-ink);
	}
	.hor-name {
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-ink);
		max-width: 100%;
	}
	.rel-tag {
		padding: 1px 6px;
		border-radius: 5px;
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.hor-note {
		font-size: 0.72rem;
		color: var(--exec-muted);
	}
	.hor-pill {
		padding: 2px 8px;
		border-radius: var(--exec-pill-radius);
		font-size: 0.7rem;
		font-weight: 600;
		white-space: nowrap;
	}
	.hor-pill.is-ok {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: var(--exec-pill-ring-ok);
	}
	.hor-pill.is-warn {
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: var(--exec-pill-ring-warn);
	}
	.hor-pill.is-bad {
		background: var(--exec-bad-bg);
		color: var(--exec-bad);
		box-shadow: var(--exec-pill-ring-bad);
	}
	.hor-pill.is-sky {
		background: var(--exec-sky-bg);
		color: var(--exec-sky);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
	}
</style>
