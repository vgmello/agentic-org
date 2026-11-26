<!--
  @component
  Calm Tool-Progress Telemetry for Embedded Tess (doc-97 §6 TESS-UX-6, TASK-80.5).
  Translates SSE tool.progress events into plain product language with an in-place Stop button,
  and collapses settled calls into a single progressive-disclosure summary line.
-->
<script lang="ts">
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Square from '@lucide/svelte/icons/square';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import type { TessToolCall } from './types';

	let {
		calls,
		streaming = false,
		onCancel
	}: {
		calls: readonly TessToolCall[];
		streaming?: boolean;
		onCancel?: () => void;
	} = $props();

	let expanded = $state(false);
	let nowMs = $state(Date.now());

	const activeCall = $derived(calls.find((c) => c.state === 'running'));
	const isRunning = $derived(streaming || Boolean(activeCall));

	$effect(() => {
		if (!isRunning) return;
		const timer = setInterval(() => {
			nowMs = Date.now();
		}, 1000);
		return () => clearInterval(timer);
	});

	const totalDurationMs = $derived.by(() => {
		let sum = 0;
		for (const c of calls) {
			if (c.durationMs !== undefined) sum += c.durationMs;
		}
		return sum;
	});

	const activeLabel = $derived.by(() => {
		if (!activeCall) return 'Working on your request…';
		const elapsed = nowMs - activeCall.startedAt;
		if (elapsed >= 2000)
			return `Still ${activeCall.label.charAt(0).toLowerCase()}${activeCall.label.slice(1)}`;
		return activeCall.label;
	});

	const settledSummary = $derived.by(() => {
		const count = calls.length;
		const dur = totalDurationMs >= 100 ? ` · ${(totalDurationMs / 1000).toFixed(1)}s` : '';
		if (count === 1) return `${calls[0].label}${dur}`;
		return `Checked ${count} sources${dur}`;
	});
</script>

{#if calls.length > 0 || streaming}
	<div class="tess-tool-wrap" data-testid="tess-tool-progress">
		{#if isRunning}
			<div class="tess-tool-live" role="status">
				<LoaderCircle class="tess-tool-spin size-3.5 shrink-0" aria-hidden="true" />
				<span class="min-w-0 flex-1 truncate">{activeLabel}</span>
				{#if onCancel}
					<button
						type="button"
						onclick={onCancel}
						class="tess-stop-btn"
						aria-label="Stop Tess turn"
					>
						<Square class="size-2.5 fill-current" aria-hidden="true" />
						<span>Stop</span>
					</button>
				{/if}
			</div>
		{:else}
			<button
				type="button"
				onclick={() => (expanded = !expanded)}
				aria-expanded={expanded}
				class="tess-tool-settled"
			>
				<CheckCircle2 class="size-3.5 shrink-0 text-[var(--tone-ok-ink)]" aria-hidden="true" />
				<span class="min-w-0 flex-1 truncate text-left">{settledSummary}</span>
				<ChevronDown
					class={['size-3.5 shrink-0 transition-transform', expanded && 'rotate-180']}
					aria-hidden="true"
				/>
			</button>

			{#if expanded}
				<ul class="tess-tool-steps" role="list">
					{#each calls as call (call.id)}
						<li class="tess-tool-step">
							{#if call.state === 'error'}
								<TriangleAlert
									class="size-3 shrink-0 text-[var(--tone-bad-ink)]"
									aria-hidden="true"
								/>
							{:else}
								<CheckCircle2
									class="size-3 shrink-0 text-[var(--tone-ok-ink)]"
									aria-hidden="true"
								/>
							{/if}
							<span class="min-w-0 flex-1 truncate">{call.label}</span>
							{#if call.durationMs !== undefined}
								<span class="tess-tool-dur tabular-nums"
									>{(call.durationMs / 1000).toFixed(1)}s</span
								>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
		{/if}
	</div>
{/if}

<style>
	.tess-tool-wrap {
		display: flex;
		flex-direction: column;
		gap: 4px;
		margin-bottom: 6px;
	}
	.tess-tool-live,
	.tess-tool-settled {
		display: flex;
		align-items: center;
		gap: 7px;
		width: 100%;
		padding: 6px 10px;
		border-radius: var(--exec-sm-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-muted);
		font-size: 0.75rem;
		line-height: 1.35;
	}
	.tess-tool-settled {
		cursor: pointer;
		transition: background-color 120ms ease;
	}
	.tess-tool-settled:hover {
		background: var(--exec-surface-hi);
		color: var(--exec-ink);
	}
	.tess-tool-live :global(.tess-tool-spin) {
		color: var(--tone-accent-ink);
		animation: tess-spin 0.9s linear infinite;
	}
	.tess-stop-btn {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		padding: 2px 8px;
		border-radius: 999px;
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring);
		font-size: 0.7rem;
		font-weight: 600;
		cursor: pointer;
	}
	.tess-stop-btn:hover {
		background: var(--tone-bad);
		color: var(--exec-on-accent);
	}
	.tess-tool-steps {
		display: flex;
		flex-direction: column;
		gap: 3px;
		padding: 6px 10px;
		border-radius: var(--exec-sm-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.tess-tool-step {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 0.72rem;
		color: var(--exec-body);
	}
	.tess-tool-dur {
		color: var(--exec-dim);
		font-size: 0.68rem;
	}
	@keyframes tess-spin {
		to {
			transform: rotate(360deg);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.tess-tool-live :global(.tess-tool-spin) {
			animation: none;
		}
	}
</style>
