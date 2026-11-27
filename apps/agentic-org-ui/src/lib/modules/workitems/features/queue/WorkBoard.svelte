<!--
  @component
  Work task board (work-items-ux §2, §9, TSK-1..4, TSK-10, CHK-1).
  Four columns (To do, In progress, Blocked, Done), each an accessible list. Done shows the last 7 days with
  "Show all done". Cards move by drag-and-drop, ⋯ Move to menu, and Alt+←/→ keyboard shortcuts with live-region
  announcements. Moving to Blocked prompts for a reason.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { mergeProps, Tooltip } from 'bits-ui';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
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
		PRIORITIES,
		PRIORITY_WORD,
		STATUS_WORD,
		TYPE_WORD,
		assignmentText,
		boardColumnsForMode,
		rowSpeech,
		severityWord,
		taskHref,
		typedRef,
		type BoardColumn,
		type BoardMode,
		type BoardStatusColumn
	} from '../../present';
	import type { Priority, TaskRow } from '../../types';
	import Avatar from '$lib/components/Avatar.svelte';
	import WorkIcon from '../../components/WorkIcon.svelte';
	import StatusBadge from '../../components/StatusBadge.svelte';
	import BudgetChip from '../../components/BudgetChip.svelte';

	let {
		organizationId,
		rows,
		boardMode = 'stages',
		showProject = true,
		stale = false,
		archivedIds,
		selectedRef = null,
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
	let allDone = $state(false);
	let announcement = $state('');
	let dragRow = $state<TaskRow | null>(null);
	let dragOverCol = $state<BoardStatusColumn | null>(null);
	let openMenuId = $state<string | null>(null);
	let frozenRows = $state.raw<TaskRow[] | null>(null);

	// Hold reordering while a card menu is open (WORK-6).
	const visibleRows = $derived(openMenuId && frozenRows ? frozenRows : rows);
	const columns = $derived(
		boardColumnsForMode(visibleRows, boardMode, { allDone, now: new Date() })
	);

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
			const currentIdx = columns.findIndex((c) => c.rows.some((r) => r.id === row.id));
			if (currentIdx < 0) return;
			let next: BoardColumn | undefined;
			for (let i = currentIdx + step; i >= 0 && i < columns.length; i += step) {
				const candidate = columns[i].writableStatus;
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

	function prioritySignal(priority: Priority): { level: 1 | 2 | 3; strong?: boolean } {
		if (priority === 'urgent') return { level: 3, strong: true };
		if (priority === 'high') return { level: 3 };
		if (priority === 'normal') return { level: 2 };
		return { level: 1 };
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
<Tooltip.Provider delayDuration={150}>
	<!-- horizontally scrollable board region needs keyboard focus -->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<div
		class="work-board-scroll"
		tabindex="0"
		role="region"
		aria-label="Work board"
		data-testid="work-board"
		data-board-mode={boardMode}
		style:--board-column-count={columns.length}
	>
		<div class="work-board-columns">
			{#each columns as col (col.column)}
				<section
					aria-labelledby="{uid}-col-{col.column}"
					data-testid="board-column"
					data-status={col.column}
					ondragover={(e) => {
						if (stale || !col.writableStatus) return;
						e.preventDefault();
						dragOverCol = col.column;
					}}
					ondragleave={() => {
						if (dragOverCol === col.column) dragOverCol = null;
					}}
					ondrop={(e) => {
						e.preventDefault();
						dragOverCol = null;
						if (dragRow && col.writableStatus) void moveRow(dragRow, col.writableStatus);
						dragRow = null;
					}}
					class="work-bcol flex min-w-0 flex-col gap-3 rounded-xl bg-surface-raised p-3 outline-1 -outline-offset-1 {dragOverCol ===
					col.column
						? 'outline-2 outline-brand'
						: 'outline-line'}"
				>
					<div class="flex items-center justify-between gap-2 px-1">
						<h3 id="{uid}-col-{col.column}" class="text-sm/6 font-semibold text-fg">
							{BOARD_COLUMN_WORD[col.column]}
							<span class="ml-1.5 text-xs/5 font-normal text-fg-muted tabular-nums"
								>{col.rows.length}</span
							>
						</h3>
						{#if col.column === 'done' && (col.hiddenDone > 0 || allDone)}
							<button
								type="button"
								onclick={() => (allDone = !allDone)}
								class="rounded-sm text-xs/5 font-medium text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
								data-testid="toggle-all-done"
							>
								{allDone ? 'Last 7 days' : `Show all done (${col.hiddenDone})`}
							</button>
						{/if}
					</div>
					{#if col.rows.length === 0}
						<p class="px-1 py-4 text-xs/5 text-fg-muted">
							{col.column === 'done' && col.hiddenDone > 0
								? 'No items completed in the last 7 days.'
								: 'No items.'}
						</p>
					{:else}
						<ul role="list" aria-labelledby="{uid}-col-{col.column}" class="flex flex-col gap-2.5">
							{#each col.rows as row (row.id)}
								{@const priSig = prioritySignal(row.priority)}
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
									class="work-bcard flex flex-col gap-2 rounded-lg bg-surface p-3 outline-1 -outline-offset-1 outline-line focus-visible:outline-2 focus-visible:outline-focus"
								>
									<div class="flex items-start justify-between gap-2">
										<div class="flex min-w-0 items-center gap-1.5 text-xs/5 text-fg-muted">
											<span
												class="work-priority-mark"
												data-priority-level={priSig.level}
												data-tone={priSig.strong ? 'strong' : undefined}
												aria-hidden="true"
											>
												<i class="is-on"></i>
												<i class={priSig.level >= 2 ? 'is-on' : undefined}></i>
												<i class={priSig.level >= 3 ? 'is-on' : undefined}></i>
											</span>
											<WorkIcon name={row.type} class="size-3.5 text-fg-muted" />
											<span class="sr-only">{TYPE_WORD[row.type]}</span>
											<span class="shrink-0 font-mono whitespace-nowrap">{row.ref}</span>
											{#if showProject && row.project}
												<span class="truncate">· {row.project.name}</span>
											{/if}
										</div>

										{#if !stale}
											<DropdownMenu.Root
												open={openMenuId === row.id}
												onOpenChange={(isOpen) => handleMenuOpenChange(row.id, isOpen)}
											>
												<Tooltip.Root disabled={openMenuId === row.id}>
													<Tooltip.Trigger>
														{#snippet child({ props: tipProps })}
															<DropdownMenu.Trigger>
																{#snippet child({ props: menuProps })}
																	<button
																		{...mergeProps(tipProps, menuProps)}
																		type="button"
																		aria-label="Actions for {typedRef(row)}"
																		class="inline-flex size-9 items-center justify-center rounded-md text-fg-muted hover:bg-surface-overlay hover:text-fg focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11"
																	>
																		<WorkIcon name="ellipsis" class="size-4" />
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
															>Actions for {typedRef(row)}</Tooltip.Content
														>
													</Tooltip.Portal>
												</Tooltip.Root>
												<DropdownMenu.Content
													align="end"
													class="triage-menu-surface w-48 ring-line"
												>
													{#if row.type === 'task' && row.status !== 'cancelled'}
														<DropdownMenu.Group>
															<DropdownMenu.GroupHeading>Move to</DropdownMenu.GroupHeading>
															{#each ['todo', 'in_progress', 'blocked', 'done'] as const as dest (dest)}
																{#if canMoveTo(row, dest)}
																	<DropdownMenu.Item onSelect={() => moveRow(row, dest)}>
																		{STATUS_WORD[dest]}
																	</DropdownMenu.Item>
																{/if}
															{/each}
														</DropdownMenu.Group>
														<DropdownMenu.Separator class="bg-line-subtle" />
													{/if}
													<DropdownMenu.Group>
														<DropdownMenu.GroupHeading>Priority</DropdownMenu.GroupHeading>
														{#each PRIORITIES as p (p)}
															{#if p !== row.priority}
																<DropdownMenu.Item onSelect={() => changePriority(row, p)}>
																	{PRIORITY_WORD[p]}
																</DropdownMenu.Item>
															{/if}
														{/each}
													</DropdownMenu.Group>
												</DropdownMenu.Content>
											</DropdownMenu.Root>
										{/if}
									</div>

									<a
										href={resolve(taskHref(row.ref) as '/')}
										onclick={(e) => handleOpen(e, row)}
										class="text-sm/5 font-semibold text-fg hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
									>
										{row.title}
									</a>

									{#if row.status === 'blocked' && row.blockedReason}
										<p class="text-xs/5 text-danger-text [overflow-wrap:anywhere]">
											Blocked: {row.blockedReason}
										</p>
									{/if}

									<div class="flex flex-wrap items-center gap-x-2.5 gap-y-1.5 text-xs/5">
										{#if row.type === 'bug'}
											<span
												class="inline-flex items-center gap-1 {row.severity === 'critical'
													? 'text-danger-text'
													: 'text-fg-secondary'}"
											>
												<WorkIcon name={row.severity ?? 'unset'} class="size-3.5" />
												<span>{severityWord(row.severity)}</span>
											</span>
										{/if}

										{#if row.priority !== 'normal'}
											<span
												class="inline-flex items-center gap-1 text-fg-secondary"
												data-testid="card-priority"
											>
												<WorkIcon name={row.priority} class="size-3.5 text-fg-muted" />
												<span>{PRIORITY_WORD[row.priority]}</span>
											</span>
										{/if}

										{#if row.checklist.total > 0}
											<span
												class="inline-flex items-center gap-1 text-fg-muted tabular-nums"
												data-testid="card-checklist"
											>
												<span>{row.checklist.done}/{row.checklist.total}</span>
												{#if row.checklist.requiredLeft > 0}
													<WorkIcon name="lock" class="size-3 text-fg-muted" />
													<span>{row.checklist.requiredLeft} required</span>
												{/if}
											</span>
										{/if}

										{#if row.reopenCount > 0}
											<span class="inline-flex items-center gap-1 text-fg-muted tabular-nums">
												<WorkIcon name="rotate" class="size-3" />
												<span>Reopened {row.reopenCount}</span>
											</span>
										{/if}

										{#if row.budget}
											<BudgetChip budget={row.budget} hold={row.budgetHold} />
										{/if}

										{#if archivedIds?.has(row.id)}
											<span
												class="inline-flex items-center rounded bg-surface-raised px-1.5 py-0.5 text-[11px]/4 font-medium text-fg-muted ring-1 ring-line"
												data-testid="work-archived-tag"
											>
												Archived
											</span>
										{/if}
									</div>

									<div
										class="flex items-center justify-between gap-2 pt-0.5 text-xs/5 text-fg-muted"
									>
										<div class="flex min-w-0 items-center gap-1.5">
											{#if row.assignee}
												<Avatar name={row.assignee.name} size="sm" />
											{/if}
											<span class="truncate">{assignmentText(row)}</span>
											{#if row.needsMe}
												<span class="shrink-0" data-testid="card-needs-you">
													<StatusBadge status={row.status} needsMe={row.needsMe} />
												</span>
											{/if}
										</div>
										{#if row.effort !== null}
											<span class="shrink-0 tabular-nums">{row.effort} pt</span>
										{/if}
									</div>
								</li>
							{/each}
						</ul>
					{/if}
				</section>
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
		--board-column-gap: 1rem;
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
	@media (max-width: 639px) {
		.work-board-columns {
			grid-template-columns: minmax(0, 1fr);
			min-width: 0;
		}
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
