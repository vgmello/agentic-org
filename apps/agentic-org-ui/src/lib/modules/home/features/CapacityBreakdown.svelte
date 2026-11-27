<script lang="ts">
	import { resolve } from '$app/paths';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import type { TaskTimeBreakdown } from '$lib/modules/home/model';

	interface Props {
		taskTime: TaskTimeBreakdown;
		offlineOffices: number;
	}

	let { taskTime, offlineOffices }: Props = $props();

	const queuedHref = $derived(offlineOffices > 0 ? '/settings/offices' : '/work');
	const barLabel = $derived(
		`Open task distribution: ${taskTime.workingPct}% agents working, ${taskTime.reviewPct}% in review, ${taskTime.waitingYouPct}% waiting on you, ${taskTime.queuedPct}% waiting for an agent`
	);
</script>

<section class="time-panel flex flex-col gap-3" aria-labelledby="exec-time-heading">
	<div class="flex flex-wrap items-baseline justify-between gap-2">
		<h2 id="exec-time-heading" class="time-title">
			<span class="exec-lbl-std">Where task time goes</span>
			<span class="exec-lbl-rep">Capacity &amp; Bottleneck Analysis</span>
		</h2>
		<span class="time-sub tabular-nums">
			{taskTime.totalOpen} open {taskTime.totalOpen === 1 ? 'task' : 'tasks'}
		</span>
	</div>

	<div class="time-bar bg-bar-track flex gap-1 overflow-hidden" role="img" aria-label={barLabel}>
		{#if taskTime.workingPct > 0}
			<div
				class="tseg seg-agents"
				style:width="{taskTime.workingPct}%"
				aria-hidden="true"
				title="Agents working: {taskTime.workingPct}% ({taskTime.workingCount})"
			></div>
		{/if}
		{#if taskTime.reviewPct > 0}
			<div
				class="tseg seg-ci"
				style:width="{taskTime.reviewPct}%"
				aria-hidden="true"
				title="In review: {taskTime.reviewPct}% ({taskTime.reviewCount})"
			></div>
		{/if}
		{#if taskTime.waitingYouPct > 0}
			<div
				class="tseg seg-human"
				style:width="{taskTime.waitingYouPct}%"
				aria-hidden="true"
				title="Waiting on you: {taskTime.waitingYouPct}% ({taskTime.waitingYouCount})"
			></div>
		{/if}
		{#if taskTime.queuedPct > 0}
			<div
				class={['tseg', taskTime.queuedIsHigh ? 'seg-queued-high' : 'seg-queued']}
				style:width="{taskTime.queuedPct}%"
				aria-hidden="true"
				title="Waiting for an agent: {taskTime.queuedPct}% ({taskTime.queuedCount})"
			></div>
		{/if}
	</div>

	<div class="flex flex-wrap items-center gap-x-5 gap-y-1 text-[0.8125rem]">
		<a
			href={resolve('/teams/members')}
			class="tleg inline-flex min-h-9 items-center gap-1.5 rounded-md px-1"
		>
			<span class="size-2 rounded-full bg-[var(--tone-accent)]"></span>
			<strong class="text-[0.9375rem] font-bold text-[var(--exec-sky)] tabular-nums"
				>{taskTime.workingPct}%</strong
			>
			<span class="text-[var(--exec-muted)]">Agents working ({taskTime.workingCount})</span>
		</a>
		<a
			href={resolve('/activity')}
			class="tleg inline-flex min-h-9 items-center gap-1.5 rounded-md px-1"
		>
			<span class="size-2 rounded-full bg-[var(--tone-ok)]"></span>
			<strong class="text-[0.9375rem] font-bold text-[var(--exec-ok)] tabular-nums"
				>{taskTime.reviewPct}%</strong
			>
			<span class="text-[var(--exec-muted)]">In review ({taskTime.reviewCount})</span>
		</a>
		<a
			href={resolve('/inbox')}
			class="tleg inline-flex min-h-9 items-center gap-1.5 rounded-md px-1"
		>
			<span class="size-2 rounded-full bg-[var(--tone-warn)]"></span>
			<strong class="text-[0.9375rem] font-bold text-[var(--exec-warn)] tabular-nums"
				>{taskTime.waitingYouPct}%</strong
			>
			<span class="text-[var(--exec-muted)]">Waiting on you ({taskTime.waitingYouCount})</span>
		</a>
		<a
			href={resolve(queuedHref)}
			class="tleg inline-flex min-h-9 items-center gap-1.5 rounded-md px-1"
		>
			<span
				class={[
					'size-2 rounded-full',
					taskTime.queuedIsHigh ? 'bg-[var(--tone-bad)]' : 'bg-[var(--tone-queued)]'
				]}
			></span>
			<strong
				class={[
					'text-[0.9375rem] font-bold tabular-nums',
					taskTime.queuedIsHigh ? 'text-[var(--exec-bad)]' : 'text-[var(--exec-purple)]'
				]}
			>
				{taskTime.queuedPct}%
			</strong>
			<span class="text-[var(--exec-muted)]">
				Waiting for an agent ({taskTime.queuedCount})
			</span>
		</a>
	</div>

	<div class="flex flex-wrap items-center justify-between gap-2 text-xs">
		<p class="text-[var(--exec-muted)]">{taskTime.summary}</p>
		<div class="flex flex-wrap items-center gap-3.5">
			<a
				href={resolve(taskTime.primaryLink.href as '/work')}
				class="time-act inline-flex min-h-9 items-center gap-0.5 font-semibold text-[var(--exec-accent-ink)] hover:underline"
			>
				<span>{taskTime.primaryLink.label}</span>
				<ChevronRight class="size-3.5" aria-hidden="true" />
			</a>
			{#if taskTime.secondaryLink}
				<a
					href={resolve(taskTime.secondaryLink.href as '/work')}
					class="time-act inline-flex min-h-9 items-center gap-0.5 font-semibold text-[var(--exec-accent-ink)] hover:underline"
				>
					<span>{taskTime.secondaryLink.label}</span>
					<ChevronRight class="size-3.5" aria-hidden="true" />
				</a>
			{/if}
		</div>
	</div>
</section>

<style>
	.time-panel {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-time-pad);
	}
	.time-panel:hover {
		background: var(--exec-panel-bg-hover);
		box-shadow: var(--exec-panel-shadow-hover);
	}
	.time-title {
		font-size: var(--exec-capacity-title-size);
		line-height: var(--exec-capacity-title-leading, 1.3);
		font-weight: var(--exec-capacity-title-weight);
		letter-spacing: var(--exec-capacity-title-tracking);
		color: var(--exec-ink);
	}
	.time-sub {
		font-size: var(--exec-capacity-sub-size);
		color: var(--exec-dim);
	}
	.time-bar {
		height: var(--exec-time-bar-height);
		border-radius: var(--exec-time-bar-radius);
	}
	.tseg {
		display: block;
		height: 100%;
		border-radius: var(--exec-time-bar-radius);
	}
	.tleg:hover span:last-child {
		color: var(--exec-ink);
	}
	.seg-agents {
		background: var(--exec-seg-agents);
	}
	.seg-ci {
		background: var(--exec-seg-ci);
	}
	.seg-human {
		background: var(--exec-seg-human);
	}
	.seg-queued {
		background: var(--exec-seg-queued);
	}
	.seg-queued-high {
		background: var(--exec-seg-queued-high);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.tleg,
		.time-act {
			min-height: 44px;
		}
	}
</style>
