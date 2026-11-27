<!--
  @component
  Tinted Work board lane (TASK-194 / TASK-229): sticky header with count pill, single Done history popover
  (Last 7 days / Last 30 days / All done), and dropzone styling.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import Check from '@lucide/svelte/icons/check';
	import History from '@lucide/svelte/icons/history';
	import { mergeProps, Tooltip } from 'bits-ui';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import {
		BOARD_COLUMN_WORD,
		DONE_WINDOW_LABELS,
		type BoardColumn,
		type BoardStatusColumn,
		type DoneWindow
	} from '../../present';
	import type { TaskRow } from '../../types';

	let {
		uid,
		column,
		rows,
		hiddenDone = 0,
		doneWindow = '7d',
		writableStatus,
		dragOver = false,
		onDoneWindowChange,
		onDragOver,
		onDragLeave,
		onDrop,
		renderCard
	}: {
		uid: string;
		column: BoardStatusColumn;
		rows: TaskRow[];
		hiddenDone?: number;
		doneWindow?: DoneWindow;
		writableStatus: BoardColumn | null;
		dragOver?: boolean;
		onDoneWindowChange: (next: DoneWindow) => void;
		onDragOver: (event: DragEvent) => void;
		onDragLeave: () => void;
		onDrop: (event: DragEvent) => void;
		renderCard: Snippet<[TaskRow]>;
	} = $props();

	let historyMenuOpen = $state(false);
	const doneWindowOptions: readonly DoneWindow[] = ['7d', '30d', 'all'];
</script>

<section
	aria-labelledby="{uid}-col-{column}"
	data-testid="board-column"
	data-status={column}
	ondragover={onDragOver}
	ondragleave={onDragLeave}
	ondrop={onDrop}
	class={['work-bcol flex min-w-0 flex-col gap-2.5', dragOver && 'is-drag-over']}
>
	<div class="bcol-h flex items-center justify-between gap-2">
		<h3 id="{uid}-col-{column}" class="flex min-w-0 items-center gap-2 text-sm/6 font-semibold">
			<span class="truncate">{BOARD_COLUMN_WORD[column]}</span>
			<span class="bcol-cnt tabular-nums">{rows.length}</span>
		</h3>

		{#if column === 'done' && (hiddenDone > 0 || doneWindow !== '7d')}
			<DropdownMenu.Root bind:open={historyMenuOpen}>
				<Tooltip.Root disabled={historyMenuOpen}>
					<Tooltip.Trigger>
						{#snippet child({ props: tipProps })}
							<DropdownMenu.Trigger>
								{#snippet child({ props: menuProps })}
									<button
										{...mergeProps(tipProps, menuProps)}
										type="button"
										data-testid="toggle-all-done"
										aria-label="Done history window: {DONE_WINDOW_LABELS[doneWindow]}{hiddenDone > 0
											? `, ${hiddenDone} older hidden`
											: ''}"
										class="bcol-window-btn inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs font-medium focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-focus max-[639px]:min-h-11"
									>
										<History class="size-3.5 shrink-0" aria-hidden="true" />
										<span class="truncate">{DONE_WINDOW_LABELS[doneWindow]}</span>
									</button>
								{/snippet}
							</DropdownMenu.Trigger>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content
							side="top"
							sideOffset={6}
							class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
							>{hiddenDone > 0
								? `${hiddenDone} older done hidden — change window`
								: 'Done history window'}</Tooltip.Content
						>
					</Tooltip.Portal>
				</Tooltip.Root>
				<DropdownMenu.Content align="end" class="triage-menu-surface w-44 ring-line">
					<DropdownMenu.Group>
						<DropdownMenu.GroupHeading>Done history</DropdownMenu.GroupHeading>
						{#each doneWindowOptions as opt (opt)}
							<DropdownMenu.Item onSelect={() => onDoneWindowChange(opt)}>
								<span class="flex w-full items-center justify-between gap-2">
									<span>{DONE_WINDOW_LABELS[opt]}</span>
									{#if doneWindow === opt}
										<Check class="size-3.5 text-brand-text" aria-hidden="true" />
									{/if}
								</span>
							</DropdownMenu.Item>
						{/each}
					</DropdownMenu.Group>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		{/if}
	</div>

	{#if rows.length === 0}
		<p class="bcol-empty px-2 py-4 text-xs/5">
			{#if column === 'done' && hiddenDone > 0}
				No items completed in {DONE_WINDOW_LABELS[doneWindow].toLowerCase()}.
			{:else if writableStatus === null}
				No items in this state.
			{:else}
				No items.
			{/if}
		</p>
	{:else}
		<ul role="list" aria-labelledby="{uid}-col-{column}" class="flex flex-col gap-2.5">
			{#each rows as row (row.id)}
				{@render renderCard(row)}
			{/each}
		</ul>
	{/if}
</section>

<style>
	.work-bcol {
		padding: 0 8px 10px;
		border-radius: var(--bd-radius-lane);
		background: var(--bd-lane);
		color: var(--bd-ink);
	}
	.work-bcol.is-drag-over {
		outline: 2px solid var(--tone-accent-ink);
		outline-offset: -2px;
	}
	.bcol-h {
		position: sticky;
		top: 0;
		z-index: 2;
		min-height: 44px;
		margin-inline: -8px;
		padding: 6px 8px 4px 14px;
		border-radius: var(--bd-radius-lane) var(--bd-radius-lane) 0 0;
		background: var(--bd-lane);
		color: var(--bd-ink);
	}
	.bcol-cnt {
		flex: none;
		padding: 1px 8px;
		border-radius: 999px;
		background: var(--bd-pill);
		font-size: 0.72rem;
		font-weight: 600;
		line-height: 1.5;
		color: var(--bd-body);
	}
	.bcol-window-btn {
		color: var(--bd-muted);
	}
	.bcol-window-btn:hover {
		background: var(--bd-pill);
		color: var(--bd-ink);
	}
	.bcol-empty {
		color: var(--bd-muted);
	}
</style>
