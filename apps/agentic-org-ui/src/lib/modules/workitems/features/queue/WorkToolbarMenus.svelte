<!--
  @component
  POC-aligned Work toolbar controls: Show view menu, Layout toggle (List/Board + 4 stages/6 statuses),
  Filter icon popover menu (Scope/Saved, Status, Severity, Priority, Project, Teams, People),
  Display icon popover menu (Group by, Subgroup by, Sort by, Direction, Row dividers),
  and primary split Create CTA (+ New task / Report bug / New request).
-->
<script lang="ts">
	import { mergeProps, Tooltip } from 'bits-ui';
	import { ChevronDown, Columns3, List, ListFilter, Plus, SlidersHorizontal } from '@lucide/svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import {
		SAVED_FILTERS,
		SHOW_VIEWS,
		type LayoutMode,
		type QueueFilters,
		type SavedFilterId,
		type ShowView
	} from '../../filters';
	import {
		GROUP_WORD,
		PRIORITIES,
		PRIORITY_WORD,
		SEVERITIES,
		SEVERITY_WORD,
		STATUSES,
		STATUS_WORD,
		WORK_SORT_OPTIONS,
		severityWord,
		type BoardMode,
		type GroupBy,
		type WorkSortBy,
		type WorkSortDir
	} from '../../present';
	import type { Priority, Severity, TaskStatus, TaskType } from '../../types';

	let {
		isOrgScope,
		showMode,
		needsMeCount,
		activeTab,
		layout,
		boardMode,
		filters,
		scopeMine,
		savedCounts,
		hasFilterBadge,
		isListSurface,
		showGroupMenu,
		primaryGroup,
		groupOptions,
		subgroupCandidates,
		effectiveSubgroup,
		sortBy,
		sortDir,
		showDividers,
		uniqueProjects,
		uniqueTeams,
		uniquePeople,
		showLayoutToggle,
		canCreate,
		createDisabled,
		onShowChange,
		onLayoutChange,
		onBoardModeChange,
		onSavedSelect,
		onFiltersChange,
		onProjectChange,
		onAssigneeChange,
		onClearFilters,
		onGroupChange,
		onSubgroupChange,
		onSortByChange,
		onSortDirChange,
		onDividersChange,
		onCreate
	}: {
		isOrgScope: boolean;
		showMode: ShowView;
		needsMeCount: number;
		activeTab: TaskType;
		layout: LayoutMode;
		boardMode: BoardMode;
		filters: QueueFilters;
		scopeMine: boolean;
		savedCounts: Record<SavedFilterId, number>;
		hasFilterBadge: boolean;
		isListSurface: boolean;
		showGroupMenu: boolean;
		primaryGroup: GroupBy;
		groupOptions: GroupBy[];
		subgroupCandidates: GroupBy[];
		effectiveSubgroup: GroupBy;
		sortBy: WorkSortBy;
		sortDir: WorkSortDir;
		showDividers: boolean;
		uniqueProjects: [string, string][];
		uniqueTeams: [string, string][];
		uniquePeople: [string, string][];
		showLayoutToggle: boolean;
		canCreate: boolean;
		createDisabled: boolean;
		onShowChange: (next: ShowView) => void;
		onLayoutChange: (next: LayoutMode) => void;
		onBoardModeChange: (next: BoardMode) => void;
		onSavedSelect: (saved: SavedFilterId) => void;
		onFiltersChange: (patch: Partial<QueueFilters>) => void;
		onProjectChange: (projectId: string) => void;
		onAssigneeChange: (assigneeId: string) => void;
		onClearFilters: () => void;
		onGroupChange: (next: GroupBy) => void;
		onSubgroupChange: (next: GroupBy) => void;
		onSortByChange: (next: WorkSortBy) => void;
		onSortDirChange: (next: WorkSortDir) => void;
		onDividersChange: (show: boolean) => void;
		onCreate: (kind: TaskType) => void;
	} = $props();

	let filterMenuOpen = $state(false);
	let displayMenuOpen = $state(false);
	let showMenuOpen = $state(false);
	let createMenuOpen = $state(false);

	const showLabel = $derived(SHOW_VIEWS.find((v) => v.id === showMode)?.label ?? 'Mine');

	const displayActive = $derived(
		sortBy !== 'default' || sortDir !== 'asc' || showDividers || effectiveSubgroup !== 'none'
	);

	const primaryCreate = $derived.by<{ kind: TaskType; label: string }>(() => {
		if (!isOrgScope || showMode === 'mine') {
			if (activeTab === 'bug') return { kind: 'bug', label: 'Report bug' };
			if (activeTab === 'request') return { kind: 'request', label: 'New request' };
		}
		return { kind: 'task', label: 'New task' };
	});

	const secondaryCreates = $derived(
		(
			[
				{ kind: 'task', label: 'New task' },
				{ kind: 'bug', label: 'Report bug' },
				{ kind: 'request', label: 'New request' }
			] as const
		).filter((c) => c.kind !== primaryCreate.kind)
	);

	const itemHit = 'min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11';
	const menuMotion =
		'triage-menu-surface motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!';

	function selectedProjectLabel(): string {
		if (!filters.projectId) return '';
		if (filters.projectId === 'none') return 'No project';
		return uniqueProjects.find(([id]) => id === filters.projectId)?.[1] ?? '';
	}

	function selectedTeamLabel(id: string): string {
		if (!id) return '';
		if (id === 'none') return 'Unassigned';
		return uniqueTeams.find(([tid]) => tid === id)?.[1] ?? '';
	}

	function selectedPersonLabel(id: string): string {
		if (!id) return '';
		if (id === 'none') return 'Unassigned';
		return uniquePeople.find(([pid]) => pid === id)?.[1] ?? '';
	}
</script>

<div class="work-controls">
	{#if isOrgScope}
		<DropdownMenu.Root bind:open={showMenuOpen}>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						aria-label="Show: {showLabel}"
						data-testid="show-menu"
						data-value={showMode}
						class="work-menu-trigger"
					>
						<span>{showLabel}</span>
						{#if showMode === 'needs_me' && needsMeCount > 0}
							<span class="work-trigger-count">{needsMeCount}</span>
						{/if}
						<ChevronDown class="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content preventScroll={false} align="end" class="{menuMotion} w-48">
				<DropdownMenu.Label>View</DropdownMenu.Label>
				<DropdownMenu.RadioGroup
					value={showMode}
					onValueChange={(val) => {
						if (val) onShowChange(val as ShowView);
					}}
				>
					{#each SHOW_VIEWS as v (v.id)}
						<DropdownMenu.RadioItem
							value={v.id}
							closeOnSelect={true}
							data-show-option={v.id}
							class="flex items-center justify-between gap-2 pr-8 {itemHit}"
						>
							<span>{v.label}</span>
							{#if v.id === 'needs_me'}
								<span class="font-mono text-xs text-fg-muted tabular-nums">{needsMeCount}</span>
							{/if}
						</DropdownMenu.RadioItem>
					{/each}
				</DropdownMenu.RadioGroup>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/if}

	{#if showLayoutToggle}
		<div class="work-layout-toggle" role="group" aria-label="Layout">
			<button
				type="button"
				aria-pressed={layout === 'list'}
				onclick={() => onLayoutChange('list')}
				class="queue-tab work-toggle-tab"
			>
				<List class="size-3.5 shrink-0" aria-hidden="true" />
				<span>List</span>
			</button>
			<button
				type="button"
				aria-pressed={layout === 'board'}
				onclick={() => onLayoutChange('board')}
				class="queue-tab work-toggle-tab"
			>
				<Columns3 class="size-3.5 shrink-0" aria-hidden="true" />
				<span>Board</span>
			</button>
		</div>

		{#if layout === 'board'}
			<div
				role="group"
				aria-label="Board columns"
				data-testid="board-mode-toggle"
				class="work-board-mode"
			>
				<button
					type="button"
					aria-pressed={boardMode === 'stages'}
					onclick={() => onBoardModeChange('stages')}
					class="work-mode-btn">4 stages</button
				>
				<button
					type="button"
					aria-pressed={boardMode === 'statuses'}
					onclick={() => onBoardModeChange('statuses')}
					class="work-mode-btn">6 statuses</button
				>
			</div>
		{/if}
	{/if}

	{#if !isOrgScope || showMode !== 'needs_me'}
		<div class="work-icon-controls" role="group" aria-label="Work filter and display controls">
			<!-- Filter popover menu -->
			<DropdownMenu.Root bind:open={filterMenuOpen}>
				<Tooltip.Root disabled={filterMenuOpen}>
					<Tooltip.Trigger>
						{#snippet child({ props: tipProps })}
							<DropdownMenu.Trigger>
								{#snippet child({ props: menuProps })}
									<button
										{...mergeProps(tipProps, menuProps)}
										type="button"
										aria-label="Filter work"
										data-testid="work-filter-btn"
										data-active={hasFilterBadge ? '' : undefined}
										class="queue-icon-btn"
									>
										<ListFilter class="size-5" aria-hidden="true" />
									</button>
								{/snippet}
							</DropdownMenu.Trigger>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content
							side="bottom"
							sideOffset={6}
							class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
							>Filter work</Tooltip.Content
						>
					</Tooltip.Portal>
				</Tooltip.Root>
				<DropdownMenu.Content
					preventScroll={false}
					align="end"
					class="{menuMotion} max-h-[min(70dvh,32rem)] w-64 max-w-[calc(100vw-2rem)] overflow-y-auto"
				>
					{#if !isOrgScope || (showMode !== 'by_project' && showMode !== 'by_assignee')}
						<DropdownMenu.Label>Scope</DropdownMenu.Label>
						<DropdownMenu.RadioGroup
							value={scopeMine ? '' : filters.saved}
							onValueChange={(val) => {
								if (val) onSavedSelect(val as SavedFilterId);
							}}
							data-testid="saved-filters"
						>
							{#each SAVED_FILTERS as sf (sf.id)}
								{@const label =
									activeTab === 'request' && sf.requestLabel ? sf.requestLabel : sf.label}
								<DropdownMenu.RadioItem
									value={sf.id}
									closeOnSelect={true}
									data-filter={sf.id}
									class="flex items-center justify-between gap-2 pr-8 {itemHit}"
								>
									<span>{label}</span>
									<span class="font-mono text-xs text-fg-muted tabular-nums">
										{savedCounts[sf.id]}
									</span>
								</DropdownMenu.RadioItem>
							{/each}
						</DropdownMenu.RadioGroup>
						<DropdownMenu.Separator />
					{/if}

					<DropdownMenu.Label>Filter by</DropdownMenu.Label>

					<DropdownMenu.Sub>
						<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
							<span>Status</span>
							{#if filters.status !== 'all'}
								<span class="ml-auto text-xs text-link">
									{filters.status === 'open' ? 'Open' : STATUS_WORD[filters.status]}
								</span>
							{/if}
						</DropdownMenu.SubTrigger>
						<DropdownMenu.SubContent class="{menuMotion} w-48">
							<DropdownMenu.RadioGroup
								value={filters.status}
								onValueChange={(val) => {
									if (val) onFiltersChange({ status: val as TaskStatus | 'open' | 'all' });
								}}
							>
								<DropdownMenu.RadioItem value="all" closeOnSelect={true} class={itemHit}>
									All statuses
								</DropdownMenu.RadioItem>
								<DropdownMenu.RadioItem value="open" closeOnSelect={true} class={itemHit}>
									Open
								</DropdownMenu.RadioItem>
								{#each STATUSES as s (s)}
									<DropdownMenu.RadioItem value={s} closeOnSelect={true} class={itemHit}>
										{STATUS_WORD[s]}
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
						</DropdownMenu.SubContent>
					</DropdownMenu.Sub>

					<DropdownMenu.Sub>
						<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
							<span>Priority</span>
							{#if filters.priority !== 'all'}
								<span class="ml-auto text-xs text-link">{PRIORITY_WORD[filters.priority]}</span>
							{/if}
						</DropdownMenu.SubTrigger>
						<DropdownMenu.SubContent class="{menuMotion} w-44">
							<DropdownMenu.RadioGroup
								value={filters.priority}
								onValueChange={(val) => {
									if (val) onFiltersChange({ priority: val as Priority | 'all' });
								}}
							>
								<DropdownMenu.RadioItem value="all" closeOnSelect={true} class={itemHit}>
									All priorities
								</DropdownMenu.RadioItem>
								{#each PRIORITIES as p (p)}
									<DropdownMenu.RadioItem value={p} closeOnSelect={true} class={itemHit}>
										{PRIORITY_WORD[p]}
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
						</DropdownMenu.SubContent>
					</DropdownMenu.Sub>

					{#if activeTab === 'bug' && (!isOrgScope || showMode === 'mine')}
						<DropdownMenu.Sub>
							<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
								<span>Severity</span>
								{#if filters.severity !== 'all'}
									<span class="ml-auto text-xs text-link">
										{filters.severity === 'unset'
											? severityWord(null)
											: SEVERITY_WORD[filters.severity]}
									</span>
								{/if}
							</DropdownMenu.SubTrigger>
							<DropdownMenu.SubContent class="{menuMotion} w-44">
								<DropdownMenu.RadioGroup
									value={filters.severity}
									onValueChange={(val) => {
										if (val) onFiltersChange({ severity: val as Severity | 'unset' | 'all' });
									}}
								>
									<DropdownMenu.RadioItem value="all" closeOnSelect={true} class={itemHit}>
										All severities
									</DropdownMenu.RadioItem>
									{#each SEVERITIES as sev (sev)}
										<DropdownMenu.RadioItem value={sev} closeOnSelect={true} class={itemHit}>
											{SEVERITY_WORD[sev]}
										</DropdownMenu.RadioItem>
									{/each}
									<DropdownMenu.RadioItem value="unset" closeOnSelect={true} class={itemHit}>
										{severityWord(null)}
									</DropdownMenu.RadioItem>
								</DropdownMenu.RadioGroup>
							</DropdownMenu.SubContent>
						</DropdownMenu.Sub>
					{/if}

					{#if isOrgScope}
						<DropdownMenu.Sub>
							<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
								<span>Project</span>
								{#if filters.projectId}
									<span class="ml-auto max-w-28 truncate text-xs text-link">
										{selectedProjectLabel()}
									</span>
								{/if}
							</DropdownMenu.SubTrigger>
							<DropdownMenu.SubContent class="{menuMotion} max-h-72 w-56 overflow-y-auto">
								<DropdownMenu.RadioGroup
									value={filters.projectId}
									onValueChange={(val) => onProjectChange(val)}
								>
									<DropdownMenu.RadioItem value="" closeOnSelect={true} class={itemHit}>
										All projects
									</DropdownMenu.RadioItem>
									<DropdownMenu.RadioItem value="none" closeOnSelect={true} class={itemHit}>
										No project
									</DropdownMenu.RadioItem>
									{#each uniqueProjects as [id, name] (id)}
										<DropdownMenu.RadioItem value={id} closeOnSelect={true} class={itemHit}>
											{name}
										</DropdownMenu.RadioItem>
									{/each}
								</DropdownMenu.RadioGroup>
							</DropdownMenu.SubContent>
						</DropdownMenu.Sub>
					{/if}

					<DropdownMenu.Sub>
						<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
							<span>Assigned team</span>
							{#if filters.assignedTeamId}
								<span class="ml-auto max-w-24 truncate text-xs text-link">
									{selectedTeamLabel(filters.assignedTeamId)}
								</span>
							{/if}
						</DropdownMenu.SubTrigger>
						<DropdownMenu.SubContent class="{menuMotion} max-h-72 w-56 overflow-y-auto">
							<DropdownMenu.RadioGroup
								value={filters.assignedTeamId}
								onValueChange={(val) => onFiltersChange({ assignedTeamId: val })}
							>
								<DropdownMenu.RadioItem value="" closeOnSelect={true} class={itemHit}>
									All assigned teams
								</DropdownMenu.RadioItem>
								<DropdownMenu.RadioItem value="none" closeOnSelect={true} class={itemHit}>
									Unassigned team
								</DropdownMenu.RadioItem>
								{#each uniqueTeams as [id, name] (id)}
									<DropdownMenu.RadioItem value={id} closeOnSelect={true} class={itemHit}>
										{name}
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
						</DropdownMenu.SubContent>
					</DropdownMenu.Sub>

					<DropdownMenu.Sub>
						<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
							<span>Accountable team</span>
							{#if filters.teamId}
								<span class="ml-auto max-w-24 truncate text-xs text-link">
									{selectedTeamLabel(filters.teamId)}
								</span>
							{/if}
						</DropdownMenu.SubTrigger>
						<DropdownMenu.SubContent class="{menuMotion} max-h-72 w-56 overflow-y-auto">
							<DropdownMenu.RadioGroup
								value={filters.teamId}
								onValueChange={(val) => onFiltersChange({ teamId: val })}
							>
								<DropdownMenu.RadioItem value="" closeOnSelect={true} class={itemHit}>
									All accountable teams
								</DropdownMenu.RadioItem>
								{#each uniqueTeams as [id, name] (id)}
									<DropdownMenu.RadioItem value={id} closeOnSelect={true} class={itemHit}>
										{name}
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
						</DropdownMenu.SubContent>
					</DropdownMenu.Sub>

					<DropdownMenu.Sub>
						<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
							<span>Assignee</span>
							{#if filters.assigneeId}
								<span class="ml-auto max-w-24 truncate text-xs text-link">
									{selectedPersonLabel(filters.assigneeId)}
								</span>
							{/if}
						</DropdownMenu.SubTrigger>
						<DropdownMenu.SubContent class="{menuMotion} max-h-72 w-56 overflow-y-auto">
							<DropdownMenu.RadioGroup
								value={filters.assigneeId}
								onValueChange={(val) => onAssigneeChange(val)}
							>
								<DropdownMenu.RadioItem value="" closeOnSelect={true} class={itemHit}>
									All assignees
								</DropdownMenu.RadioItem>
								<DropdownMenu.RadioItem value="none" closeOnSelect={true} class={itemHit}>
									Unassigned
								</DropdownMenu.RadioItem>
								{#each uniquePeople as [id, name] (id)}
									<DropdownMenu.RadioItem value={id} closeOnSelect={true} class={itemHit}>
										{name}
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
						</DropdownMenu.SubContent>
					</DropdownMenu.Sub>

					<DropdownMenu.Sub>
						<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
							<span>Reporter</span>
							{#if filters.reporterId}
								<span class="ml-auto max-w-24 truncate text-xs text-link">
									{selectedPersonLabel(filters.reporterId)}
								</span>
							{/if}
						</DropdownMenu.SubTrigger>
						<DropdownMenu.SubContent class="{menuMotion} max-h-72 w-56 overflow-y-auto">
							<DropdownMenu.RadioGroup
								value={filters.reporterId}
								onValueChange={(val) => onFiltersChange({ reporterId: val })}
							>
								<DropdownMenu.RadioItem value="" closeOnSelect={true} class={itemHit}>
									All reporters
								</DropdownMenu.RadioItem>
								{#each uniquePeople as [id, name] (id)}
									<DropdownMenu.RadioItem value={id} closeOnSelect={true} class={itemHit}>
										{name}
									</DropdownMenu.RadioItem>
								{/each}
							</DropdownMenu.RadioGroup>
						</DropdownMenu.SubContent>
					</DropdownMenu.Sub>

					{#if hasFilterBadge}
						<DropdownMenu.Separator />
						<DropdownMenu.Group>
							<DropdownMenu.Item onSelect={onClearFilters} class={itemHit}>
								Reset filters
							</DropdownMenu.Item>
						</DropdownMenu.Group>
					{/if}
				</DropdownMenu.Content>
			</DropdownMenu.Root>

			<!-- Display popover menu -->
			<DropdownMenu.Root bind:open={displayMenuOpen}>
				<Tooltip.Root disabled={displayMenuOpen}>
					<Tooltip.Trigger>
						{#snippet child({ props: tipProps })}
							<DropdownMenu.Trigger>
								{#snippet child({ props: menuProps })}
									<button
										{...mergeProps(tipProps, menuProps)}
										type="button"
										aria-label="Work display"
										data-testid="work-display-btn"
										data-active={displayActive ? '' : undefined}
										class="queue-icon-btn"
									>
										<SlidersHorizontal class="size-5" aria-hidden="true" />
									</button>
								{/snippet}
							</DropdownMenu.Trigger>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content
							side="bottom"
							sideOffset={6}
							class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
							>Work display</Tooltip.Content
						>
					</Tooltip.Portal>
				</Tooltip.Root>
				<DropdownMenu.Content
					preventScroll={false}
					align="end"
					class="{menuMotion} max-h-[min(70dvh,32rem)] w-56 max-w-[calc(100vw-2rem)] overflow-y-auto"
				>
					{#if showGroupMenu}
						<DropdownMenu.Label>Group by</DropdownMenu.Label>
						<DropdownMenu.RadioGroup
							value={primaryGroup}
							onValueChange={(val) => {
								if (val) onGroupChange(val as GroupBy);
							}}
							data-testid="group-by-menu"
						>
							{#each groupOptions as g (g)}
								<DropdownMenu.RadioItem
									value={g}
									closeOnSelect={true}
									data-group-option={g}
									class={itemHit}
								>
									{GROUP_WORD[g]}
								</DropdownMenu.RadioItem>
							{/each}
						</DropdownMenu.RadioGroup>
						<DropdownMenu.Separator />
					{/if}

					{#if isListSurface && primaryGroup !== 'none'}
						<DropdownMenu.Sub>
							<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
								<span>Subgroup by</span>
								<span class="ml-auto text-xs text-fg-muted">
									{GROUP_WORD[effectiveSubgroup]}
								</span>
							</DropdownMenu.SubTrigger>
							<DropdownMenu.SubContent class="{menuMotion} w-48" data-testid="subgroup-by-menu">
								<DropdownMenu.RadioGroup
									value={effectiveSubgroup}
									onValueChange={(val) => {
										if (val) onSubgroupChange(val as GroupBy);
									}}
								>
									{#each subgroupCandidates as g (g)}
										<DropdownMenu.RadioItem
											value={g}
											closeOnSelect={true}
											data-subgroup-option={g}
											class={itemHit}
										>
											{GROUP_WORD[g]}
										</DropdownMenu.RadioItem>
									{/each}
								</DropdownMenu.RadioGroup>
							</DropdownMenu.SubContent>
						</DropdownMenu.Sub>
						<DropdownMenu.Separator />
					{/if}

					{#if isListSurface}
						<DropdownMenu.Sub>
							<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
								<span>Sort by</span>
								<span class="ml-auto text-xs text-fg-muted">
									{WORK_SORT_OPTIONS.find((o) => o.id === sortBy)?.label ?? 'Default'}
								</span>
							</DropdownMenu.SubTrigger>
							<DropdownMenu.SubContent class="{menuMotion} w-44">
								<DropdownMenu.RadioGroup
									value={sortBy}
									onValueChange={(val) => {
										if (val) onSortByChange(val as WorkSortBy);
									}}
								>
									{#each WORK_SORT_OPTIONS as opt (opt.id)}
										<DropdownMenu.RadioItem value={opt.id} closeOnSelect={true} class={itemHit}>
											{opt.label}
										</DropdownMenu.RadioItem>
									{/each}
								</DropdownMenu.RadioGroup>
							</DropdownMenu.SubContent>
						</DropdownMenu.Sub>
						<DropdownMenu.Sub>
							<DropdownMenu.SubTrigger class="flex items-center justify-between gap-2 {itemHit}">
								<span>Direction</span>
								<span class="ml-auto text-xs text-fg-muted">
									{sortDir === 'asc' ? 'Ascending' : 'Descending'}
								</span>
							</DropdownMenu.SubTrigger>
							<DropdownMenu.SubContent class="{menuMotion} w-40">
								<DropdownMenu.RadioGroup
									value={sortDir}
									onValueChange={(val) => {
										if (val === 'asc' || val === 'desc') onSortDirChange(val);
									}}
								>
									<DropdownMenu.RadioItem value="asc" closeOnSelect={true} class={itemHit}>
										Ascending
									</DropdownMenu.RadioItem>
									<DropdownMenu.RadioItem value="desc" closeOnSelect={true} class={itemHit}>
										Descending
									</DropdownMenu.RadioItem>
								</DropdownMenu.RadioGroup>
							</DropdownMenu.SubContent>
						</DropdownMenu.Sub>
						<DropdownMenu.Separator />
						<DropdownMenu.CheckboxItem
							checked={showDividers}
							onCheckedChange={(checked) => onDividersChange(Boolean(checked))}
							class={itemHit}
						>
							Row dividers
						</DropdownMenu.CheckboxItem>
					{:else}
						<DropdownMenu.Label>Board columns</DropdownMenu.Label>
						<DropdownMenu.RadioGroup
							value={boardMode}
							onValueChange={(val) => {
								if (val === 'stages' || val === 'statuses') onBoardModeChange(val);
							}}
						>
							<DropdownMenu.RadioItem value="stages" closeOnSelect={true} class={itemHit}>
								4 stages
							</DropdownMenu.RadioItem>
							<DropdownMenu.RadioItem value="statuses" closeOnSelect={true} class={itemHit}>
								6 statuses
							</DropdownMenu.RadioItem>
						</DropdownMenu.RadioGroup>
					{/if}
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		</div>
	{/if}

	{#if canCreate}
		<div class="work-create-group">
			<button
				type="button"
				disabled={createDisabled}
				onclick={() => onCreate(primaryCreate.kind)}
				class="work-create-primary"
			>
				<Plus class="size-3.5 shrink-0" aria-hidden="true" />
				<span>{primaryCreate.label}</span>
			</button>
			<DropdownMenu.Root bind:open={createMenuOpen}>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							disabled={createDisabled}
							aria-label="More create options"
							class="work-create-chevron"
						>
							<ChevronDown class="size-3.5" aria-hidden="true" />
						</button>
					{/snippet}
				</DropdownMenu.Trigger>
				<DropdownMenu.Content preventScroll={false} align="end" class="{menuMotion} w-44">
					{#each secondaryCreates as item (item.kind)}
						<DropdownMenu.Item onSelect={() => onCreate(item.kind)} class={itemHit}>
							{item.label}
						</DropdownMenu.Item>
					{/each}
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		</div>
	{/if}
</div>

<style>
	.work-controls {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 8px;
		margin-inline-start: auto;
		margin-block: 4px;
	}
	.work-menu-trigger {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-height: var(--work-control-height, 36px);
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.work-menu-trigger:hover,
	.work-menu-trigger[aria-expanded='true'] {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.work-trigger-count {
		color: var(--tone-accent-ink, var(--brand-text));
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
	}
	.work-layout-toggle {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding-inline-start: 8px;
		border-inline-start: 1px solid var(--exec-border, var(--line));
	}
	.work-toggle-tab {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: var(--work-control-height, 36px);
		padding: 0 10px;
		border: 1px solid transparent;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.8125rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
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
	.work-board-mode {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding: 2px;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line));
	}
	.work-mode-btn {
		display: inline-flex;
		align-items: center;
		min-height: calc(var(--work-control-height, 36px) - 6px);
		padding: 0 9px;
		border-radius: calc(var(--exec-ctrl-radius, 6px) - 2px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.75rem;
		font-weight: 600;
		white-space: nowrap;
		cursor: pointer;
	}
	.work-mode-btn:hover {
		color: var(--exec-ink, var(--fg));
	}
	.work-mode-btn[aria-pressed='true'] {
		background: var(--exec-surface-hi, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	.work-icon-controls {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding-inline-start: 6px;
		border-inline-start: 1px solid var(--exec-border, var(--line));
	}
	.queue-icon-btn {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: var(--work-control-height, 36px);
		min-height: var(--work-control-height, 36px);
		border-radius: var(--exec-ctrl-radius, 6px);
		background: transparent;
		color: var(--exec-body, var(--fg-secondary));
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.queue-icon-btn:hover,
	.queue-icon-btn[aria-expanded='true'] {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.queue-icon-btn[data-active] {
		color: var(--tone-accent-ink, var(--brand-text));
	}
	.queue-icon-btn[data-active]::after {
		content: '';
		position: absolute;
		inset: 5px 5px auto auto;
		width: 5px;
		height: 5px;
		border-radius: 50%;
		background: currentColor;
	}
	.work-create-group {
		display: inline-flex;
		align-items: stretch;
		border-radius: var(--exec-ctrl-radius, 8px);
		overflow: hidden;
		background: var(--exec-cta-bg, var(--primary));
		box-shadow: var(--exec-cta-shadow, none);
	}
	.work-create-primary,
	.work-create-chevron {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-height: var(--work-control-height, 36px);
		color: var(--exec-on-accent, var(--on-emphasis));
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.work-create-primary {
		gap: 5px;
		padding: 0 11px 0 10px;
		white-space: nowrap;
	}
	.work-create-chevron {
		min-width: 28px;
		padding: 0 6px;
		border-inline-start: 1px solid var(--exec-border-hi, var(--line-strong));
	}
	.work-create-primary:hover:not(:disabled),
	.work-create-chevron:hover:not(:disabled) {
		background: var(--exec-cta-fill, var(--primary-hover));
	}
	.work-create-primary:disabled,
	.work-create-chevron:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	.work-menu-trigger:focus-visible,
	.work-toggle-tab:focus-visible,
	.work-mode-btn:focus-visible,
	.queue-icon-btn:focus-visible,
	.work-create-primary:focus-visible,
	.work-create-chevron:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}

	@media (max-width: 720px), (pointer: coarse) {
		.work-mode-btn {
			min-height: 44px;
		}
		.work-create-chevron {
			min-width: 44px;
		}
	}

	@media (forced-colors: active) {
		.work-toggle-tab[aria-pressed='true'],
		.work-mode-btn[aria-pressed='true'] {
			background: Highlight;
			border-color: Highlight;
			color: HighlightText;
		}
		.queue-icon-btn,
		.work-menu-trigger {
			border: 1px solid ButtonText;
		}
		.queue-icon-btn[data-active] {
			border: 2px solid Highlight;
		}
	}
</style>
