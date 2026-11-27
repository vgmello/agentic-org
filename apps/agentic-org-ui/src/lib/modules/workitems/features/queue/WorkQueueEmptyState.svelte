<!--
  @component
  Empty states for WorkQueueView (no items at all, empty tab, or zero filtered results).
-->
<script lang="ts">
	import EmptyState from '$lib/components/EmptyState.svelte';
	import { Button } from '$lib/components/ui/button';
	import { mobileButton } from '$lib/components/classes';
	import type { TaskType } from '../../types';

	let {
		kind,
		activeTab = 'task',
		noOpenBugs = false,
		hasActiveFilter = false,
		scopeMine = false,
		createDisabled = false,
		onCreate,
		onClearFilters,
		onShowAll,
		onShowByProject
	}: {
		kind: 'empty_org' | 'empty_grouped' | 'empty_type' | 'empty_filtered';
		activeTab?: TaskType;
		noOpenBugs?: boolean;
		hasActiveFilter?: boolean;
		scopeMine?: boolean;
		createDisabled?: boolean;
		onCreate: (kind: TaskType) => void;
		onClearFilters: () => void;
		onShowAll: () => void;
		onShowByProject: () => void;
	} = $props();
</script>

{#if kind === 'empty_org'}
	<EmptyState
		level={3}
		title="No work yet"
		description="Capture a task, bug or request with an accountable team even when there are no projects."
	>
		{#snippet action()}
			<div class="flex flex-wrap justify-center gap-2">
				<Button
					type="button"
					class={mobileButton}
					disabled={createDisabled}
					onclick={() => onCreate('task')}>New task</Button
				>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={createDisabled}
					onclick={() => onCreate('request')}>New request</Button
				>
			</div>
		{/snippet}
	</EmptyState>
{:else if kind === 'empty_grouped'}
	<EmptyState
		level={3}
		title="No items match these filters"
		description="Try clearing or adjusting your filters."
	>
		{#snippet action()}
			<Button type="button" variant="outline" class={mobileButton} onclick={onClearFilters}
				>Clear filters</Button
			>
		{/snippet}
	</EmptyState>
{:else if kind === 'empty_type'}
	{#if activeTab === 'bug'}
		<EmptyState level={3} title="No bugs yet" description="Report one when something doesn't work.">
			{#snippet action()}
				<Button
					type="button"
					class={mobileButton}
					disabled={createDisabled}
					onclick={() => onCreate('bug')}>Report bug</Button
				>
			{/snippet}
		</EmptyState>
	{:else if activeTab === 'request'}
		<EmptyState
			level={3}
			title="No requests yet"
			description="Capture what you need and choose an accountable team."
		>
			{#snippet action()}
				<Button
					type="button"
					class={mobileButton}
					disabled={createDisabled}
					onclick={() => onCreate('request')}>New request</Button
				>
			{/snippet}
		</EmptyState>
	{:else}
		<EmptyState
			level={3}
			title="No tasks yet"
			description="Create a task with a title and accountable team."
		>
			{#snippet action()}
				<Button
					type="button"
					class={mobileButton}
					disabled={createDisabled}
					onclick={() => onCreate('task')}>New task</Button
				>
			{/snippet}
		</EmptyState>
	{/if}
{:else}
	<EmptyState
		level={3}
		title={noOpenBugs
			? 'No open bugs.'
			: activeTab === 'bug'
				? 'No bugs match these filters.'
				: 'No items match these filters.'}
		description={noOpenBugs
			? 'Every recorded bug in this scope is closed.'
			: 'Clear filters to see all items.'}
	>
		{#snippet action()}
			{#if hasActiveFilter}
				<Button type="button" variant="outline" class={mobileButton} onclick={onClearFilters}
					>Clear filters</Button
				>
			{:else if scopeMine}
				<div class="flex flex-wrap justify-center gap-2">
					<Button type="button" variant="outline" class={mobileButton} onclick={onShowAll}
						>Show all</Button
					>
					<Button type="button" variant="outline" class={mobileButton} onclick={onShowByProject}
						>Show by project</Button
					>
				</div>
			{/if}
		{/snippet}
	</EmptyState>
{/if}
