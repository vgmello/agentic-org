<!--
  @component
  POC-aligned Linear-style divided rows for Tasks, Bugs and Requests
  (work-items-ux §6, §9, §11, §13; feature-requests-ux §2; BUG-10..15, WORK-4, WORK-5, RQS-7).
  Single container-query responsive layout (zero horizontal overflow at 320 px / 200% text).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import { projectDotTone, type WorkStatusRingTone } from '$lib/modules/work/compact';
	import {
		PRIORITY_WORD,
		STATUS_WORD,
		TYPE_WORD,
		ageOf,
		groupRows,
		rowSpeech,
		severityCounts,
		severityWord,
		taskHref,
		workRowComparator,
		type GroupBy,
		type WorkSortBy,
		type WorkSortDir
	} from '../../present';
	import type { Priority, TaskRow, TaskStatus, TaskType } from '../../types';
	import BudgetChip from '../../components/BudgetChip.svelte';

	function prioritySignal(priority: Priority): { level: 1 | 2 | 3; strong?: boolean } {
		if (priority === 'urgent') return { level: 3, strong: true };
		if (priority === 'high') return { level: 3 };
		if (priority === 'normal') return { level: 2 };
		return { level: 1 };
	}

	function statusTone(status: TaskStatus): WorkStatusRingTone {
		if (status === 'done') return 'done';
		if (status === 'blocked') return 'blocked';
		if (status === 'in_progress') return 'active';
		return 'neutral';
	}

	let {
		rows,
		tab,
		groupBy,
		subgroupBy = 'none',
		sortBy = 'default',
		sortDir = 'asc',
		showProject = true,
		showDividers = false,
		selectedRef = null,
		onopen
	}: {
		rows: TaskRow[];
		tab: TaskType | 'all';
		groupBy: GroupBy;
		subgroupBy?: GroupBy;
		sortBy?: WorkSortBy;
		sortDir?: WorkSortDir;
		showProject?: boolean;
		showDividers?: boolean;
		selectedRef?: string | null;
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
	const sortFn = $derived(workRowComparator(tab, sortBy, sortDir));
	const groups = $derived(groupRows(rows, groupBy, sortFn));
	const effectiveSubgroupBy = $derived<GroupBy>(
		groupBy !== 'none' && subgroupBy !== 'none' && subgroupBy !== groupBy ? subgroupBy : 'none'
	);
</script>

<div class="work-items" data-testid="work-list" data-work-dividers={showDividers ? '' : undefined}>
	{#each groups as group (group.key)}
		{@const headingId = `${uid}-group-${group.key || 'none'}`}
		{@const subgroups =
			effectiveSubgroupBy !== 'none'
				? groupRows(group.rows, effectiveSubgroupBy, sortFn)
				: [{ key: 'all', label: '', rows: group.rows }]}
		<section aria-labelledby={headingId} class="work-group">
			{#if groupBy !== 'none'}
				<div class="work-group-title" data-testid="work-group-heading">
					<h3 id={headingId} class="work-group-label">
						<span>{group.label}</span>
						<span class="cnt">{group.rows.length}</span>
					</h3>
					{#if tab === 'bug'}
						<p class="work-group-meta" data-testid="group-severity-counts">
							{severityCounts(group.rows)}
						</p>
					{/if}
				</div>
			{:else}
				<h3 id={headingId} class="sr-only">Work items</h3>
			{/if}

			<div class={effectiveSubgroupBy !== 'none' ? 'work-subgroups' : undefined}>
				{#each subgroups as subgroup (subgroup.key)}
					<div class={effectiveSubgroupBy !== 'none' ? 'work-subgroup' : undefined}>
						{#if effectiveSubgroupBy !== 'none'}
							<h4 class="work-subgroup-title" data-testid="work-subgroup-heading">
								<span>{subgroup.label}</span>
								<span class="cnt">{subgroup.rows.length}</span>
							</h4>
						{/if}

						<ul role="list" class="work-list">
							{#each subgroup.rows as row (row.id)}
								{@const priSig = prioritySignal(row.priority)}
								{@const tone = statusTone(row.status)}
								{@const teamChip = row.assignedTeam ?? row.team}
								{@const teamDot = projectDotTone(teamChip.id)}
								{@const isSelected = selectedRef === row.ref}
								<li
									class="work-row"
									data-testid="work-row"
									data-ref={row.ref}
									aria-current={isSelected ? 'true' : undefined}
								>
									<div class="work-summary">
										<div class="work-ident">
											<span
												class="work-priority-mark"
												data-priority-level={priSig.level}
												data-tone={priSig.strong ? 'strong' : undefined}
												title="{PRIORITY_WORD[row.priority]} priority"
												aria-hidden="true"
											>
												<i class="is-on"></i>
												<i class={priSig.level >= 2 ? 'is-on' : undefined}></i>
												<i class={priSig.level >= 3 ? 'is-on' : undefined}></i>
											</span>
											<span class="work-ref">{row.ref}</span>
										</div>

										<div class="work-heading">
											<span class="work-markers">
												<span
													class="work-status"
													title="{STATUS_WORD[row.status]}{row.needsMe ? ' · Needs you' : ''}"
													aria-hidden="true"
												>
													<span class="work-status-ring" data-tone={tone}>
														{#if row.status === 'done'}
															<svg
																viewBox="0 0 16 16"
																fill="none"
																stroke="currentColor"
																stroke-width="1.8"
																stroke-linecap="round"
																stroke-linejoin="round"
															>
																<circle cx="8" cy="8" r="6" />
																<path d="m5.5 8.2 1.7 1.7 3.3-3.6" />
															</svg>
														{:else if row.status === 'in_progress'}
															<svg
																viewBox="0 0 16 16"
																fill="none"
																stroke="currentColor"
																stroke-width="1.8"
															>
																<circle cx="8" cy="8" r="6" />
																<path
																	d="M8 2a6 6 0 0 1 0 12V2Z"
																	fill="currentColor"
																	stroke="none"
																/>
															</svg>
														{:else if row.status === 'blocked'}
															<svg
																viewBox="0 0 16 16"
																fill="none"
																stroke="currentColor"
																stroke-width="1.8"
																stroke-linecap="round"
															>
																<circle cx="8" cy="8" r="6" />
																<path d="M5.5 8h5" />
															</svg>
														{:else if row.status === 'cancelled'}
															<svg
																viewBox="0 0 16 16"
																fill="none"
																stroke="currentColor"
																stroke-width="1.6"
																stroke-dasharray="2.5 2"
															>
																<circle cx="8" cy="8" r="6" />
															</svg>
														{:else}
															<svg
																viewBox="0 0 16 16"
																fill="none"
																stroke="currentColor"
																stroke-width="1.8"
															>
																<circle cx="8" cy="8" r="6" />
															</svg>
														{/if}
													</span>
													{#if row.needsMe}
														<span class="work-attention-dot"></span>
													{/if}
												</span>

												{#if row.checklist.total > 0}
													<span
														class="work-criteria"
														data-done={row.checklist.done === row.checklist.total ? '' : undefined}
														title="{row.checklist.done} of {row.checklist.total} steps done"
													>
														{row.checklist.done}/{row.checklist.total}
													</span>
												{/if}
											</span>

											<a
												href={resolve(taskHref(row.ref) as '/')}
												aria-label={rowSpeech(row)}
												onclick={(e) => handleOpen(e, row)}
												class="work-title"
											>
												{row.title}
											</a>

											{#if tab === 'all' && row.type !== 'task'}
												<span class="work-inline-chip">{TYPE_WORD[row.type]}</span>
											{/if}

											{#if row.type === 'bug'}
												<span
													class="work-inline-chip"
													data-tone={row.severity === 'critical' ? 'bad' : undefined}
												>
													{severityWord(row.severity)}
												</span>
											{/if}

											{#if row.needsMe}
												<span class="work-inline-chip" data-tone="warn">Needs you</span>
											{/if}

											{#if row.awaitingVerification}
												<span class="work-inline-chip">Verify fix</span>
											{/if}

											{#if row.budget}
												<BudgetChip budget={row.budget} hold={row.budgetHold} />
											{/if}
										</div>
									</div>

									<div class="work-facts">
										{#if showProject}
											<span
												class="work-project"
												data-muted={!row.project ? '' : undefined}
												data-testid="work-project"
											>
												{row.project ? row.project.name : 'No project'}
											</span>
										{/if}

										<span
											class="work-team"
											title={row.assignedTeam
												? `Assigned team: ${row.assignedTeam.name}`
												: `Accountable team: ${row.team.name}`}
										>
											<i class="work-team-dot" data-dot={teamDot} aria-hidden="true"></i>
											<span>{teamChip.name}</span>
										</span>

										<span class="work-owner" data-unassigned={!row.assignee ? '' : undefined}>
											{#if row.assignee}
												<Avatar name={row.assignee.name} size="sm" />
												<span class="work-owner-name">{row.assignee.name}</span>
											{:else}
												<span class="work-owner-name">Unassigned</span>
											{/if}
										</span>

										<span class="work-age" title="Created {ageOf(row.createdAt)} ago">
											{ageOf(row.createdAt)}
										</span>
									</div>
								</li>
							{/each}
						</ul>
					</div>
				{/each}
			</div>
		</section>
	{/each}
</div>

<style>
	.work-items {
		container-type: inline-size;
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.work-group + .work-group {
		margin-top: 12px;
	}
	.work-subgroups {
		margin-inline-start: 8px;
	}
	.work-subgroup + .work-subgroup {
		margin-block-start: 6px;
	}
	.work-group-title,
	.work-subgroup-title {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 8px;
		padding: 6px 12px;
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-ink, var(--fg));
	}
	.work-group-title {
		border-radius: var(--exec-ctrl-radius, 6px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line-subtle));
		margin-bottom: 4px;
	}
	.work-group-label {
		display: inline-flex;
		align-items: baseline;
		gap: 8px;
		font-size: inherit;
		font-weight: inherit;
		color: inherit;
		margin: 0;
	}
	.work-group-meta {
		margin: 0;
		font-size: 0.75rem;
		font-weight: 400;
		color: var(--exec-muted, var(--fg-muted));
	}
	.work-subgroup-title {
		justify-content: flex-start;
		color: var(--exec-body, var(--fg-secondary));
		padding-block: 5px 3px;
		margin: 0;
	}
	.work-group-title .cnt,
	.work-subgroup-title .cnt {
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 500;
		font-variant-numeric: tabular-nums;
	}
	.work-list {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	.work-row {
		position: relative;
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		align-items: center;
		gap: 6px 16px;
		min-height: 44px;
		padding: 9px 12px;
		border-radius: var(--exec-ctrl-radius, 6px);
		transition: background-color 110ms ease-out;
	}
	.work-row:is(:hover, :focus-within) {
		background: var(
			--work-row-hover,
			var(--triage-row-hover, var(--exec-btn-ghost-bg-hover, var(--surface-raised)))
		);
	}
	.work-row[aria-current='true'] {
		background: var(--work-row-selected, var(--triage-row-selected, var(--tone-accent-bg)));
		box-shadow: inset 3px 0 0 var(--work-selected-bar, var(--tone-accent-ink));
	}
	.work-items[data-work-dividers] .work-row:not([aria-current='true']) {
		box-shadow: inset 0 -1px var(--exec-border, var(--line-subtle));
	}
	.work-items[data-work-dividers] .work-row:last-child:not([aria-current='true']) {
		box-shadow: none;
	}
	.work-items[data-work-dividers] .work-row:is(:hover, :focus-within):not([aria-current='true']) {
		box-shadow: none;
	}
	.work-summary {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		min-width: 0;
	}
	.work-ident {
		display: inline-flex;
		flex: none;
		align-items: center;
		gap: 6px;
		min-height: 1.4375rem;
	}
	.work-ref {
		color: var(--exec-muted, var(--fg-muted));
		font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
		font-size: 0.78125rem;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
		min-width: 3.25rem;
	}
	.work-heading {
		display: flex;
		flex: 1 1 auto;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 8px;
		min-width: 0;
	}
	.work-markers {
		display: inline-flex;
		flex: none;
		align-items: center;
		gap: 6px;
		min-height: 1.4375rem;
	}
	.work-status {
		position: relative;
		display: inline-flex;
		flex: none;
		align-items: center;
		justify-content: center;
		width: 1rem;
		height: 1rem;
	}
	.work-status-ring {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 0.9375rem;
		height: 0.9375rem;
		color: var(--exec-muted, var(--fg-muted));
	}
	.work-status-ring svg {
		width: 100%;
		height: 100%;
		display: block;
	}
	.work-status-ring[data-tone='active'] {
		color: var(--tone-accent, var(--brand));
	}
	.work-status-ring[data-tone='done'] {
		color: var(--tone-ok, var(--success));
	}
	.work-status-ring[data-tone='blocked'] {
		color: var(--tone-bad, var(--danger));
	}
	.work-attention-dot {
		position: absolute;
		inset: -1px -1px auto auto;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--tone-warn, var(--warning));
		box-shadow: 0 0 0 1.5px var(--exec-bg, var(--surface));
	}
	.work-criteria {
		display: inline-flex;
		align-items: center;
		padding: 1px 6px;
		border-radius: var(--exec-sm-radius, 4px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line-subtle));
		color: var(--exec-muted, var(--fg-muted));
		font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
		font-size: 0.6875rem;
		line-height: 1.35;
		font-variant-numeric: tabular-nums;
	}
	.work-criteria[data-done] {
		color: var(--tone-ok-ink, var(--success-text));
		background: var(--tone-ok-bg, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--tone-ok-ring, var(--line-subtle));
	}
	.work-title {
		color: var(--exec-ink, var(--fg));
		font-size: 0.9375rem;
		font-weight: 550;
		line-height: 1.4;
		letter-spacing: -0.005em;
		text-decoration: none;
		overflow-wrap: anywhere;
		min-width: 0;
	}
	.work-title:hover {
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.work-title:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
		border-radius: 3px;
	}
	.work-inline-chip {
		display: inline-flex;
		align-items: center;
		padding: 1px 6px;
		border-radius: var(--exec-sm-radius, 4px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line-subtle));
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.71875rem;
		font-weight: 500;
		line-height: 1.35;
		white-space: nowrap;
	}
	.work-inline-chip[data-tone='warn'] {
		background: var(--tone-warn-bg, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--tone-warn-ring, var(--line));
		color: var(--tone-warn-ink, var(--warning-text));
	}
	.work-inline-chip[data-tone='bad'] {
		background: var(--tone-bad-bg, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring, var(--line));
		color: var(--tone-bad-ink, var(--danger-text));
	}
	.work-facts {
		display: inline-flex;
		align-items: center;
		gap: 12px;
		min-width: 0;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.8125rem;
		white-space: nowrap;
	}
	.work-project {
		color: var(--exec-body, var(--fg-secondary));
		max-width: 11rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.work-project[data-muted] {
		color: var(--exec-dim, var(--fg-subtle));
	}
	.work-team {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 2px 8px;
		border-radius: 999px;
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line-subtle));
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.75rem;
		font-weight: 500;
		max-width: 10rem;
	}
	.work-team span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.work-team-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		flex: none;
		background: var(--tone-accent, var(--brand));
	}
	.work-team-dot[data-dot='ok'] {
		background: var(--tone-ok, var(--success));
	}
	.work-team-dot[data-dot='warn'] {
		background: var(--tone-warn, var(--warning));
	}
	.work-team-dot[data-dot='queued'] {
		background: var(--tone-queued, var(--brand));
	}
	.work-team-dot[data-dot='bad'] {
		background: var(--tone-bad, var(--danger));
	}
	.work-owner {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		min-width: 7.5rem;
		max-width: 10.5rem;
		color: var(--exec-body, var(--fg-secondary));
	}
	.work-owner[data-unassigned] {
		color: var(--exec-dim, var(--fg-subtle));
	}
	.work-owner-name {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.work-age {
		color: var(--exec-dim, var(--fg-subtle));
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
		min-width: 2.75rem;
		text-align: right;
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

	@container (max-width: 56rem) {
		.work-row {
			grid-template-columns: minmax(0, 1fr);
			gap: 5px;
			padding-block: 10px;
		}
		.work-facts {
			flex-wrap: wrap;
			gap: 6px 10px;
			padding-inline-start: 4.9rem;
			white-space: normal;
		}
		.work-owner {
			min-width: 0;
		}
		.work-age {
			text-align: left;
			min-width: 0;
		}
	}

	@container (max-width: 34rem) {
		.work-summary {
			flex-direction: column;
			gap: 3px;
		}
		.work-facts {
			padding-inline-start: 0;
		}
		.work-project,
		.work-team,
		.work-owner {
			max-width: 100%;
		}
	}

	@media (forced-colors: active) {
		.work-row[aria-current='true'] {
			outline: 2px solid Highlight;
		}
		.work-group-title {
			border: 1px solid CanvasText;
		}
	}
</style>
