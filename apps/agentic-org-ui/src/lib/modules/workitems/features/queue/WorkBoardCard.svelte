<!--
  @component
  Borderless raised canonical Work board card (TASK-194 / TASK-229):
  top metadata row, 3-line clamped title with hover/focus tooltip only when clamped,
  SVG checklist progress ring, assignee footer, and keyboard/drag actions.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { mergeProps, Tooltip } from 'bits-ui';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import Avatar from '$lib/components/Avatar.svelte';
	import BudgetChip from '../../components/BudgetChip.svelte';
	import StatusBadge from '../../components/StatusBadge.svelte';
	import WorkIcon from '../../components/WorkIcon.svelte';
	import {
		PRIORITIES,
		PRIORITY_WORD,
		STATUS_WORD,
		TYPE_WORD,
		assignmentText,
		severityWord,
		taskHref,
		typedRef,
		type BoardColumn
	} from '../../present';
	import type { Priority, TaskRow } from '../../types';

	let {
		row,
		showProject = true,
		stale = false,
		archived = false,
		menuOpen = false,
		canMoveTo,
		onMenuOpenChange,
		onOpen,
		onMove,
		onChangePriority
	}: {
		row: TaskRow;
		showProject?: boolean;
		stale?: boolean;
		archived?: boolean;
		menuOpen?: boolean;
		canMoveTo: (row: TaskRow, target: BoardColumn) => boolean;
		onMenuOpenChange: (isOpen: boolean) => void;
		onOpen: (event: MouseEvent, row: TaskRow) => void;
		onMove: (row: TaskRow, target: BoardColumn) => void;
		onChangePriority: (row: TaskRow, priority: Priority) => void;
	} = $props();

	let titleEl = $state<HTMLAnchorElement | null>(null);
	let titleTipOpen = $state(false);

	function handleTitleTipOpenChange(next: boolean) {
		titleTipOpen = next && Boolean(titleEl && titleEl.scrollHeight > titleEl.clientHeight + 1);
	}

	const priSig = $derived(
		row.priority === 'urgent'
			? { level: 3 as const, strong: true }
			: row.priority === 'high'
				? { level: 3 as const, strong: false }
				: row.priority === 'normal'
					? { level: 2 as const, strong: false }
					: { level: 1 as const, strong: false }
	);

	const checklistPct = $derived(
		row.checklist.total > 0 ? Math.round((row.checklist.done / row.checklist.total) * 100) : 0
	);
	const RING_R = 6;
	const RING_CIRC = 2 * Math.PI * RING_R;
	const ringDashOffset = $derived(RING_CIRC * (1 - checklistPct / 100));
</script>

<div class="flex items-center justify-between gap-2">
	<div class="flex min-w-0 flex-1 items-center gap-1.5 text-xs/5">
		{#if showProject && row.project}
			<span class="bcard-chip min-w-0 max-w-[11rem]" title={row.project.name}
				><span class="truncate">{row.project.name}</span></span
			>
		{/if}
		{#if row.priority !== 'normal'}
			<span
				class="bcard-chip shrink-0 font-semibold"
				data-tone={priSig.strong ? 'bad' : undefined}
				data-testid="card-priority"
			>
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
				<span>{PRIORITY_WORD[row.priority]}</span>
			</span>
		{:else}
			<span class="work-priority-mark" data-priority-level={priSig.level} aria-hidden="true">
				<i class="is-on"></i>
				<i class="is-on"></i>
				<i></i>
			</span>
		{/if}
		<WorkIcon name={row.type} class="size-3.5 shrink-0 text-[var(--bd-muted)]" />
		<span class="sr-only">{TYPE_WORD[row.type]}</span>
		<span class="bcard-ref ml-auto shrink-0 font-mono whitespace-nowrap">{row.ref}</span>
	</div>

	{#if !stale}
		<DropdownMenu.Root open={menuOpen} onOpenChange={onMenuOpenChange}>
			<Tooltip.Root disabled={menuOpen}>
				<Tooltip.Trigger>
					{#snippet child({ props: tipProps })}
						<DropdownMenu.Trigger>
							{#snippet child({ props: menuProps })}
								<button
									{...mergeProps(tipProps, menuProps)}
									type="button"
									aria-label="Actions for {typedRef(row)}"
									class="bcard-menu-btn inline-flex size-8 shrink-0 items-center justify-center rounded-md focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11"
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
			<DropdownMenu.Content align="end" class="triage-menu-surface w-48 ring-line">
				{#if row.type === 'task' && row.status !== 'cancelled'}
					<DropdownMenu.Group>
						<DropdownMenu.GroupHeading>Move to</DropdownMenu.GroupHeading>
						{#each ['todo', 'in_progress', 'blocked', 'done'] as const as dest (dest)}
							{#if canMoveTo(row, dest)}
								<DropdownMenu.Item onSelect={() => onMove(row, dest)}>
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
							<DropdownMenu.Item onSelect={() => onChangePriority(row, p)}>
								{PRIORITY_WORD[p]}
							</DropdownMenu.Item>
						{/if}
					{/each}
				</DropdownMenu.Group>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/if}
</div>

<Tooltip.Root bind:open={titleTipOpen} onOpenChange={handleTitleTipOpenChange}>
	<Tooltip.Trigger>
		{#snippet child({ props })}
			<a
				bind:this={titleEl}
				{...mergeProps(props, {
					onclick: (e: MouseEvent) => onOpen(e, row)
				})}
				href={resolve(taskHref(row.ref) as '/')}
				class="bcard-title text-sm/5 font-semibold hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
			>
				{row.title}
			</a>
		{/snippet}
	</Tooltip.Trigger>
	{#if titleTipOpen}
		<Tooltip.Portal>
			<Tooltip.Content
				role="tooltip"
				side="top"
				sideOffset={6}
				class="z-50 max-w-xs rounded-md bg-surface-overlay px-2.5 py-1.5 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
				>{row.title}</Tooltip.Content
			>
		</Tooltip.Portal>
	{/if}
</Tooltip.Root>

{#if row.status === 'blocked' && row.blockedReason}
	<p class="text-xs/5 text-danger-text [overflow-wrap:anywhere]">
		Blocked: {row.blockedReason}
	</p>
{/if}

{#if row.checklist.total > 0}
	<div
		class="bcard-progress flex items-center gap-2 text-xs/5"
		data-complete={checklistPct === 100 ? '' : undefined}
		data-testid="card-checklist"
	>
		<svg viewBox="0 0 16 16" class="bring size-4 shrink-0" aria-hidden="true">
			<circle cx="8" cy="8" r={RING_R} class="bring-track" />
			<circle
				cx="8"
				cy="8"
				r={RING_R}
				class="bring-fill"
				stroke-dasharray={RING_CIRC}
				stroke-dashoffset={ringDashOffset}
				transform="rotate(-90 8 8)"
			/>
		</svg>
		<span class="min-w-0 truncate tabular-nums">
			{row.checklist.done}/{row.checklist.total}
			{#if row.checklist.requiredLeft > 0}
				<span class="ml-1 inline-flex items-center gap-0.5 text-[var(--bd-muted)]">
					<WorkIcon name="lock" class="size-3" />
					{row.checklist.requiredLeft} required
				</span>
			{/if}
		</span>
		<span class="bprog-pct ml-auto shrink-0 font-bold tabular-nums">{checklistPct}%</span>
	</div>
{/if}

{#if row.type === 'bug' || row.reopenCount > 0 || row.budget || archived}
	<div class="flex flex-wrap items-center gap-x-2.5 gap-y-1.5 text-xs/5">
		{#if row.type === 'bug'}
			<span
				class="inline-flex items-center gap-1 {row.severity === 'critical'
					? 'text-danger-text'
					: 'text-[var(--bd-body)]'}"
			>
				<WorkIcon name={row.severity ?? 'unset'} class="size-3.5" />
				<span>{severityWord(row.severity)}</span>
			</span>
		{/if}
		{#if row.reopenCount > 0}
			<span class="inline-flex items-center gap-1 text-[var(--bd-muted)] tabular-nums">
				<WorkIcon name="rotate" class="size-3" />
				<span>Reopened {row.reopenCount}</span>
			</span>
		{/if}
		{#if row.budget}
			<BudgetChip budget={row.budget} hold={row.budgetHold} />
		{/if}
		{#if archived}
			<span class="bcard-chip font-medium" data-testid="work-archived-tag">Archived</span>
		{/if}
	</div>
{/if}

<div class="flex items-center justify-between gap-2 pt-0.5 text-xs/5 text-[var(--bd-muted)]">
	<div class="flex min-w-0 items-center gap-1.5">
		{#if row.assignee}
			<Avatar name={row.assignee.name} size="sm" />
		{/if}
		<span class="truncate text-[var(--bd-body)]">{assignmentText(row)}</span>
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

<style>
	.bcard-chip {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		min-height: 22px;
		padding: 0 7px;
		border-radius: 6px;
		background: var(--bd-chip);
		color: var(--bd-body);
		font-size: 0.72rem;
	}
	.bcard-chip[data-tone='bad'] {
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
	}
	.bcard-ref {
		font-size: 0.72rem;
		color: var(--bd-muted);
	}
	.bcard-menu-btn {
		color: var(--bd-muted);
	}
	.bcard-menu-btn:hover {
		background: var(--bd-pill);
		color: var(--bd-ink);
	}
	.bcard-title {
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 3;
		line-clamp: 3;
		overflow: hidden;
		overflow-wrap: anywhere;
		color: var(--bd-ink);
	}
	.bcard-progress {
		color: var(--bd-body);
	}
	.bring {
		fill: none;
		stroke-width: 2.25;
	}
	.bring-track {
		stroke: var(--bd-ring-track);
	}
	.bring-fill {
		stroke: var(--bd-ring);
		stroke-linecap: round;
	}
	.bcard-progress[data-complete] .bring-fill {
		stroke: var(--bd-ring-done);
	}
	.bprog-pct {
		color: var(--bd-ink);
	}
	.work-priority-mark {
		display: inline-flex;
		flex: none;
		align-items: flex-end;
		justify-content: center;
		gap: 2px;
		width: 0.9rem;
		height: 0.9rem;
		padding-bottom: 2px;
		color: var(--bd-muted);
	}
	.work-priority-mark[data-tone='strong'] {
		color: currentColor;
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
