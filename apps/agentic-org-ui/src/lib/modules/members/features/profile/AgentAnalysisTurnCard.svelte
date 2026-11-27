<script lang="ts">
	import type { Turn } from '../../types';

	let {
		turn,
		promptText,
		traceOpen,
		onToggleTrace
	}: {
		turn: Turn;
		promptText: string;
		traceOpen: boolean;
		onToggleTrace: () => void;
	} = $props();

	const processing = $derived(turn.state === 'running' || turn.state === 'accepted');
</script>

<article class="af-turn">
	<div class="af-msg-user">
		<div class="af-user-bubble">{promptText}</div>
	</div>
	{#if turn.summary || turn.failure}
		<div class="af-reply-text">
			{turn.summary ?? turn.failure}
		</div>
	{/if}
	<div class="af-turn-trace">
		<div class={['c-trace-wrap', traceOpen && 'is-open']}>
			<div class="c-trace-bar">
				{#if processing}
					<span class="c-thinking-cue" role="status">
						<span class="c-thinking-dots" aria-hidden="true">
							<i></i><i></i><i></i>
						</span>
						<span>Thinking…</span>
					</span>
				{/if}
				<button
					type="button"
					class="c-trace-toggle"
					aria-expanded={traceOpen}
					aria-controls="trace-{turn.id}"
					onclick={onToggleTrace}
				>
					<span class="c-trace-rule" aria-hidden="true"></span>
					<span>{traceOpen ? 'Hide trace' : 'Follow trace'}</span>
				</button>
			</div>
			{#if traceOpen}
				<div class="c-trace-panel" id="trace-{turn.id}">
					<div class="c-trace-summary">
						State: {turn.state} · Started {new Date(turn.startedAt).toLocaleTimeString()}
						{#if turn.endedAt}
							· Ended {new Date(turn.endedAt).toLocaleTimeString()}
						{/if}
					</div>
					{#if turn.attempts.length > 0}
						<div class="c-steps">
							{#each turn.attempts as att (att.n)}
								<div class="c-step">
									<span class="c-step-dot"></span>
									<span class="c-step-tool">Attempt #{att.n}</span>
									<span class="c-step-arg">{att.reason ?? att.outcome}</span>
								</div>
							{/each}
						</div>
					{:else}
						<p class="c-trace-empty">
							{processing
								? 'Live run in progress at office…'
								: 'No tool-call breakdown recorded for this session.'}
						</p>
					{/if}
				</div>
			{/if}
		</div>
	</div>
</article>

<style>
	.af-turn {
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.af-msg-user {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
	}
	.af-user-bubble {
		max-width: min(82%, 540px);
		padding: 9px 14px;
		border-radius: 13px;
		border-bottom-right-radius: 4px;
		background: var(--tone-accent-bg);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
		color: var(--exec-ink, var(--fg));
		font-size: 0.875rem;
		line-height: 1.5;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.af-reply-text {
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.9375rem;
		line-height: 1.62;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		max-width: 64ch;
	}
	.af-turn-trace {
		margin-top: -6px;
	}
	.c-trace-wrap {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.c-trace-bar {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
	}
	.c-thinking-cue {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		font-size: 0.8125rem;
		font-weight: 500;
		color: var(--exec-body, var(--fg-secondary));
	}
	.c-thinking-dots {
		display: inline-flex;
		align-items: center;
		gap: 3px;
	}
	.c-thinking-dots i {
		width: 5px;
		height: 5px;
		border-radius: 99px;
		background: var(--tone-accent, var(--brand));
		opacity: 0.35;
		animation: trace-dot 1.2s ease-in-out infinite;
	}
	.c-thinking-dots i:nth-child(2) {
		animation-delay: 0.18s;
	}
	.c-thinking-dots i:nth-child(3) {
		animation-delay: 0.36s;
	}
	@keyframes trace-dot {
		0%,
		80%,
		100% {
			opacity: 0.28;
			transform: scale(0.85);
		}
		40% {
			opacity: 1;
			transform: scale(1.15);
		}
	}
	.c-trace-toggle {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 0 4px 2px 0;
		border-radius: 4px;
		border: 0;
		background: transparent;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 500;
		opacity: 0.55;
		cursor: pointer;
		transition:
			opacity 0.16s ease,
			color 0.16s ease;
	}
	.c-trace-rule {
		display: inline-block;
		width: 10px;
		height: 1px;
		background: currentColor;
		opacity: 0.7;
		transition:
			width 0.16s ease,
			background-color 0.16s ease;
	}
	.af-turn:is(:hover, :focus-within) .c-trace-toggle {
		opacity: 0.85;
	}
	.c-trace-toggle:is(:hover, :focus-visible),
	.c-trace-wrap.is-open .c-trace-toggle {
		opacity: 1;
		color: var(--exec-ink, var(--fg));
	}
	.c-trace-wrap.is-open .c-trace-rule {
		width: 14px;
		background: var(--tone-accent-ink, var(--link));
		opacity: 1;
	}
	.c-trace-panel {
		border-radius: 8px;
		background: var(--exec-well, var(--surface-sunken));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line));
		padding: 10px 12px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.75rem;
	}
	.c-trace-summary {
		color: var(--exec-muted, var(--fg-muted));
		padding-bottom: 6px;
		border-bottom: 1px solid var(--exec-border, var(--line));
	}
	.c-steps {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.c-step {
		display: grid;
		grid-template-columns: auto auto minmax(0, 1fr);
		align-items: center;
		gap: 8px;
	}
	.c-step-dot {
		width: 5px;
		height: 5px;
		border-radius: 99px;
		background: var(--tone-accent, var(--brand));
	}
	.c-step-tool {
		color: var(--tone-accent-ink, var(--link));
		font-weight: 600;
	}
	.c-step-arg {
		color: var(--exec-muted, var(--fg-muted));
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.c-trace-empty {
		margin: 0;
		color: var(--exec-muted, var(--fg-muted));
	}
	@media (prefers-reduced-motion: reduce) {
		.c-trace-toggle,
		.c-trace-rule {
			transition: none;
		}
		.c-thinking-dots i {
			animation: none;
		}
	}
</style>
