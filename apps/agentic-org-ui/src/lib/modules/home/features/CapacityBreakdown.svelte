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
		<h2 id="exec-time-heading" class="text-[15px] font-semibold text-[var(--exec-ink)]">
			Where task time goes
		</h2>
		<span class="text-xs text-[var(--exec-dim)] tabular-nums">
			{taskTime.totalOpen} open {taskTime.totalOpen === 1 ? 'item' : 'items'}
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
				class="seg-agents"
				style:width="{taskTime.workingPct}%"
				aria-label="Agents working: {taskTime.workingPct}%"
			></a>
		{/if}
		{#if taskTime.reviewPct > 0}
			<a
				href={resolve('/work')}
				class="seg-ci"
				style:width="{taskTime.reviewPct}%"
				aria-label="In review: {taskTime.reviewPct}%"
			></a>
		{/if}
		{#if taskTime.waitingYouPct > 0}
			<a
				href={resolve('/inbox')}
				class="seg-human"
				style:width="{taskTime.waitingYouPct}%"
				aria-label="Waiting on you: {taskTime.waitingYouPct}%"
			></a>
		{/if}
		{#if taskTime.queuedPct > 0}
			<a
				href={resolve(queuedHref)}
				class={taskTime.queuedIsHigh ? 'seg-queued-high' : 'seg-queued'}
				style:width="{taskTime.queuedPct}%"
				aria-label="Waiting for an agent: {taskTime.queuedPct}%"
			></a>
		{/if}
	</div>

	<div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs">
		<a href={resolve('/teams/members')} class="inline-flex items-center gap-1.5">
			<span class="size-2 rounded-full bg-[var(--tone-accent)]"></span>
			<strong class="text-[var(--exec-sky)]">{taskTime.workingPct}%</strong>
			<span class="text-[var(--exec-muted)]">Agents working ({taskTime.workingCount})</span>
		</a>
		<a href={resolve('/work')} class="inline-flex items-center gap-1.5">
			<span class="size-2 rounded-full bg-[var(--tone-ok)]"></span>
			<strong class="text-[var(--exec-ok)]">{taskTime.reviewPct}%</strong>
			<span class="text-[var(--exec-muted)]">In review ({taskTime.reviewCount})</span>
		</a>
		<a href={resolve('/inbox')} class="inline-flex items-center gap-1.5">
			<span class="size-2 rounded-full bg-[var(--tone-warn)]"></span>
			<strong class="text-[var(--exec-warn)]">{taskTime.waitingYouPct}%</strong>
			<span class="text-[var(--exec-muted)]">Waiting on you ({taskTime.waitingYouCount})</span>
		</a>
		<a href={resolve(queuedHref)} class="inline-flex items-center gap-1.5">
			<span
				class={[
					'size-2 rounded-full',
					taskTime.queuedIsHigh ? 'bg-[var(--tone-bad)]' : 'bg-[var(--tone-queued)]'
				]}
			></span>
			<strong
				class={taskTime.queuedIsHigh ? 'text-[var(--exec-bad)]' : 'text-[var(--exec-purple)]'}
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
		<div class="flex items-center gap-3.5">
			<a
				href={resolve(taskTime.primaryLink.href as '/work')}
				class="inline-flex items-center gap-0.5 font-semibold text-[var(--exec-accent-ink)]"
			>
				<span>{taskTime.primaryLink.label}</span>
				<ChevronRight class="size-3.5" aria-hidden="true" />
			</a>
			{#if taskTime.secondaryLink}
				<a
					href={resolve(taskTime.secondaryLink.href as '/work')}
					class="inline-flex items-center gap-0.5 font-semibold text-[var(--exec-accent-ink)]"
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
		backdrop-filter: var(--exec-panel-blur);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-time-pad);
	}
	.time-panel:hover {
		background: var(--exec-panel-bg-hover);
		box-shadow: var(--exec-panel-shadow-hover);
	}
	.time-bar {
		height: var(--exec-time-bar-height);
		border-radius: var(--exec-time-bar-radius);
	}
	.time-bar > a {
		border-radius: var(--exec-time-bar-radius);
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
