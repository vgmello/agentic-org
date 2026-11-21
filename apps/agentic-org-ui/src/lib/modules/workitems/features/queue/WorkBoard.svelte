<!--
  @component
  Work task board (work-items-ux §2, §9, TSK-1..4, TSK-10, CHK-1).
  Four columns (To do, In progress, Blocked, Done), each an accessible list. Done shows the last 7 days with
  "Show all done". Cards move by drag-and-drop, ⋯ Move to menu, and Alt+←/→ keyboard shortcuts with live-region
  announcements. Moving to Blocked prompts for a reason.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
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
		PRIORITIES,
		PRIORITY_WORD,
		STATUS_WORD,
		TYPE_WORD,
		assignmentText,
		boardColumns,
		neighbourColumn,
		rowSpeech,
		severityWord,
		taskHref,
		typedRef,
		type BoardColumn
	} from '../../present';
	import type { Priority, TaskRow } from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';
	import StatusBadge from '../../components/StatusBadge.svelte';
	import BudgetChip from '../../components/BudgetChip.svelte';

	let {
		organizationId,
		rows,
		showProject = true,
		stale = false,
		onchanged
	}: {
		organizationId: string;
		rows: TaskRow[];
		showProject?: boolean;
		stale?: boolean;
		onchanged: () => void;
	} = $props();

	const uid = $props.id();
	let allDone = $state(false);
	let announcement = $state('');
	let dragRow = $state<TaskRow | null>(null);
	let dragOverCol = $state<BoardColumn | null>(null);
	let openMenuId = $state<string | null>(null);
	let frozenRows = $state.raw<TaskRow[] | null>(null);

	// Hold reordering while a card menu is open (WORK-6).
	const visibleRows = $derived(openMenuId && frozenRows ? frozenRows : rows);
	const columns = $derived(boardColumns(visibleRows, { allDone, now: new Date() }));

	// Blocked reason modal state (TSK-3).
	let blockTarget = $state<TaskRow | null>(null);
	let blockReason = $state('');
	let blockError = $state<string | undefined>();
	let blocking = $state(false);

	function announce(text: string) {
		announcement = '';
		setTimeout(() => {
			announcement = text;
		}, 10);
	}

	async function moveRow(row: TaskRow, target: BoardColumn) {
		if (stale || row.status === target) return;
		if (target === 'blocked') {
			blockTarget = row;
			blockReason = row.blockedReason ?? '';
			blockError = undefined;
			return;
		}
		try {
			await workItemsApi.setStatus(organizationId, row.id, {
				status: target,
				expectedRevision: 0
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
				expectedRevision: 0
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
				expectedRevision: 0
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
			const next = neighbourColumn(row.status, step);
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

<div
	class="grid grid-cols-1 gap-4 min-[640px]:grid-cols-2 min-[1024px]:grid-cols-4"
	data-testid="work-board"
>
	{#each columns as col (col.column)}
		<section
			aria-labelledby="{uid}-col-{col.column}"
			data-testid="board-column"
			data-status={col.column}
			ondragover={(e) => {
				if (stale) return;
				e.preventDefault();
				dragOverCol = col.column;
			}}
			ondragleave={() => {
				if (dragOverCol === col.column) dragOverCol = null;
			}}
			ondrop={(e) => {
				e.preventDefault();
				dragOverCol = null;
				if (dragRow) void moveRow(dragRow, col.column);
				dragRow = null;
			}}
			class="flex min-w-0 flex-col gap-3 rounded-xl bg-surface-raised p-3 outline-1 -outline-offset-1 {dragOverCol ===
			col.column
				? 'outline-2 outline-brand'
				: 'outline-line'}"
		>
			<div class="flex items-center justify-between gap-2 px-1">
				<h3 id="{uid}-col-{col.column}" class="text-sm/6 font-semibold text-fg">
					{STATUS_WORD[col.column]}
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
						<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
						<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
						<li
							draggable={!stale}
							tabindex="0"
							aria-label={rowSpeech(row)}
							data-testid="work-card"
							data-ref={row.ref}
							ondragstart={() => (dragRow = row)}
							ondragend={() => ((dragRow = null), (dragOverCol = null))}
							onkeydown={(e) => handleCardKeydown(e, row)}
							class="flex flex-col gap-2 rounded-lg bg-surface p-3 outline-1 -outline-offset-1 outline-line focus-visible:outline-2 focus-visible:outline-focus"
						>
							<div class="flex items-start justify-between gap-2">
								<div class="flex min-w-0 items-center gap-1.5 text-xs/5 text-fg-muted">
									<WorkIcon name={row.type} class="size-3.5 text-fg-muted" />
									<span class="sr-only">{TYPE_WORD[row.type]}</span>
									<span class="font-mono">{row.ref}</span>
									{#if showProject && row.project}
										<span class="truncate">· {row.project.name}</span>
									{/if}
								</div>

								{#if !stale}
									<DropdownMenu.Root
										open={openMenuId === row.id}
										onOpenChange={(isOpen) => handleMenuOpenChange(row.id, isOpen)}
									>
										<DropdownMenu.Trigger>
											{#snippet child({ props })}
												<button
													{...props}
													type="button"
													aria-label="Actions for {typedRef(row)}"
													class="inline-flex size-7 items-center justify-center rounded-md text-fg-muted hover:bg-surface-overlay hover:text-fg focus-visible:outline-2 focus-visible:outline-focus max-[639px]:size-11"
												>
													<WorkIcon name="ellipsis" class="size-4" />
												</button>
											{/snippet}
										</DropdownMenu.Trigger>
										<DropdownMenu.Content align="end" class="w-48 bg-surface-sunken ring-line">
											<DropdownMenu.Group>
												<DropdownMenu.GroupHeading>Move to</DropdownMenu.GroupHeading>
												{#each ['todo', 'in_progress', 'blocked', 'done'] as const as dest (dest)}
													{#if dest !== row.status}
														<DropdownMenu.Item onSelect={() => moveRow(row, dest)}>
															{STATUS_WORD[dest]}
														</DropdownMenu.Item>
													{/if}
												{/each}
											</DropdownMenu.Group>
											<DropdownMenu.Separator class="bg-line-subtle" />
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
								{#if row.needsMe}
									<StatusBadge status={row.status} needsMe={row.needsMe} />
								{/if}

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
							</div>

							<div class="flex items-center justify-between gap-2 pt-0.5 text-xs/5 text-fg-muted">
								<span class="truncate">{assignmentText(row)}</span>
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
