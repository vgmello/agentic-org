<!--
  @component
  Display popover menu inside WorkToolbarMenus (Group by, Subgroup by, Sort by, Direction,
  Row dividers, Board columns, Show archived).
-->
<script lang="ts">
	import { mergeProps, Tooltip } from 'bits-ui';
	import { SlidersHorizontal } from '@lucide/svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import {
		GROUP_WORD,
		WORK_SORT_OPTIONS,
		type BoardMode,
		type GroupBy,
		type WorkSortBy,
		type WorkSortDir
	} from '../../present';

	let {
		isListSurface,
		showGroupMenu,
		primaryGroup,
		groupOptions,
		subgroupCandidates,
		effectiveSubgroup,
		sortBy,
		sortDir,
		showDividers,
		showArchived = false,
		boardMode,
		itemHit,
		menuMotion,
		onGroupChange,
		onSubgroupChange,
		onSortByChange,
		onSortDirChange,
		onDividersChange,
		onBoardModeChange,
		onShowArchivedChange
	}: {
		isListSurface: boolean;
		showGroupMenu: boolean;
		primaryGroup: GroupBy;
		groupOptions: GroupBy[];
		subgroupCandidates: GroupBy[];
		effectiveSubgroup: GroupBy;
		sortBy: WorkSortBy;
		sortDir: WorkSortDir;
		showDividers: boolean;
		showArchived?: boolean;
		boardMode: BoardMode;
		itemHit: string;
		menuMotion: string;
		onGroupChange: (next: GroupBy) => void;
		onSubgroupChange: (next: GroupBy) => void;
		onSortByChange: (next: WorkSortBy) => void;
		onSortDirChange: (next: WorkSortDir) => void;
		onDividersChange: (show: boolean) => void;
		onBoardModeChange: (next: BoardMode) => void;
		onShowArchivedChange?: (show: boolean) => void;
	} = $props();

	let open = $state(false);

	const displayActive = $derived(
		sortBy !== 'default' ||
			sortDir !== 'asc' ||
			showDividers ||
			showArchived ||
			effectiveSubgroup !== 'none'
	);
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

		{#if onShowArchivedChange}
			<DropdownMenu.Separator />
			<DropdownMenu.CheckboxItem
				checked={showArchived}
				onCheckedChange={(checked) => onShowArchivedChange(Boolean(checked))}
				data-testid="work-show-archived-toggle"
				class={itemHit}
			>
				Show archived
			</DropdownMenu.CheckboxItem>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>
