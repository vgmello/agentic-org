<!--
  @component
  Grouped table (desktop >= 640 px) and divided rows (narrow < 640 px) for Tasks, Bugs and Requests
  (work-items-ux §6, §9, §11, §13; feature-requests-ux §2; BUG-10..15, WORK-4, WORK-5, RQS-7).
  Sticky group headings show severity counts with words (including "not set") for Bugs.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import {
		PRIORITY_WORD,
		TYPE_WORD,
		ageOf,
		assignmentText,
		byPriorityThenAge,
		bySeverityThenPriority,
		groupRows,
		rowSpeech,
		severityCounts,
		severityWord,
		taskHref,
		type GroupBy
	} from '../../present';
	import type { Priority, TaskRow, TaskType } from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';
	import StatusBadge from '../../components/StatusBadge.svelte';
	import BudgetChip from '../../components/BudgetChip.svelte';

	function prioritySignal(priority: Priority): { level: 1 | 2 | 3; strong?: boolean } {
		if (priority === 'urgent') return { level: 3, strong: true };
		if (priority === 'high') return { level: 3 };
		if (priority === 'normal') return { level: 2 };
		return { level: 1 };
	}

	let {
		rows,
		tab,
		groupBy,
		showProject = true,
		onopen
	}: {
		rows: TaskRow[];
		tab: TaskType | 'all';
		groupBy: GroupBy;
		showProject?: boolean;
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
	const sortFn = $derived(tab === 'bug' ? bySeverityThenPriority : byPriorityThenAge);
	const groups = $derived(groupRows(rows, groupBy, sortFn));
	const showSeverityCol = $derived(tab === 'bug' || rows.some((r) => r.type === 'bug'));
</script>

<div class="flex flex-col gap-6" data-testid="work-list">
	{#each groups as group (group.key)}
		{@const headingId = `${uid}-group-${group.key || 'none'}`}
		<section aria-labelledby={headingId} class="flex flex-col gap-2">
			{#if groupBy !== 'none'}
				<div
					class="sticky top-14 z-10 flex flex-wrap items-baseline justify-between gap-2 border-b border-line bg-surface py-2 min-[640px]:top-16"
					data-testid="work-group-heading"
				>
					<h3 id={headingId} class="text-sm/6 font-semibold text-fg">
						{group.label}
						<span class="ml-1.5 text-xs/5 font-normal text-fg-muted tabular-nums"
							>{group.rows.length}</span
						>
					</h3>
					{#if tab === 'bug'}
						<p class="text-xs/5 text-fg-muted" data-testid="group-severity-counts">
							{severityCounts(group.rows)}
						</p>
					{/if}
				</div>
			{:else}
				<h3 id={headingId} class="sr-only">Work items</h3>
			{/if}

			<!-- Desktop table (>= 640 px) -->
			<div
				class="hidden overflow-x-auto rounded-xl bg-surface-raised outline-1 -outline-offset-1 outline-line min-[640px]:block"
			>
				<table class="w-full text-left text-sm/6">
					<thead class="border-b border-line text-xs/5 font-semibold text-fg-muted">
						<tr>
							<th scope="col" class="px-3 py-2.5">Item</th>
							<th scope="col" class="px-3 py-2.5">Title</th>
							{#if showProject}
								<th scope="col" class="px-3 py-2.5">Project</th>
							{/if}
							{#if showSeverityCol}
								<th scope="col" class="px-3 py-2.5">Severity</th>
							{/if}
							<th scope="col" class="px-3 py-2.5">Priority</th>
							<th scope="col" class="px-3 py-2.5">Status</th>
							<th scope="col" class="px-3 py-2.5">Accountable</th>
							<th scope="col" class="px-3 py-2.5">Assigned team</th>
							<th scope="col" class="px-3 py-2.5">Assignee</th>
							<th scope="col" class="px-3 py-2.5">Reporter</th>
							<th scope="col" class="px-3 py-2.5">Age</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-line-subtle text-fg-body">
						{#each group.rows as row (row.id)}
							{@const priSig = prioritySignal(row.priority)}
							<tr data-testid="work-row" data-ref={row.ref} class="hover:bg-surface-overlay/50">
								<td class="px-3 py-2.5 whitespace-nowrap">
									<span class="inline-flex items-center gap-1.5 text-xs/5 text-fg-muted">
										<WorkIcon name={row.type} class="size-3.5" />
										<span>{TYPE_WORD[row.type]}</span>
										<span class="font-mono text-fg-secondary">{row.ref}</span>
									</span>
								</td>
								<th scope="row" class="min-w-48 px-3 py-2.5 font-normal">
									<div class="flex flex-wrap items-center gap-2">
										<a
											href={resolve(taskHref(row.ref) as '/')}
											aria-label={rowSpeech(row)}
											onclick={(e) => handleOpen(e, row)}
											class="font-semibold text-fg hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
										>
											{row.title}
										</a>
										{#if row.budget}
											<BudgetChip budget={row.budget} hold={row.budgetHold} />
										{/if}
										{#if row.awaitingVerification}
											<span
												class="rounded-md bg-surface-overlay px-1.5 py-0.5 text-xs/5 text-fg-secondary"
											>
												Verify fix
											</span>
										{/if}
									</div>
								</th>
								{#if showProject}
									<td
										class="px-3 py-2.5 whitespace-nowrap text-fg-secondary"
										data-testid="work-project"
									>
										{row.project ? row.project.name : 'No project'}
									</td>
								{/if}
								{#if showSeverityCol}
									<td class="px-3 py-2.5 whitespace-nowrap text-xs/5">
										{#if row.type === 'bug'}
											<span
												class="inline-flex items-center gap-1 {row.severity === 'critical'
													? 'text-danger-text'
													: 'text-fg-secondary'}"
											>
												<WorkIcon name={row.severity ?? 'unset'} class="size-3.5" />
												<span>{severityWord(row.severity)}</span>
											</span>
										{:else}
											<span class="text-fg-subtle">—</span>
										{/if}
									</td>
								{/if}
								<td class="px-3 py-2.5 whitespace-nowrap text-xs/5">
									<span class="inline-flex items-center gap-1.5 text-fg-secondary">
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
										{#if row.priority !== 'normal'}
											<span>{PRIORITY_WORD[row.priority]}</span>
										{:else}
											<span class="sr-only">Priority normal</span>
											<span aria-hidden="true" class="text-fg-subtle">Normal</span>
										{/if}
									</span>
								</td>
								<td class="px-3 py-2.5 whitespace-nowrap">
									<StatusBadge status={row.status} needsMe={row.needsMe} />
								</td>
								<td class="px-3 py-2.5 whitespace-nowrap text-fg-secondary">{row.team.name}</td>
								<td class="px-3 py-2.5 whitespace-nowrap text-fg-muted">
									{row.assignedTeam ? row.assignedTeam.name : '—'}
								</td>
								<td class="px-3 py-2.5 whitespace-nowrap text-fg-secondary">
									{#if row.assignee}
										<span>{row.assignee.name}</span>
										<span class="ml-1 text-xs/5 text-fg-muted">{row.assignee.kind}</span>
									{:else}
										<span class="text-fg-muted">Unassigned</span>
									{/if}
								</td>
								<td class="px-3 py-2.5 whitespace-nowrap text-fg-muted">
									<span>{row.reporter.name}</span>
									<span class="ml-1 text-xs/5">{row.reporter.kind}</span>
								</td>
								<td class="px-3 py-2.5 whitespace-nowrap text-xs/5 text-fg-muted tabular-nums">
									{ageOf(row.createdAt)}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>

			<!-- Narrow divided rows (< 640 px, zero sideways overflow at 320 px / 200% text) -->
			<ul
				role="list"
				class="divide-y divide-line-subtle rounded-xl bg-surface-raised outline-1 -outline-offset-1 outline-line min-[640px]:hidden"
			>
				{#each group.rows as row (row.id)}
					{@const priSig = prioritySignal(row.priority)}
					<li
						class="relative flex flex-col gap-1.5 px-4 py-3.5"
						data-testid="work-row-mobile"
						data-ref={row.ref}
					>
						<div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs/5 text-fg-muted">
							<span class="inline-flex items-center gap-1">
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
								<WorkIcon name={row.type} class="size-3.5" />
								<span>{TYPE_WORD[row.type]}</span>
								<span class="font-mono text-fg-secondary">{row.ref}</span>
							</span>
							{#if showProject}
								<span>· {row.project ? row.project.name : 'No project'}</span>
							{/if}
							<span class="tabular-nums">· {ageOf(row.createdAt)}</span>
						</div>

						<a
							href={resolve(taskHref(row.ref) as '/')}
							aria-label={rowSpeech(row)}
							onclick={(e) => handleOpen(e, row)}
							class="text-sm/6 font-semibold text-fg hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
						>
							<span class="absolute inset-0"></span>
							{row.title}
						</a>

						<div class="flex flex-wrap items-center gap-2 text-xs/5">
							<StatusBadge status={row.status} needsMe={row.needsMe} />
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
								<span class="inline-flex items-center gap-1 text-fg-secondary">
									<WorkIcon name={row.priority} class="size-3.5 text-fg-muted" />
									<span>{PRIORITY_WORD[row.priority]}</span>
								</span>
							{/if}
							{#if row.budget}
								<BudgetChip budget={row.budget} hold={row.budgetHold} />
							{/if}
						</div>

						<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
							Accountable {row.team.name} · {assignmentText(row)} · Reported by {row.reporter.name}
						</p>
					</li>
				{/each}
			</ul>
		</section>
	{/each}
</div>

<style>
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
