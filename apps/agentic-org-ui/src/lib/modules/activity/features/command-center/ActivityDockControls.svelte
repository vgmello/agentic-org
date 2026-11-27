<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Flag from '@lucide/svelte/icons/flag';

	let {
		panelOpen,
		toolsBtnEl = $bindable<HTMLButtonElement | null>(null),
		mode,
		onClose = undefined,
		onToggle = undefined
	}: {
		panelOpen: boolean;
		toolsBtnEl?: HTMLButtonElement | null;
		mode: 'header' | 'rail';
		onClose?: () => void;
		onToggle?: () => void;
	} = $props();
</script>

{#if mode === 'header'}
	<div class="side-panel-header">
		<h2>Release gates</h2>
		<Tooltip.Root delayDuration={250} ignoreNonKeyboardFocus>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						aria-label="Hide release gates"
						onclick={onClose}
						class="activity-tool-btn min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
					>
						<ChevronRight class="size-4" aria-hidden="true" />
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					side="left"
					sideOffset={6}
					escapeKeydownBehavior="defer-otherwise-close"
					class="activity-tooltip-content"
				>
					Hide release gates
				</Tooltip.Content>
			</Tooltip.Portal>
		</Tooltip.Root>
	</div>
{:else}
	<div class="activity-tools">
		<Tooltip.Root delayDuration={250} ignoreNonKeyboardFocus>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						bind:this={toolsBtnEl}
						type="button"
						aria-expanded={panelOpen}
						aria-controls="activity-release-panel"
						aria-label={panelOpen ? 'Hide release gates' : 'Show release gates'}
						data-testid="activity-gates-toggle"
						onclick={onToggle}
						class="activity-tool-btn min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
					>
						<Flag class="size-4" aria-hidden="true" />
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					side="left"
					sideOffset={6}
					escapeKeydownBehavior="defer-otherwise-close"
					class="activity-tooltip-content"
				>
					{panelOpen ? 'Hide release gates' : 'Show release gates'}
				</Tooltip.Content>
			</Tooltip.Portal>
		</Tooltip.Root>
	</div>
{/if}

<style>
	.side-panel-header {
		position: sticky;
		inset-block-start: 0;
		z-index: 1;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 8px var(--activity-panel-padding);
		border-block-end: 1px solid var(--line);
		background: var(--side-panel-surface, var(--exec-surface));
	}
	.side-panel-header h2 {
		margin: 0;
		color: var(--exec-ink);
		font-size: 0.9375rem;
		font-weight: 650;
	}
	.activity-tools {
		grid-area: tools;
		z-index: 2;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
		padding-block: 8px;
		border-inline-start: 1px solid var(--line);
		background: var(--side-panel-surface, var(--exec-surface));
	}
	.activity-tool-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--fg-secondary);
		cursor: pointer;
	}
	.activity-tool-btn:hover {
		background: var(--exec-well);
		color: var(--exec-ink);
	}
	.activity-tool-btn[aria-expanded='true'] {
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--line-strong);
	}
	.activity-tool-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	@media (forced-colors: active) {
		.activity-tool-btn[aria-expanded='true'] {
			border: 1px solid CanvasText;
		}
	}
</style>
