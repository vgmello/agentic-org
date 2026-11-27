<!--
  @component
  Fallback local search box, no-employee notice, and active filter chips bar for WorkQueueView.
-->
<script lang="ts">
	import { X } from '@lucide/svelte';
	import { Input } from '$lib/components/ui/input';
	import { inputClass } from '$lib/components/classes';
	import type { ActiveFilterChip } from './active-chips';
	import { focusHeadingFromSearch } from './work-queue-helpers';

	let {
		uid,
		isOrgScope,
		usesHeaderScopedSearch,
		shortcutsEnabled,
		showNoEmployeeNotice,
		activeChips,
		showArchived,
		query = $bindable(''),
		onShowAll,
		onHideArchived,
		onRemoveChip,
		onClearFilters
	}: {
		uid: string;
		isOrgScope: boolean;
		usesHeaderScopedSearch: boolean;
		shortcutsEnabled: boolean;
		showNoEmployeeNotice: boolean;
		activeChips: readonly ActiveFilterChip[];
		showArchived: boolean;
		query: string;
		onShowAll: () => void;
		onHideArchived: () => void;
		onRemoveChip: (chip: ActiveFilterChip) => void;
		onClearFilters: () => void;
	} = $props();
</script>

{#if !usesHeaderScopedSearch}
	<div class="relative w-full max-w-xs">
		<label for="{uid}-search" class="sr-only"
			>{isOrgScope ? 'Search work' : 'Search work in this view'}</label
		>
		<Input
			id="{uid}-search"
			type="search"
			bind:value={query}
			onkeydown={focusHeadingFromSearch}
			placeholder="Search title or T- ref…"
			aria-keyshortcuts={shortcutsEnabled ? '/' : undefined}
			class={inputClass}
		/>
		<span id="{uid}-search-scope" class="sr-only">
			Searches shared titles, references, teams and projects.
		</span>
	</div>
{/if}

{#if showNoEmployeeNotice}
	<div class="work-notice" data-testid="no-employee-note">
		<span>
			This account has no employee record, so "my" filters match nothing. Choose All or filter by
			team or person.
		</span>
		<button type="button" class="work-notice-action" onclick={onShowAll}>Show all</button>
	</div>
{/if}

{#if activeChips.length > 0 || showArchived}
	<div class="work-active-filters" data-testid="active-filter-bar">
		{#if showArchived}
			<button
				type="button"
				class="work-filter-chip work-archived-chip"
				data-testid="work-showing-archived-chip"
				onclick={onHideArchived}
			>
				<span>Showing archived</span>
				<X class="size-3 shrink-0 opacity-70" aria-hidden="true" />
			</button>
		{/if}
		{#each activeChips as chip (chip.id)}
			<button type="button" class="work-filter-chip" onclick={() => onRemoveChip(chip)}>
				<span>{chip.label}</span>
				<X class="size-3 shrink-0 opacity-70" aria-hidden="true" />
			</button>
		{/each}
		{#if activeChips.length > 0}
			<button type="button" class="work-clear-btn" onclick={onClearFilters}>Clear filters</button>
		{/if}
	</div>
{/if}

<style>
	.work-notice {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 12px;
		padding: 8px 12px;
		border-radius: var(--exec-ctrl-radius, 6px);
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line));
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.78125rem;
	}
	.work-notice-action {
		font-weight: 600;
		color: var(--tone-accent-ink, var(--link));
		cursor: pointer;
	}
	.work-notice-action:hover {
		text-decoration: underline;
	}
	.work-active-filters {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
	}
	.work-filter-chip {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		min-height: 26px;
		padding: 2px 9px;
		border-radius: 999px;
		background: var(--tone-accent-bg, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring, var(--line));
		color: var(--exec-ink, var(--fg));
		font-size: 0.75rem;
		font-weight: 500;
		cursor: pointer;
	}
	.work-filter-chip:hover {
		background: var(--exec-surface-hi, var(--surface-overlay));
	}
	.work-clear-btn {
		display: inline-flex;
		align-items: center;
		min-height: 26px;
		padding: 2px 8px;
		border-radius: var(--exec-ctrl-radius, 6px);
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
	}
	.work-clear-btn:hover {
		color: var(--exec-ink, var(--fg));
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
	}
</style>
