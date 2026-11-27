<!--
  @component
  Priority signal bars and status ring glyph for WorkListRow.
-->
<script lang="ts">
	import type { WorkStatusRingTone } from '$lib/modules/work/compact';
	import { PRIORITY_WORD, STATUS_WORD } from '../../present';
	import type { Priority, TaskStatus } from '../../types';

	let {
		priority,
		status,
		needsMe = false,
		variant
	}: {
		priority?: Priority;
		status?: TaskStatus;
		needsMe?: boolean;
		variant: 'priority' | 'status';
	} = $props();

	const priSig = $derived.by<{ level: 1 | 2 | 3; strong?: boolean }>(() => {
		if (priority === 'urgent') return { level: 3, strong: true };
		if (priority === 'high') return { level: 3 };
		if (priority === 'normal') return { level: 2 };
		return { level: 1 };
	});

	const tone = $derived.by<WorkStatusRingTone>(() => {
		if (status === 'done') return 'done';
		if (status === 'blocked') return 'blocked';
		if (status === 'in_progress') return 'active';
		return 'neutral';
	});
</script>

{#if variant === 'priority' && priority}
	<span
		class="work-priority-mark"
		data-priority-level={priSig.level}
		data-tone={priSig.strong ? 'strong' : undefined}
		title="{PRIORITY_WORD[priority]} priority"
		aria-hidden="true"
	>
		<i class="is-on"></i>
		<i class={priSig.level >= 2 ? 'is-on' : undefined}></i>
		<i class={priSig.level >= 3 ? 'is-on' : undefined}></i>
	</span>
{:else if variant === 'status' && status}
	<span
		class="work-status"
		title="{STATUS_WORD[status]}{needsMe ? ' · Needs you' : ''}"
		aria-hidden="true"
	>
		<span class="work-status-ring" data-tone={tone}>
			{#if status === 'done'}
				<svg
					viewBox="0 0 16 16"
					fill="none"
					stroke="currentColor"
					stroke-width="1.8"
					stroke-linecap="round"
					stroke-linejoin="round"
				>
					<circle cx="8" cy="8" r="6" />
					<path d="m5.5 8.2 1.7 1.7 3.3-3.6" />
				</svg>
			{:else if status === 'in_progress'}
				<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8">
					<circle cx="8" cy="8" r="6" />
					<path d="M8 2a6 6 0 0 1 0 12V2Z" fill="currentColor" stroke="none" />
				</svg>
			{:else if status === 'blocked'}
				<svg
					viewBox="0 0 16 16"
					fill="none"
					stroke="currentColor"
					stroke-width="1.8"
					stroke-linecap="round"
				>
					<circle cx="8" cy="8" r="6" />
					<path d="M5.5 8h5" />
				</svg>
			{:else if status === 'cancelled'}
				<svg
					viewBox="0 0 16 16"
					fill="none"
					stroke="currentColor"
					stroke-width="1.6"
					stroke-dasharray="2.5 2"
				>
					<circle cx="8" cy="8" r="6" />
				</svg>
			{:else}
				<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8">
					<circle cx="8" cy="8" r="6" />
				</svg>
			{/if}
		</span>
		{#if needsMe}
			<span class="work-attention-dot"></span>
		{/if}
	</span>
{/if}

<style>
	.work-status {
		position: relative;
		display: inline-flex;
		flex: none;
		align-items: center;
		justify-content: center;
		width: 1rem;
		height: 1rem;
	}
	.work-status-ring {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 0.9375rem;
		height: 0.9375rem;
		color: var(--exec-muted, var(--fg-muted));
	}
	.work-status-ring svg {
		width: 100%;
		height: 100%;
		display: block;
	}
	.work-status-ring[data-tone='active'] {
		color: var(--tone-accent, var(--brand));
	}
	.work-status-ring[data-tone='done'] {
		color: var(--tone-ok, var(--success));
	}
	.work-status-ring[data-tone='blocked'] {
		color: var(--tone-bad, var(--danger));
	}
	.work-attention-dot {
		position: absolute;
		inset: -1px -1px auto auto;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--tone-warn, var(--warning));
		box-shadow: 0 0 0 1.5px var(--exec-bg, var(--surface));
	}
	.work-priority-mark {
		display: inline-flex;
		flex: none;
		align-items: flex-end;
		justify-content: center;
		gap: 2px;
		width: 1rem;
		height: 1rem;
		padding-bottom: 3px;
		color: var(--exec-muted, var(--fg-muted));
	}
	.work-priority-mark[data-tone='strong'] {
		color: var(--exec-ink, var(--fg));
	}
	.work-priority-mark i {
		display: block;
		width: 2.5px;
		border-radius: 1px;
		background: currentColor;
		opacity: 0.24;
	}
	.work-priority-mark i:nth-child(1) {
		height: 4px;
	}
	.work-priority-mark i:nth-child(2) {
		height: 7px;
	}
	.work-priority-mark i:nth-child(3) {
		height: 10px;
	}
	.work-priority-mark i.is-on {
		opacity: 1;
	}
</style>
