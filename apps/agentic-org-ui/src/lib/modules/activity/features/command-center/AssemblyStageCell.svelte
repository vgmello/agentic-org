<script lang="ts">
	import { resolve } from '$app/paths';
	import type { AssemblyChip, AssemblyStage } from './assembly-model';

	let {
		stage,
		chips
	}: {
		stage: AssemblyStage;
		chips: readonly AssemblyChip[];
	} = $props();
</script>

<td>
	<div class="assembly-cell">
		{#if chips.length === 0}
			<span class="assembly-dash" aria-label="None">—</span>
		{:else}
			{#each chips as chip (chip.id)}
				<a
					href={resolve(chip.href as '/work')}
					class="assembly-chip"
					data-chip-id={chip.id}
					data-stage={stage}
					title={chip.tooltip}
				>
					{#if stage === 'agents'}
						<span class="agent-pulse-dot" aria-hidden="true"></span>
						<span>
							{chip.shortId}{#if chip.agentFirstName}
								· {chip.agentFirstName}
							{/if}
						</span>
					{:else}
						{chip.shortId}
					{/if}
				</a>
			{/each}
		{/if}
	</div>
</td>

<style>
	td {
		padding: 10px 10px;
		text-align: start;
		vertical-align: top;
		border-inline-start: 1px solid var(--line);
	}
	.assembly-cell {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.assembly-chip {
		--stage-color: var(--fg-secondary);
		--stage-bg: var(--exec-well);
		--stage-ring: var(--exec-border-hi);
		position: relative;
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: 36px;
		max-width: 100%;
		padding: 4px 10px;
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--stage-bg);
		box-shadow: inset 0 0 0 1px var(--stage-ring);
		color: var(--stage-color);
		font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.75rem;
		font-weight: 550;
		line-height: 1.25;
		text-decoration: none;
		overflow-wrap: anywhere;
		transition: box-shadow 0.15s ease;
	}
	.assembly-chip:hover,
	.assembly-chip:focus-visible {
		box-shadow: inset 0 0 0 1.5px var(--stage-color);
	}
	.assembly-chip:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.assembly-chip[data-stage='agents'] {
		--stage-color: var(--tone-accent-ink);
		--stage-bg: var(--tone-accent-bg);
		--stage-ring: var(--tone-accent-ring);
	}
	.assembly-chip[data-stage='ci'] {
		--stage-color: var(--exec-sky);
		--stage-bg: var(--exec-sky-bg);
		--stage-ring: var(--exec-sky-ring);
	}
	.assembly-chip[data-stage='review'] {
		--stage-color: var(--exec-warn);
		--stage-bg: var(--exec-warn-bg);
		--stage-ring: var(--exec-warn-ring);
	}
	.assembly-chip[data-stage='merged'] {
		--stage-color: var(--exec-ok);
		--stage-bg: var(--exec-ok-bg);
		--stage-ring: var(--exec-ok-ring);
	}
	.agent-pulse-dot {
		width: 6px;
		height: 6px;
		border-radius: 999px;
		background: var(--tone-accent-ink);
		flex-shrink: 0;
		animation: assembly-pulse 1.8s ease-in-out infinite;
	}
	@keyframes assembly-pulse {
		0%,
		100% {
			opacity: 1;
		}
		50% {
			opacity: 0.35;
		}
	}
	.assembly-dash {
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		line-height: 36px;
	}
	@media (width < 40rem), (pointer: coarse) {
		.assembly-chip {
			min-height: 44px;
			padding: 6px 12px;
		}
		.assembly-dash {
			line-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.agent-pulse-dot {
			animation: none;
		}
		.assembly-chip {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.assembly-chip {
			border: 1px solid CanvasText;
		}
	}
</style>
