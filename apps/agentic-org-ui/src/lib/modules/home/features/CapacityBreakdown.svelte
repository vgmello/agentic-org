<script lang="ts">
	import { resolve } from '$app/paths';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import SectionHead from '$lib/components/SectionHead.svelte';
	import type { TaskTimeBreakdown } from '$lib/modules/home/model';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';

	interface Props {
		taskTime: TaskTimeBreakdown;
		offlineOffices: number;
	}

	let { taskTime, offlineOffices }: Props = $props();

	const isReport = $derived(uiPrefs.theme === 'report' || uiPrefs.theme === 'report-dark');
	const queuedHref = $derived(offlineOffices > 0 ? '/settings/offices' : '/work');
	const headCount = $derived(
		`${taskTime.totalOpen} open ${taskTime.totalOpen === 1 ? 'task' : 'tasks'}`
	);
</script>

<section class="time-sec flex flex-col gap-3" aria-labelledby="exec-time-heading">
	<SectionHead
		id="exec-time-heading"
		title="Where task time goes"
		reportTitle="Capacity & Bottleneck Analysis"
		count={headCount}
	>
		{#snippet action()}
			<a
				href={resolve('/insights')}
				class="inline-flex items-center gap-0.5 text-xs font-semibold text-[var(--exec-accent-ink)] hover:underline"
			>
				<span>Insights</span>
				<ChevronRight class="size-3.5" aria-hidden="true" />
			</a>
		{/snippet}
	</SectionHead>

	<div class={['time-panel flex flex-col gap-3', isReport && 'is-report']}>
		<div
			class="time-bar bg-bar-track flex gap-1 overflow-hidden"
			role="group"
			aria-label="Task time breakdown across open tasks"
		>
			{#if taskTime.workingPct > 0}
				<a
					href={resolve('/teams/members')}
					class="tseg seg-agents focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					style:width="{taskTime.workingPct}%"
					aria-label="Agents working: {taskTime.workingPct}% ({taskTime.workingCount} {taskTime.workingCount ===
					1
						? 'task'
						: 'tasks'})"
					title="Agents working: {taskTime.workingPct}% ({taskTime.workingCount})"
				></a>
			{/if}
			{#if taskTime.reviewPct > 0}
				<a
					href={resolve('/activity')}
					class="tseg seg-ci focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					style:width="{taskTime.reviewPct}%"
					aria-label="In review: {taskTime.reviewPct}% ({taskTime.reviewCount} {taskTime.reviewCount ===
					1
						? 'task'
						: 'tasks'})"
					title="In review: {taskTime.reviewPct}% ({taskTime.reviewCount})"
				></a>
			{/if}
			{#if taskTime.waitingYouPct > 0}
				<a
					href={resolve('/inbox')}
					class="tseg seg-human focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					style:width="{taskTime.waitingYouPct}%"
					aria-label="Waiting on you: {taskTime.waitingYouPct}% ({taskTime.waitingYouCount} {taskTime.waitingYouCount ===
					1
						? 'task'
						: 'tasks'})"
					title="Waiting on you: {taskTime.waitingYouPct}% ({taskTime.waitingYouCount})"
				></a>
			{/if}
			{#if taskTime.queuedPct > 0}
				<a
					href={resolve(queuedHref)}
					class={[
						'tseg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus',
						taskTime.queuedIsHigh ? 'seg-queued-high' : 'seg-queued'
					]}
					style:width="{taskTime.queuedPct}%"
					aria-label="Waiting for an agent: {taskTime.queuedPct}% ({taskTime.queuedCount} {taskTime.queuedCount ===
					1
						? 'task'
						: 'tasks'})"
					title="Waiting for an agent: {taskTime.queuedPct}% ({taskTime.queuedCount})"
				></a>
			{/if}
		</div>

		<div class="flex flex-wrap items-center gap-x-5 gap-y-1 text-[0.8125rem]">
			<a
				href={resolve('/teams/members')}
				class="tleg inline-flex min-h-9 items-center gap-1.5 rounded-md px-1"
			>
				<span class="size-2 rounded-full bg-[var(--tone-accent)]"></span>
				<strong class="text-[0.9375rem] font-bold text-[var(--exec-sky)] tabular-nums">
					{taskTime.workingPct}%
				</strong>
				<span class="text-[var(--exec-muted)]">Agents working ({taskTime.workingCount})</span>
			</a>
			<a
				href={resolve('/activity')}
				class="tleg inline-flex min-h-9 items-center gap-1.5 rounded-md px-1"
			>
				<span class="size-2 rounded-full bg-[var(--tone-ok)]"></span>
				<strong class="text-[0.9375rem] font-bold text-[var(--exec-ok)] tabular-nums">
					{taskTime.reviewPct}%
				</strong>
				<span class="text-[var(--exec-muted)]">In review ({taskTime.reviewCount})</span>
			</a>
			<a
				href={resolve('/inbox')}
				class="tleg inline-flex min-h-9 items-center gap-1.5 rounded-md px-1"
			>
				<span class="size-2 rounded-full bg-[var(--tone-warn)]"></span>
				<strong class="text-[0.9375rem] font-bold text-[var(--exec-warn)] tabular-nums">
					{taskTime.waitingYouPct}%
				</strong>
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
	</div>
</section>

<style>
	.time-sec {
		padding: var(--exec-section-pad-y);
	}
	.time-panel {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-time-pad);
	}
	.time-panel.is-report {
		padding: 8px 0 0;
	}
	.time-panel:hover {
		background: var(--exec-panel-bg-hover);
		box-shadow: var(--exec-panel-shadow-hover);
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
		opacity: 0.6;
	}
	.time-bar .tseg:hover,
	.time-bar .tseg:focus-visible {
		opacity: 1;
		filter: brightness(1.1);
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
	@media (prefers-reduced-motion: reduce) {
		.tseg {
			transition: none;
		}
	}
</style>
