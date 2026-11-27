<!--
  @component
  Filter popover menu inside WorkToolbarMenus (Scope/Saved, Status, Priority, Severity, Project,
  Assigned team, Accountable team, Assignee, Reporter, Reset filters).
-->
<script lang="ts">
	import { mergeProps, Tooltip } from 'bits-ui';
	import { ListFilter } from '@lucide/svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import {
		SAVED_FILTERS,
		type QueueFilters,
		type SavedFilterId,
		type ShowView
	} from '../../filters';
	import {
		PRIORITIES,
		PRIORITY_WORD,
		SEVERITIES,
		SEVERITY_WORD,
		STATUSES,
		STATUS_WORD,
		severityWord
	} from '../../present';
	import type { Priority, Severity, TaskStatus, TaskType } from '../../types';

	let {
		isOrgScope,
		showMode,
		activeTab,
		filters,
		scopeMine,
		savedCounts,
		hasFilterBadge,
		uniqueProjects,
		uniqueTeams,
		uniquePeople,
		itemHit,
		menuMotion,
		onSavedSelect,
		onFiltersChange,
		onProjectChange,
		onAssigneeChange,
		onClearFilters
	}: {
		isOrgScope: boolean;
		showMode: ShowView;
		activeTab: TaskType;
		filters: QueueFilters;
		scopeMine: boolean;
		savedCounts: Record<SavedFilterId, number>;
		hasFilterBadge: boolean;
		uniqueProjects: [string, string][];
		uniqueTeams: [string, string][];
		uniquePeople: [string, string][];
		itemHit: string;
		menuMotion: string;
		onSavedSelect: (saved: SavedFilterId) => void;
		onFiltersChange: (patch: Partial<QueueFilters>) => void;
		onProjectChange: (projectId: string) => void;
		onAssigneeChange: (assigneeId: string) => void;
		onClearFilters: () => void;
	} = $props();

	let open = $state(false);

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

<DropdownMenu.Root bind:open>
	<Tooltip.Root disabled={open}>
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
					{@const label = activeTab === 'request' && sf.requestLabel ? sf.requestLabel : sf.label}
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
							{filters.severity === 'unset' ? severityWord(null) : SEVERITY_WORD[filters.severity]}
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
