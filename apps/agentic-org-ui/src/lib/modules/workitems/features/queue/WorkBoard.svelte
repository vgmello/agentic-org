<!--
  @component
  Work task board (work-items-ux §2, §9, TSK-1..4, TSK-10, CHK-1; TASK-194 / TASK-229).
  Tinted lanes with borderless raised canonical cards, SVG checklist rings, clamped-title tooltips,
  board-level Done history popover (7d/30d/all), search empty-lane collapse/reveal with polite live announce,
  and a phone lane picker (< 640px).
-->
<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import { errorClass, mobileButton, textareaClass } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import {
		BOARD_COLUMN_WORD,
		DONE_WINDOW_LABELS,
		PRIORITY_WORD,
		STATUS_WORD,
		boardColumnsForMode,
		rowSpeech,
		typedRef,
		type BoardColumn,
		type BoardMode,
		type BoardStatusColumn,
		type DoneWindow
	} from '../../present';
	import type { Priority, TaskRow } from '../../types';
	import WorkBoardCard from './WorkBoardCard.svelte';
	import WorkBoardLane from './WorkBoardLane.svelte';

	let {
		organizationId,
		rows,
		boardMode = 'stages',
		showProject = true,
		stale = false,
		archivedIds,
		selectedRef = null,
		searchQuery = '',
		onchanged,
		onopen
	}: {
		organizationId: string;
		rows: TaskRow[];
		boardMode?: BoardMode;
		showProject?: boolean;
		stale?: boolean;
		archivedIds?: ReadonlySet<string>;
		selectedRef?: string | null;
		searchQuery?: string;
		onchanged: () => void;
		onopen?: (row: TaskRow, triggerEl: HTMLElement | null) => void;
	} = $props();

	function handleOpen(event: MouseEvent, row: TaskRow) {
		if (
			!onopen ||
			event.defaultPrevented ||
			event.button !== 0 ||
			event.metaKey ||
			event.ctrlKey ||
			event.shiftKey ||
			event.altKey
		) {
			return;
		}
		event.preventDefault();
		onopen(row, event.currentTarget as HTMLElement | null);
	}

	const uid = $props.id();
	let doneWindow = $state<DoneWindow>('7d');
	let revealEmptyForQuery = $state<string | null>(null);
	let phoneLane = $state<BoardStatusColumn | null>(null);
	let announcement = $state('');
	let dragRow = $state<TaskRow | null>(null);
	let dragOverCol = $state<BoardStatusColumn | null>(null);
	let openMenuId = $state<string | null>(null);
	let frozenRows = $state.raw<TaskRow[] | null>(null);

	// Hold reordering while a card menu is open (WORK-6).
	const visibleRows = $derived(openMenuId && frozenRows ? frozenRows : rows);
	const allColumns = $derived(
		boardColumnsForMode(visibleRows, boardMode, { doneWindow, now: new Date() })
	);

	const trimmedQuery = $derived(searchQuery.trim());
	const revealEmptyInSearch = $derived(trimmedQuery !== '' && revealEmptyForQuery === trimmedQuery);
	const laneHasContent = (col: (typeof allColumns)[number]) =>
		col.rows.length > 0 || col.hiddenDone > 0;
	const matchingColumns = $derived(allColumns.filter(laneHasContent));
	const collapsibleEmptyCount = $derived(allColumns.length - matchingColumns.length);
	const canCollapseEmptyLanes = $derived(
		trimmedQuery !== '' && matchingColumns.length > 0 && collapsibleEmptyCount > 0
	);
	const hiddenEmptyCount = $derived(
		canCollapseEmptyLanes && !revealEmptyInSearch ? collapsibleEmptyCount : 0
	);
	const displayedColumns = $derived(
		canCollapseEmptyLanes && !revealEmptyInSearch ? matchingColumns : allColumns
	);
	const activePhoneLane = $derived.by<BoardStatusColumn>(() => {
		if (phoneLane && displayedColumns.some((c) => c.column === phoneLane)) return phoneLane;
		const firstWithRows = displayedColumns.find((c) => c.rows.length > 0);
		return firstWithRows?.column ?? displayedColumns[0]?.column ?? 'todo';
	});

	// Blocked reason modal state (TSK-3).
	let blockTarget = $state<TaskRow | null>(null);
	let blockReason = $state('');
	let blockError = $state<string | undefined>();
	let blocking = $state(false);
	let announceTimer: ReturnType<typeof setTimeout> | undefined;

	function announce(text: string) {
		clearTimeout(announceTimer);
		announcement = '';
		announceTimer = setTimeout(() => {
			announcement = text;
		}, 10);
	}

	$effect(() => () => clearTimeout(announceTimer));

	function handleDoneWindowChange(next: DoneWindow) {
		doneWindow = next;
		announce(`Done column showing ${DONE_WINDOW_LABELS[next].toLowerCase()}.`);
	}

	function toggleRevealEmptyInSearch() {
		if (!canCollapseEmptyLanes) return;
		const nextReveal = !revealEmptyInSearch;
		revealEmptyForQuery = nextReveal ? trimmedQuery : null;
		announce(
			nextReveal
				? `Showing all ${allColumns.length} lanes.`
				: `Hiding ${collapsibleEmptyCount} empty ${collapsibleEmptyCount === 1 ? 'lane' : 'lanes'} while searching.`
		);
	}

	function canMoveTo(row: TaskRow, target: BoardColumn): boolean {
		// `task.set_status` accepts Tasks only; bugs and requests transition via their detail workflows.
		if (row.type !== 'task') return false;
		if (row.status === target || row.status === 'cancelled') return false;
		return true;
	}

	async function moveRow(row: TaskRow, target: BoardColumn) {
		if (stale || !canMoveTo(row, target)) return;
		if (target === 'blocked') {
			blockTarget = row;
			blockReason = row.blockedReason ?? '';
			blockError = undefined;
			return;
		}
		try {
			await workItemsApi.setStatus(organizationId, row.id, {
				status: target,
				expectedRevision: row.revision
			});
			const msg = `Moved ${typedRef(row)} to ${STATUS_WORD[target]}.`;
			announce(msg);
			toast.success(msg);
			onchanged();
		} catch (e) {
			toast.error(toAppError(e).message);
		}
	}

	async function confirmBlock(event: SubmitEvent) {
		event.preventDefault();
		if (!blockTarget || blocking) return;
		const reason = blockReason.trim();
		if (!reason) {
			blockError = 'Explain what is blocking this item.';
			return;
		}
		blocking = true;
		blockError = undefined;
		try {
			await workItemsApi.setStatus(organizationId, blockTarget.id, {
				status: 'blocked',
				blockedReason: reason,
				expectedRevision: blockTarget.revision
			});
			const msg = `Marked ${typedRef(blockTarget)} as Blocked.`;
			announce(msg);
			toast.success(msg);
			blockTarget = null;
			blockReason = '';
			onchanged();
		} catch (e) {
			blockError = toAppError(e).message;
		} finally {
			blocking = false;
		}
	}

	async function changePriority(row: TaskRow, priority: Priority) {
		if (stale || row.priority === priority) return;
		try {
			await workItemsApi.setPriority(organizationId, row.id, {
				priority,
				expectedRevision: row.revision
			});
			const msg = `Changed ${typedRef(row)} priority to ${PRIORITY_WORD[priority]}.`;
			announce(msg);
			toast.success(msg);
			onchanged();
		} catch (e) {
			toast.error(toAppError(e).message);
		}
	}

	function handleCardKeydown(event: KeyboardEvent, row: TaskRow) {
		if (!event.altKey || stale) return;
		if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
			const step = event.key === 'ArrowLeft' ? -1 : 1;
			const currentIdx = allColumns.findIndex((c) => c.rows.some((r) => r.id === row.id));
			if (currentIdx < 0) return;
			let next: BoardColumn | undefined;
			for (let i = currentIdx + step; i >= 0 && i < allColumns.length; i += step) {
				const candidate = allColumns[i].writableStatus;
				if (candidate && canMoveTo(row, candidate)) {
					next = candidate;
					break;
				}
			}
			if (!next) return;
			event.preventDefault();
			void moveRow(row, next);
		}
	}

	function handleMenuOpenChange(rowId: string, isOpen: boolean) {
		if (isOpen) {
			frozenRows = rows;
			openMenuId = rowId;
		} else if (openMenuId === rowId) {
			openMenuId = null;
			frozenRows = null;
		}
	}
</script>

<div class="sr-only" aria-live="polite" aria-atomic="true" data-testid="board-live-region">
	{announcement}
</div>
<div class="sr-only" aria-live="polite" aria-atomic="true" data-testid="board-search-live">
	{#if trimmedQuery}
		{displayedColumns.reduce((sum, c) => sum + c.rows.length, 0)} matching items across {displayedColumns.length}
		{displayedColumns.length === 1 ? 'lane' : 'lanes'}.
	{/if}
</div>

<Tooltip.Provider delayDuration={150}>
	{#if canCollapseEmptyLanes}
		<div
			class="mb-2 flex flex-wrap items-center justify-between gap-2 text-xs/5 text-fg-muted"
			data-testid="board-search-lanes-bar"
		>
			<span>
				{#if hiddenEmptyCount > 0}
					{hiddenEmptyCount} empty {hiddenEmptyCount === 1 ? 'lane' : 'lanes'} hidden for “{trimmedQuery}”.
				{:else}
					Showing all {allColumns.length} lanes for “{trimmedQuery}”.
				{/if}
			</span>
			<button
				type="button"
				onclick={toggleRevealEmptyInSearch}
				class="rounded-md px-2 py-1 font-medium text-link hover:underline focus-visible:outline-2 focus-visible:outline-focus"
			>
				{revealEmptyInSearch ? 'Hide empty lanes' : `Show empty lanes (${hiddenEmptyCount})`}
			</button>
		</div>
	{/if}

	<!-- Phone lane picker (< 640px) -->
	<div
		class="phone-lane-picker mb-2 flex items-center gap-1.5 overflow-x-auto pb-1 sm:hidden"
		role="group"
		aria-label="Board lane"
		data-testid="phone-lane-picker"
	>
		{#each displayedColumns as col (col.column)}
			<button
				type="button"
				onclick={() => (phoneLane = col.column)}
				aria-pressed={activePhoneLane === col.column}
				class={['phone-lane-pill shrink-0', activePhoneLane === col.column && 'is-active']}
			>
				<span>{BOARD_COLUMN_WORD[col.column]}</span>
				<span class="tabular-nums opacity-75">{col.rows.length}</span>
			</button>
		{/each}
	</div>

	<!-- horizontally scrollable board region needs keyboard focus -->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<div
		class="work-board-scroll"
		tabindex="0"
		role="region"
		aria-label="Work board"
		data-testid="work-board"
		data-board-mode={boardMode}
		style:--board-column-count={displayedColumns.length}
	>
		<div class="work-board-columns">
			{#each displayedColumns as col (col.column)}
				<div class={['board-lane-slot min-w-0', activePhoneLane !== col.column && 'max-sm:hidden']}>
					<WorkBoardLane
						{uid}
						column={col.column}
						rows={col.rows}
						hiddenDone={col.hiddenDone}
						{doneWindow}
						writableStatus={col.writableStatus}
						dragOver={dragOverCol === col.column}
						onDoneWindowChange={handleDoneWindowChange}
						onDragOver={(e) => {
							if (stale || !col.writableStatus) return;
							e.preventDefault();
							dragOverCol = col.column;
						}}
						onDragLeave={() => {
							if (dragOverCol === col.column) dragOverCol = null;
						}}
						onDrop={(e) => {
							e.preventDefault();
							dragOverCol = null;
							if (dragRow && col.writableStatus) void moveRow(dragRow, col.writableStatus);
							dragRow = null;
						}}
					>
						{#snippet renderCard(row)}
							<!-- card keyboard focus and drag-and-drop paired with keyboard status menu -->
							<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
							<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
							<li
								draggable={!stale && row.type === 'task' && row.status !== 'cancelled'}
								tabindex="0"
								aria-label={rowSpeech(row)}
								aria-current={selectedRef === row.ref ? 'true' : undefined}
								data-testid="work-card"
								data-ref={row.ref}
								ondragstart={() => (dragRow = row)}
								ondragend={() => ((dragRow = null), (dragOverCol = null))}
								onkeydown={(e) => handleCardKeydown(e, row)}
								class="work-bcard flex flex-col gap-2"
							>
								<WorkBoardCard
									{row}
									{showProject}
									{stale}
									archived={Boolean(archivedIds?.has(row.id))}
									menuOpen={openMenuId === row.id}
									{canMoveTo}
									onMenuOpenChange={(isOpen) => handleMenuOpenChange(row.id, isOpen)}
									onOpen={handleOpen}
									onMove={(targetRow, dest) => void moveRow(targetRow, dest)}
									onChangePriority={(targetRow, p) => void changePriority(targetRow, p)}
								/>
							</li>
						{/snippet}
					</WorkBoardLane>
				</div>
			{/each}
		</div>
	</div>
</Tooltip.Provider>

<!-- Blocked reason dialog (TSK-3) -->
<Dialog
	open={blockTarget !== null}
	onOpenChange={(next) => !next && !blocking && (blockTarget = null)}
>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text={blockTarget ? `Block ${blockTarget.ref}` : 'Block task'} />
			<DialogDescription>
				Explain what is blocking this work so the team knows what is needed to unblock it.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={confirmBlock} novalidate>
			<div>
				<Label for="{uid}-block-reason" class="block text-sm/6 font-medium text-fg">Reason</Label>
				<textarea
					id="{uid}-block-reason"
					rows={3}
					bind:value={blockReason}
					disabled={blocking}
					aria-invalid={blockError ? 'true' : undefined}
					aria-describedby={blockError ? `${uid}-block-err` : undefined}
					placeholder="Waiting on…"
					class="mt-1.5 {textareaClass}"></textarea>
				{#if blockError}
					<p id="{uid}-block-err" class={errorClass} role="alert">{blockError}</p>
				{/if}
			</div>
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={blocking}
					onclick={() => (blockTarget = null)}>Cancel</Button
				>
				<Button type="submit" class={mobileButton} disabled={blocking} aria-busy={blocking}>
					{blocking ? 'Saving…' : 'Mark as blocked'}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>

<style>
	.work-board-scroll {
		min-width: 0;
		overflow-x: auto;
		overscroll-behavior-inline: contain;
		padding-bottom: 4px;
	}
	.work-board-scroll:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.work-board-columns {
		--board-column-width: 15.5rem;
		--board-column-gap: 14px;
		display: grid;
		grid-template-columns: repeat(
			var(--board-column-count, 4),
			minmax(var(--board-column-width), 1fr)
		);
		gap: var(--board-column-gap);
		min-width: calc(
			var(--board-column-count, 4) * var(--board-column-width) +
				(var(--board-column-count, 4) - 1) * var(--board-column-gap)
		);
	}
	.phone-lane-pill {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-height: 44px;
		padding: 0 12px;
		border-radius: 999px;
		background: var(--bd-lane);
		color: var(--bd-body);
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.phone-lane-pill.is-active {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.work-bcard {
		position: relative;
		min-width: 0;
		padding: 14px 16px;
		border: 1px solid transparent;
		border-radius: var(--bd-radius-card);
		background: var(--bd-card);
		box-shadow: var(--bd-shadow);
		color: var(--bd-body);
		transition:
			transform 0.16s ease,
			box-shadow 0.16s ease,
			background-color 0.16s ease;
	}
	.work-bcard:hover {
		transform: translateY(-2px);
		background: var(--bd-card-hover);
		box-shadow: var(--bd-lift);
	}
	.work-bcard:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.work-bcard[aria-current='true'] {
		box-shadow:
			0 0 0 2px var(--tone-accent-ink),
			var(--bd-shadow);
	}
	@media (prefers-reduced-motion: reduce) {
		.work-bcard {
			transition: none;
		}
		.work-bcard:hover {
			transform: none;
		}
	}
	@media (max-width: 639px) {
		.work-board-columns {
			grid-template-columns: minmax(0, 1fr);
			min-width: 0;
		}
	}
</style>
