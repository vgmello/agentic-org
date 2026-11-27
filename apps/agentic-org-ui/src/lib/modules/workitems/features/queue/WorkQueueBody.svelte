<!--
  @component
  Results panel body for WorkQueueView: Needs-me list, empty states, filter bar, Board, or List.
-->
<script lang="ts">
	import { gutters } from '$lib/components/classes';
	import {
		hasActiveFilters,
		type LayoutMode,
		type QueueFilters,
		type ShowView
	} from '../../filters';
	import {
		isOpen,
		type BoardMode,
		type GroupBy,
		type WorkSortBy,
		type WorkSortDir
	} from '../../present';
	import type { TaskRow, TaskType, WorkQueue } from '../../types';
	import type { ActiveFilterChip } from './active-chips';
	import NeedsMeList from './NeedsMeList.svelte';
	import WorkBoard from './WorkBoard.svelte';
	import WorkList from './WorkList.svelte';
	import WorkQueueEmptyState from './WorkQueueEmptyState.svelte';
	import WorkQueueFilterBar from './WorkQueueFilterBar.svelte';

	let {
		uid,
		organizationId,
		queue,
		isOrgScope,
		isStale,
		projectArchived,
		tabSelected,
		activeShowLabel,
		showMode,
		activeTab,
		layout,
		boardMode,
		primaryGroup,
		effectiveSubgroup,
		sortBy,
		sortDir,
		showDividers,
		showArchived,
		archivedIds,
		openRowRef,
		usesHeaderScopedSearch,
		shortcutsEnabled,
		showNoEmployeeNotice,
		activeChips,
		filters,
		scopeMine,
		needsMeItems,
		typeTotal,
		filtered,
		boardRows,
		query = $bindable(''),
		onOpenDetail,
		onCreate,
		onClearFilters,
		onShowAll,
		onShowByProject,
		onHideArchived,
		onRemoveChip,
		onChanged
	}: {
		uid: string;
		organizationId: string;
		queue: WorkQueue;
		isOrgScope: boolean;
		isStale: boolean;
		projectArchived: boolean;
		tabSelected: boolean;
		activeShowLabel: string;
		showMode: ShowView;
		activeTab: TaskType;
		layout: LayoutMode;
		boardMode: BoardMode;
		primaryGroup: GroupBy;
		effectiveSubgroup: GroupBy;
		sortBy: WorkSortBy;
		sortDir: WorkSortDir;
		showDividers: boolean;
		showArchived: boolean;
		archivedIds: ReadonlySet<string>;
		openRowRef: string | null;
		usesHeaderScopedSearch: boolean;
		shortcutsEnabled: boolean;
		showNoEmployeeNotice: boolean;
		activeChips: ActiveFilterChip[];
		filters: QueueFilters;
		scopeMine: boolean;
		needsMeItems: TaskRow[];
		typeTotal: number;
		filtered: TaskRow[];
		boardRows: TaskRow[];
		query: string;
		onOpenDetail: (row: TaskRow, triggerEl: HTMLElement | null) => void;
		onCreate: (kind: TaskType) => void;
		onClearFilters: () => void;
		onShowAll: () => void;
		onShowByProject: () => void;
		onHideArchived: () => void;
		onRemoveChip: (chip: ActiveFilterChip) => void;
		onChanged: () => void;
	} = $props();
</script>

<div
	id="{uid}-panel"
	role={tabSelected ? 'tabpanel' : 'region'}
	aria-labelledby={tabSelected ? `${uid}-tab-${activeTab}` : undefined}
	aria-label={tabSelected ? undefined : activeShowLabel}
	class={[
		'flex min-w-0 flex-1 flex-col gap-3',
		isOrgScope && 'work-results-canvas py-4',
		isOrgScope && gutters
	]}
>
	{#if isOrgScope && showMode === 'needs_me'}
		<NeedsMeList
			rows={needsMeItems}
			sources={queue.needsMeSources}
			onopen={isOrgScope ? onOpenDetail : undefined}
		/>
	{:else if queue.rows.length === 0}
		<WorkQueueEmptyState
			kind="empty_org"
			createDisabled={isStale || projectArchived}
			{onCreate}
			{onClearFilters}
			{onShowAll}
			{onShowByProject}
		/>
	{:else}
		<WorkQueueFilterBar
			{uid}
			{isOrgScope}
			{usesHeaderScopedSearch}
			{shortcutsEnabled}
			{showNoEmployeeNotice}
			{activeChips}
			{showArchived}
			bind:query
			{onShowAll}
			{onHideArchived}
			{onRemoveChip}
			{onClearFilters}
		/>

		{#if isOrgScope && (showMode === 'by_project' || showMode === 'by_assignee')}
			{#if filtered.length === 0}
				<WorkQueueEmptyState
					kind="empty_grouped"
					{onCreate}
					{onClearFilters}
					{onShowAll}
					{onShowByProject}
				/>
			{:else}
				<WorkList
					rows={filtered}
					tab="all"
					groupBy={showMode === 'by_project' ? 'project' : 'assignee'}
					subgroupBy={effectiveSubgroup}
					{sortBy}
					{sortDir}
					showProject={showMode !== 'by_project'}
					{showDividers}
					{archivedIds}
					selectedRef={openRowRef}
					onopen={onOpenDetail}
				/>
			{/if}
		{:else if typeTotal === 0}
			<WorkQueueEmptyState
				kind="empty_type"
				{activeTab}
				createDisabled={isStale || projectArchived}
				{onCreate}
				{onClearFilters}
				{onShowAll}
				{onShowByProject}
			/>
		{:else if (activeTab === 'task' && layout === 'board' ? boardRows.length : filtered.length) === 0}
			<WorkQueueEmptyState
				kind="empty_filtered"
				{activeTab}
				noOpenBugs={activeTab === 'bug' &&
					filters.status === 'open' &&
					queue.rows.filter((r) => r.type === 'bug' && isOpen(r.status)).length === 0}
				hasActiveFilter={hasActiveFilters(filters)}
				{scopeMine}
				{onCreate}
				{onClearFilters}
				{onShowAll}
				{onShowByProject}
			/>
		{:else if activeTab === 'task' && layout === 'board'}
			<WorkBoard
				{organizationId}
				rows={boardRows}
				{boardMode}
				showProject={isOrgScope}
				stale={isStale}
				{archivedIds}
				selectedRef={openRowRef}
				searchQuery={filters.query}
				onchanged={onChanged}
				onopen={isOrgScope ? onOpenDetail : undefined}
			/>
		{:else}
			<WorkList
				rows={filtered}
				tab={activeTab}
				groupBy={primaryGroup}
				subgroupBy={effectiveSubgroup}
				{sortBy}
				{sortDir}
				showProject={isOrgScope}
				{showDividers}
				{archivedIds}
				selectedRef={openRowRef}
				onopen={isOrgScope ? onOpenDetail : undefined}
			/>
		{/if}
	{/if}
</div>
