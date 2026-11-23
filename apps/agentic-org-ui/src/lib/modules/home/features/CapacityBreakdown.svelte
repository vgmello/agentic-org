<script lang="ts">
	import { resolve } from '$app/paths';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import type { TaskTimeBreakdown } from '$lib/home/model';

	interface Props {
		taskTime: TaskTimeBreakdown;
		offlineOffices: number;
	}

	let { taskTime, offlineOffices }: Props = $props();

	const queuedHref = $derived(offlineOffices > 0 ? '/settings/offices' : '/work');
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

	<div
		class="time-bar bg-bar-track flex gap-1 overflow-hidden"
		role="group"
		aria-label="Open task distribution"
	>
		{#if taskTime.workingPct > 0}
			<a
				href={resolve('/teams/members')}
				class="tseg seg-agents"
				style:width="{taskTime.workingPct}%"
				aria-label="Agents working: {taskTime.workingPct}%"
				title="Agents working: {taskTime.workingPct}% ({taskTime.workingCount})"
			></a>
		{/if}
		{#if taskTime.reviewPct > 0}
			<a
				href={resolve('/activity')}
				class="tseg seg-ci"
				style:width="{taskTime.reviewPct}%"
				aria-label="Automated checks & review: {taskTime.reviewPct}%"
				title="Automated checks & review: {taskTime.reviewPct}% ({taskTime.reviewCount})"
			></a>
		{/if}
		{#if taskTime.waitingYouPct > 0}
			<a
				href={resolve('/inbox')}
				class="tseg seg-human"
				style:width="{taskTime.waitingYouPct}%"
				aria-label="Waiting on you: {taskTime.waitingYouPct}%"
				title="Waiting on you: {taskTime.waitingYouPct}% ({taskTime.waitingYouCount})"
			></a>
		{/if}
		{#if taskTime.queuedPct > 0}
			<a
				href={resolve(queuedHref)}
				class={['tseg', taskTime.queuedIsHigh ? 'seg-queued-high' : 'seg-queued']}
				style:width="{taskTime.queuedPct}%"
				aria-label="Waiting for an agent: {taskTime.queuedPct}%"
				title="Waiting for an agent: {taskTime.queuedPct}% ({taskTime.queuedCount})"
			></a>
		{/if}
	</div>

	<div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs">
		<a href={resolve('/teams/members')} class="tleg inline-flex items-center gap-1.5">
			<span class="size-2 rounded-full bg-[var(--tone-accent)]"></span>
			<strong class="text-[var(--exec-sky)] tabular-nums">{taskTime.workingPct}%</strong>
			<span class="text-[var(--exec-muted)]">Agents working ({taskTime.workingCount})</span>
		</a>
		<a href={resolve('/activity')} class="tleg inline-flex items-center gap-1.5">
			<span class="size-2 rounded-full bg-[var(--tone-ok)]"></span>
			<strong class="text-[var(--exec-ok)] tabular-nums">{taskTime.reviewPct}%</strong>
			<span class="text-[var(--exec-muted)]">In review ({taskTime.reviewCount})</span>
		</a>
		<a href={resolve('/inbox')} class="tleg inline-flex items-center gap-1.5">
			<span class="size-2 rounded-full bg-[var(--tone-warn)]"></span>
			<strong class="text-[var(--exec-warn)] tabular-nums">{taskTime.waitingYouPct}%</strong>
			<span class="text-[var(--exec-muted)]">Waiting on you ({taskTime.waitingYouCount})</span>
		</a>
		<a href={resolve(queuedHref)} class="tleg inline-flex items-center gap-1.5">
			<span
				class={[
					'size-2 rounded-full',
					taskTime.queuedIsHigh ? 'bg-[var(--tone-bad)]' : 'bg-[var(--tone-queued)]'
				]}
			></span>
			<strong
				class={[
					'tabular-nums',
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
				class="inline-flex items-center gap-0.5 font-semibold text-[var(--exec-accent-ink)] hover:underline"
			>
				<span>{taskTime.primaryLink.label}</span>
				<ChevronRight class="size-3.5" aria-hidden="true" />
			</a>
			{#if taskTime.secondaryLink}
				<a
					href={resolve(taskTime.secondaryLink.href as '/work')}
					class="inline-flex items-center gap-0.5 font-semibold text-[var(--exec-accent-ink)] hover:underline"
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
		transition:
			opacity 0.16s ease,
			filter 0.16s ease;
	}
	.time-bar:hover .tseg {
		opacity: 0.55;
	}
	.time-bar .tseg:hover {
		opacity: 1;
		filter: brightness(1.12);
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
</style>
