<!--
  @component
  Profile › Work (TASK-130, Prototype Release 06fb02ef, offices-and-teams-ux §7.2):
  Open-by-default Tasks view with Open / Needs me / All preset tabs, Status + Project Filter popover,
  Display popover (Group by Status/Project/None, Sort, Direction, Row dividers), List | Board layout
  (4 stages / 6 statuses), and shared header scoped search (`/` focuses, `Escape` preserves query).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import FilterIcon from '@lucide/svelte/icons/list-filter';
	import SlidersHorizontalIcon from '@lucide/svelte/icons/sliders-horizontal';
	import XIcon from '@lucide/svelte/icons/x';
	import Loaded from '$lib/components/Loaded.svelte';
	import ScopedSearchInput from '$lib/components/ScopedSearchInput.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuLabel,
		DropdownMenuRadioGroup,
		DropdownMenuRadioItem,
		DropdownMenuSeparator,
		DropdownMenuCheckboxItem,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import { mobileButton } from '$lib/components/classes';
	import {
		readSessionShowArchived,
		writeSessionShowArchived
	} from '$lib/modules/workitems/archive';
	import { useScopedSearchContext } from '$lib/platform/workspace/scoped-search.svelte';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';
	import { membersApi } from '../../api';
	import { WORK_PRIORITY_LABELS, WORK_STATE_LABELS } from '../../present';
	import type { WorkState } from '../../types';
	import WorkRows from '../../components/WorkRows.svelte';
	import { profileContext } from '../profile/context.svelte';
	import { tabResource } from './reload.svelte';
	import {
		filterAndSortMemberWork,
		groupMemberWork,
		isArchivedMemberWorkRow,
		memberWorkBoardColumns,
		memberWorkPresetCounts,
		memberWorkProjects,
		type MemberWorkBoardMode,
		type MemberWorkDirection,
		type MemberWorkGroupBy,
		type MemberWorkLayout,
		type MemberWorkPreset,
		type MemberWorkSortBy
	} from './work-tab-model';

	const ctx = profileContext();
	const work = tabResource((o, m) => membersApi.work(o, m));
	const scopedSearch = useScopedSearchContext();

	const isSelf = $derived(ctx.profile.viewer.isSelf);
	let query = $state('');
	let preset = $state<MemberWorkPreset>('all');
	let statusFilter = $state<'all' | WorkState>('all');
	let projectFilter = $state<string>('all');
	let groupBy = $state<MemberWorkGroupBy>('status');
	let sortBy = $state<MemberWorkSortBy>('default');
	let direction = $state<MemberWorkDirection>('asc');
	let layout = $state<MemberWorkLayout>('list');
	let boardMode = $state<MemberWorkBoardMode>('stages');
	let showDividers = $state(true);
	let showArchived = $state(readSessionShowArchived());

	const isAgent = $derived(ctx.profile.kind === 'agent');
	const hasActiveFilter = $derived(statusFilter !== 'all' || projectFilter !== 'all');
	const hasActiveDisplay = $derived(
		groupBy !== 'status' ||
			sortBy !== 'default' ||
			direction !== 'asc' ||
			!showDividers ||
			showArchived
	);

	function setShowArchived(next: boolean) {
		showArchived = next;
		writeSessionShowArchived(next);
	}

	$effect(() => {
		if (!scopedSearch) return;
		const agent = isAgent;
		const name = ctx.profile.name;
		return scopedSearch.register({
			getValue: () => query,
			setValue: (next) => {
				query = next;
			},
			label: agent ? `Search tasks for ${name}` : `Search work for ${name}`,
			placeholder: agent ? 'Search tasks…' : 'Search work…',
			headingSelector: '#profile-heading, #shell-page-title, main h1, main h2'
		});
	});

	const STATUS_OPTIONS: readonly { id: 'all' | WorkState; label: string }[] = [
		{ id: 'all', label: 'Any status' },
		{ id: 'todo', label: WORK_STATE_LABELS.todo },
		{ id: 'in_progress', label: WORK_STATE_LABELS.in_progress },
		{ id: 'in_review', label: WORK_STATE_LABELS.in_review },
		{ id: 'blocked', label: WORK_STATE_LABELS.blocked },
		{ id: 'done', label: WORK_STATE_LABELS.done }
	];
</script>

<Loaded
	resource={work}
	label="work"
	error="Couldn't load {ctx.profile.name}'s work"
	isEmpty={(rows) => rows.length === 0}
	emptyTitle="No work assigned"
	emptyDescription="Work items assigned to {ctx.profile.name} show here."
>
	{#snippet emptyAction()}
		{#if ctx.profile.status !== 'inactive'}
			<Button
				variant="outline"
				class={mobileButton}
				onclick={(e) => ctx.act('assign', e.currentTarget)}>Assign task</Button
			>
		{/if}
	{/snippet}
	{#snippet children(rows)}
		{@const archivedRefs = new Set(
			rows.filter((r) => isArchivedMemberWorkRow(r, uiPrefs.archiveAfterDays)).map((r) => r.ref)
		)}
		{@const visibleRows =
			showArchived || archivedRefs.size === 0 ? rows : rows.filter((r) => !archivedRefs.has(r.ref))}
		{@const counts = memberWorkPresetCounts(visibleRows)}
		{@const projects = memberWorkProjects(visibleRows)}
		{@const presetOptions = isSelf
			? [
					{ id: 'all' as const, label: 'All', count: counts.all },
					{ id: 'open' as const, label: 'Open', count: counts.open },
					{ id: 'needs_me' as const, label: 'Needs me', count: counts.needs_me }
				]
			: [
					{ id: 'all' as const, label: 'All', count: counts.all },
					{ id: 'needs_me' as const, label: 'Needs me', count: counts.needs_me },
					{
						id: 'requested_by_me' as const,
						label: 'Requested by me',
						count: counts.requested_by_me
					}
				]}
		{@const filtered = filterAndSortMemberWork(visibleRows, {
			preset,
			status: statusFilter,
			project: projectFilter,
			query,
			sortBy,
			direction
		})}
		{@const groups = groupMemberWork(filtered, groupBy)}
		{@const boardCols = memberWorkBoardColumns(filtered, boardMode)}

		<div class="member-work-toolbar" data-testid="member-work-toolbar">
			<div class="flex min-w-0 flex-1 flex-wrap items-center gap-3">
				{#if !scopedSearch}
					<ScopedSearchInput
						bind:value={query}
						label={isAgent
							? `Search tasks for ${ctx.profile.name}`
							: `Search work for ${ctx.profile.name}`}
						placeholder={isAgent ? 'Search tasks…' : 'Search work…'}
						headingSelector="#profile-heading, #shell-page-title, main h1, main h2"
					/>
				{/if}

				<div role="group" aria-label="Filter tasks" class="mw-seg-track">
					{#each presetOptions as opt (opt.id)}
						<button
							type="button"
							aria-pressed={preset === opt.id && statusFilter === 'all'}
							onclick={() => {
								preset = opt.id;
								if (statusFilter !== 'all') statusFilter = 'all';
							}}
							class={[
								'mw-seg-btn inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs/5 font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11',
								preset === opt.id && statusFilter === 'all'
									? 'is-on'
									: 'text-fg-muted hover:text-fg'
							]}
						>
							<span>{opt.label}</span>
							<span class="tabular-nums opacity-75">{opt.count}</span>
						</button>
					{/each}
				</div>

				<div role="group" aria-label="Layout" class="mw-seg-track">
					{#each [{ id: 'list' as const, label: 'List' }, { id: 'board' as const, label: 'Board' }] as mode (mode.id)}
						<button
							type="button"
							aria-pressed={layout === mode.id}
							onclick={() => (layout = mode.id)}
							class={[
								'mw-seg-btn inline-flex items-center rounded-md px-2.5 py-1 text-xs/5 font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11',
								layout === mode.id ? 'is-on' : 'text-fg-muted hover:text-fg'
							]}
						>
							{mode.label}
						</button>
					{/each}
				</div>

				{#if layout === 'board'}
					<div role="group" aria-label="Board columns" class="mw-seg-track">
						{#each [{ id: 'stages' as const, label: '4 stages' }, { id: 'statuses' as const, label: 'All statuses' }] as colMode (colMode.id)}
							<button
								type="button"
								aria-pressed={boardMode === colMode.id}
								onclick={() => (boardMode = colMode.id)}
								class={[
									'mw-seg-btn inline-flex items-center rounded-md px-2 py-1 text-xs/5 font-medium transition-colors focus-visible:outline-2 focus-visible:outline-focus [@media(pointer:coarse)]:min-h-11',
									boardMode === colMode.id ? 'is-on' : 'text-fg-muted hover:text-fg'
								]}
							>
								{colMode.label}
							</button>
						{/each}
					</div>
				{/if}

				<DropdownMenu>
					<DropdownMenuTrigger>
						{#snippet child({ props })}
							<Button
								{...props}
								variant="outline"
								size="sm"
								aria-label="Filter work"
								class="relative min-h-8 gap-1.5 px-2.5 text-xs [@media(pointer:coarse)]:min-h-11"
							>
								<FilterIcon class="size-3.5" aria-hidden="true" />
								<span>Filter</span>
								{#if hasActiveFilter}
									<span class="mw-active-dot" aria-hidden="true"></span>
								{/if}
							</Button>
						{/snippet}
					</DropdownMenuTrigger>
					<DropdownMenuContent align="start" class="triage-menu-surface w-56">
						<DropdownMenuLabel>Status</DropdownMenuLabel>
						<DropdownMenuRadioGroup bind:value={statusFilter}>
							{#each STATUS_OPTIONS as st (st.id)}
								<DropdownMenuRadioItem value={st.id}>{st.label}</DropdownMenuRadioItem>
							{/each}
						</DropdownMenuRadioGroup>
						{#if projects.length > 0}
							<DropdownMenuSeparator />
							<DropdownMenuLabel>Project</DropdownMenuLabel>
							<DropdownMenuRadioGroup bind:value={projectFilter}>
								<DropdownMenuRadioItem value="all">All projects</DropdownMenuRadioItem>
								{#each projects as proj (proj)}
									<DropdownMenuRadioItem value={proj}>{proj}</DropdownMenuRadioItem>
								{/each}
							</DropdownMenuRadioGroup>
						{/if}
					</DropdownMenuContent>
				</DropdownMenu>

				<DropdownMenu>
					<DropdownMenuTrigger>
						{#snippet child({ props })}
							<Button
								{...props}
								variant="outline"
								size="sm"
								aria-label="Display options"
								class="relative min-h-8 gap-1.5 px-2.5 text-xs [@media(pointer:coarse)]:min-h-11"
							>
								<SlidersHorizontalIcon class="size-3.5" aria-hidden="true" />
								<span>Display</span>
								{#if hasActiveDisplay}
									<span class="mw-active-dot" aria-hidden="true"></span>
								{/if}
							</Button>
						{/snippet}
					</DropdownMenuTrigger>
					<DropdownMenuContent align="start" class="triage-menu-surface w-56">
						<DropdownMenuLabel>Group by</DropdownMenuLabel>
						<DropdownMenuRadioGroup bind:value={groupBy}>
							<DropdownMenuRadioItem value="status">Status</DropdownMenuRadioItem>
							<DropdownMenuRadioItem value="project">Project</DropdownMenuRadioItem>
							<DropdownMenuRadioItem value="none">No grouping</DropdownMenuRadioItem>
						</DropdownMenuRadioGroup>
						<DropdownMenuSeparator />
						<DropdownMenuLabel>Sort by</DropdownMenuLabel>
						<DropdownMenuRadioGroup bind:value={sortBy}>
							<DropdownMenuRadioItem value="default">Default</DropdownMenuRadioItem>
							<DropdownMenuRadioItem value="title">Title</DropdownMenuRadioItem>
							<DropdownMenuRadioItem value="priority">Priority</DropdownMenuRadioItem>
						</DropdownMenuRadioGroup>
						<DropdownMenuSeparator />
						<DropdownMenuLabel>Direction</DropdownMenuLabel>
						<DropdownMenuRadioGroup bind:value={direction}>
							<DropdownMenuRadioItem value="asc">Ascending</DropdownMenuRadioItem>
							<DropdownMenuRadioItem value="desc">Descending</DropdownMenuRadioItem>
						</DropdownMenuRadioGroup>
						<DropdownMenuSeparator />
						<DropdownMenuCheckboxItem bind:checked={showDividers}>
							Row dividers
						</DropdownMenuCheckboxItem>
						<DropdownMenuCheckboxItem
							checked={showArchived}
							onCheckedChange={(checked) => setShowArchived(Boolean(checked))}
						>
							Show archived
						</DropdownMenuCheckboxItem>
					</DropdownMenuContent>
				</DropdownMenu>
			</div>

			<p class="text-xs/5 text-fg-muted tabular-nums" role="status">
				{filtered.length} of {visibleRows.length}
				{visibleRows.length === 1 ? 'item' : 'items'}
			</p>
		</div>

		{#if hasActiveFilter || showArchived}
			<div class="mw-chip-bar" role="region" aria-label="Active filters">
				{#if showArchived}
					<button
						type="button"
						class="mw-chip"
						onclick={() => setShowArchived(false)}
						aria-label="Hide archived items"
					>
						<span>Showing archived</span>
						<XIcon class="size-3" aria-hidden="true" />
					</button>
				{/if}
				{#if statusFilter !== 'all'}
					<button
						type="button"
						class="mw-chip"
						onclick={() => (statusFilter = 'all')}
						aria-label="Clear status filter {WORK_STATE_LABELS[statusFilter]}"
					>
						<span>Status: {WORK_STATE_LABELS[statusFilter]}</span>
						<XIcon class="size-3" aria-hidden="true" />
					</button>
				{/if}
				{#if projectFilter !== 'all'}
					<button
						type="button"
						class="mw-chip"
						onclick={() => (projectFilter = 'all')}
						aria-label="Clear project filter {projectFilter}"
					>
						<span>Project: {projectFilter}</span>
						<XIcon class="size-3" aria-hidden="true" />
					</button>
				{/if}
			</div>
		{/if}

		{#if filtered.length === 0}
			<p class="px-4 py-8 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				{query.trim()
					? 'No work matches this search.'
					: isAgent
						? 'No tasks match this filter.'
						: 'No work assigned.'}
			</p>
		{:else if layout === 'board'}
			<div class="mw-board" data-testid="member-work-board">
				{#each boardCols as col (col.id)}
					<section class="mw-board-col" aria-label={col.label}>
						<header class="mw-board-col-hd">
							<h4 class="text-xs/5 font-semibold text-fg-secondary">{col.label}</h4>
							<span class="text-xs/5 text-fg-muted tabular-nums">{col.rows.length}</span>
						</header>
						{#if col.rows.length === 0}
							<p class="px-2 py-4 text-xs/5 text-fg-muted">None</p>
						{:else}
							<ul role="list" class="flex flex-col gap-2">
								{#each col.rows as item (item.ref)}
									<li class="mw-board-card">
										<a
											href={resolve(`/work/${encodeURIComponent(item.ref)}` as '/')}
											class="block rounded-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
										>
											<p class="flex items-center justify-between gap-2 text-xs/5 text-fg-muted">
												<span class="font-mono">{item.ref}</span>
												{#if item.priority}
													<span>{WORK_PRIORITY_LABELS[item.priority]}</span>
												{/if}
											</p>
											<p class="mt-1 text-sm/5 font-semibold text-fg [overflow-wrap:anywhere]">
												{item.title}
											</p>
											{#if item.project}
												<p class="mt-1.5 text-xs/5 text-fg-muted">{item.project}</p>
											{/if}
										</a>
									</li>
								{/each}
							</ul>
						{/if}
					</section>
				{/each}
			</div>
		{:else}
			<div class="mw-list" data-work-dividers={showDividers ? 'on' : 'off'}>
				{#each groups as group (group.id)}
					{#if group.label}
						<div class="mw-group-title">
							<span>{group.label}</span>
							<StatusPill label={String(group.rows.length)} />
						</div>
					{/if}
					<WorkRows rows={group.rows} hideAssignee {archivedRefs} />
				{/each}
			</div>
		{/if}
	{/snippet}
</Loaded>

<style>
	.member-work-toolbar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 0.625rem 0.75rem;
		border-bottom: 1px solid var(--line-subtle);
		padding: 0.625rem 1rem;
	}
	@media (min-width: 640px) {
		.member-work-toolbar {
			padding-inline: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.member-work-toolbar {
			padding-inline: 2rem;
		}
	}
	.mw-seg-track {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding: 2px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--toolbar-seg-bg, var(--exec-well, var(--surface-raised)));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line));
	}
	.mw-seg-btn.is-on {
		background: var(--toolbar-seg-active-bg, var(--exec-surface-hi, var(--surface-overlay)));
		color: var(--exec-ink, var(--fg));
		box-shadow: inset 0 0 0 1px var(--exec-border-hi, var(--line-strong));
	}
	.mw-active-dot {
		width: 0.375rem;
		height: 0.375rem;
		border-radius: 999px;
		background: var(--tone-accent);
	}
	.mw-chip-bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.375rem;
		padding: 0.5rem 1rem;
		border-bottom: 1px solid var(--line-subtle);
	}
	@media (min-width: 640px) {
		.mw-chip-bar {
			padding-inline: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.mw-chip-bar {
			padding-inline: 2rem;
		}
	}
	.mw-chip {
		display: inline-flex;
		align-items: center;
		gap: 0.375rem;
		border-radius: 999px;
		padding: 0.1875rem 0.625rem;
		font-size: 0.75rem;
		line-height: 1.125rem;
		font-weight: 500;
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	.mw-group-title {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.5rem 1rem;
		font-size: 0.75rem;
		font-weight: 650;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		color: var(--fg-muted);
		background: var(--surface-raised);
		border-bottom: 1px solid var(--line-subtle);
	}
	@media (min-width: 640px) {
		.mw-group-title {
			padding-inline: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.mw-group-title {
			padding-inline: 2rem;
		}
	}
	.mw-list[data-work-dividers='off'] :global(ul) {
		border-color: transparent;
	}
	.mw-list[data-work-dividers='off'] :global(ul > li) {
		border-top-color: transparent;
	}
	.mw-board {
		display: grid;
		grid-auto-flow: column;
		grid-auto-columns: minmax(15.5rem, 1fr);
		gap: 0.75rem;
		overflow-x: auto;
		padding: 1rem;
	}
	@media (min-width: 640px) {
		.mw-board {
			padding-inline: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.mw-board {
			padding-inline: 2rem;
		}
	}
	.mw-board-col {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
		padding: 0.625rem;
		border-radius: var(--exec-panel-radius, 12px);
		background: var(--exec-panel-bg);
		box-shadow: var(--exec-panel-shadow);
		min-width: 0;
	}
	.mw-board-col-hd {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.125rem 0.25rem;
	}
	.mw-board-card {
		padding: 0.625rem 0.75rem;
		border-radius: 0.5rem;
		background: var(--surface-raised);
		box-shadow: inset 0 0 0 1px var(--line-subtle);
	}
	.mw-board-card:hover {
		background: var(--surface-overlay);
	}
</style>
