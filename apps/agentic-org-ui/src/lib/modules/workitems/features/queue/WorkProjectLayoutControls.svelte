<!--
  @component
  Project Work header controls (List/Board toggle + 4 stages/6 statuses) registered into ProjectFrame.
-->
<script lang="ts">
	import type { LayoutMode } from '../../filters';
	import type { BoardMode } from '../../present';

	let {
		layout,
		boardMode,
		onLayoutChange,
		onBoardModeChange
	}: {
		layout: LayoutMode;
		boardMode: BoardMode;
		onLayoutChange: (next: LayoutMode) => void;
		onBoardModeChange: (next: BoardMode) => void;
	} = $props();
</script>

<div
	class="project-work-controls"
	role="group"
	aria-label="Work view controls"
	data-testid="project-work-controls"
>
	<div class="work-layout-toggle" role="group" aria-label="Layout">
		<button
			type="button"
			aria-pressed={layout === 'list'}
			onclick={() => onLayoutChange('list')}
			class="queue-tab work-toggle-tab min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
			>List</button
		>
		<button
			type="button"
			aria-pressed={layout === 'board'}
			onclick={() => onLayoutChange('board')}
			class="queue-tab work-toggle-tab min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
			>Board</button
		>
	</div>
	{#if layout === 'board'}
		<div
			role="group"
			aria-label="Board columns"
			data-testid="board-mode-toggle"
			class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
		>
			<button
				type="button"
				aria-pressed={boardMode === 'stages'}
				onclick={() => onBoardModeChange('stages')}
				class="rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {boardMode ===
				'stages'
					? 'bg-surface-overlay text-fg'
					: 'text-fg-secondary hover:text-fg'}">4 stages</button
			>
			<button
				type="button"
				aria-pressed={boardMode === 'statuses'}
				onclick={() => onBoardModeChange('statuses')}
				class="rounded-sm px-2.5 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11 [@media(width<=720px)]:min-h-11 {boardMode ===
				'statuses'
					? 'bg-surface-overlay text-fg'
					: 'text-fg-secondary hover:text-fg'}">6 statuses</button
			>
		</div>
	{/if}
</div>

<style>
	.project-work-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		max-inline-size: 100%;
	}
	.work-layout-toggle {
		display: flex;
		align-items: center;
		gap: 2px;
		padding-inline-start: 8px;
		border-inline-start: 1px solid var(--exec-border, var(--line));
	}
	.work-toggle-tab {
		display: inline-flex;
		align-items: center;
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
	}
	.work-toggle-tab:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.work-toggle-tab[aria-pressed='true'] {
		background: var(--workspace-mode-selected, var(--exec-well, var(--surface-raised)));
		border-color: var(--exec-border-hi, var(--line-strong));
		color: var(--exec-ink, var(--fg));
	}
	.work-toggle-tab:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -4px;
	}

	@media (forced-colors: active) {
		.work-toggle-tab[aria-pressed='true'] {
			background: Highlight;
			border-color: Highlight;
			color: HighlightText;
		}
	}
</style>
