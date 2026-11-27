<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Calendar from '@lucide/svelte/icons/calendar';
	import SectionHead from '$lib/components/SectionHead.svelte';
	import type { HomeMilestone } from '$lib/modules/home/model';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';

	interface Props {
		milestones: readonly HomeMilestone[];
		spotlightIds?: ReadonlySet<string>;
		onSpotJump?: (decisionId: string) => void;
	}

	let { milestones, spotlightIds, onSpotJump }: Props = $props();

	const isReport = $derived(uiPrefs.theme === 'report' || uiPrefs.theme === 'report-dark');
</script>

<section class="horizon-sec flex flex-col gap-3" aria-labelledby="hb-hor-h">
	<SectionHead
		id="hb-hor-h"
		title="Upcoming"
		reportTitle="Upcoming Milestones · Next 14 Days"
		count={milestones.length > 0 ? `${milestones.length} scheduled` : 'Next 14 days'}
	>
		{#snippet action()}
			<button
				type="button"
				onclick={() => toast.info('Calendar view is coming in a future release.')}
				class="hor-cal-btn inline-flex items-center gap-1.5 text-xs font-semibold"
			>
				<Calendar class="size-3.5" aria-hidden="true" />
				<span>View calendar</span>
			</button>
		{/snippet}
	</SectionHead>

	<div class={['horizon-panel flex flex-col gap-3', isReport && 'is-report']}>
		{#if milestones.length === 0}
			<p class="hor-empty text-xs text-[var(--exec-muted)]">
				No target dates or upcoming deadlines scheduled in the next 14 days.
			</p>
		{:else}
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
											<span class="rel-tag shrink-0 font-mono text-xs">{m.releaseTag}</span>
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
											<span class="rel-tag shrink-0 font-mono text-xs">{m.releaseTag}</span>
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
		{/if}
	</div>
</section>

<style>
	.horizon-sec {
		padding: var(--exec-section-pad-y);
		border-bottom: var(--exec-section-border);
	}
	.horizon-panel {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-horizon-pad);
	}
	.horizon-panel.is-report {
		padding: 4px 0 0;
	}
	.hor-cal-btn {
		color: var(--exec-accent-ink);
		cursor: pointer;
	}
	.hor-cal-btn:hover {
		text-decoration: underline;
	}
	.hor-empty {
		padding: 10px 12px;
		border-radius: var(--exec-inset-radius);
		background: var(--exec-hor-item-bg);
		box-shadow: inset 0 0 0 1px var(--exec-border);
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
	@media (prefers-reduced-motion: reduce) {
		.hor-item {
			transition: none;
		}
	}
</style>
