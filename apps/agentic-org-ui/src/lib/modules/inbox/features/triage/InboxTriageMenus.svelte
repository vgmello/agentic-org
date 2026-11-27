<script lang="ts">
	import { mergeProps, Tooltip } from 'bits-ui';
	import ListFilter from '@lucide/svelte/icons/list-filter';
	import SlidersHorizontal from '@lucide/svelte/icons/sliders-horizontal';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import type { InboxScope } from '../../types';
	import type { TriageOrder } from './triage-model';

	let {
		urgentOnly,
		scope,
		scopeOpts,
		sortOrder,
		showDividers,
		isFiltered,
		displayActive,
		onSetUrgentOnly,
		onSetScope,
		onClearFilterMenu,
		onSetSortOrder,
		onToggleRowDividers
	}: {
		urgentOnly: boolean;
		scope: InboxScope;
		scopeOpts: {
			teams: { id: string; name: string }[];
			projects: { id: string; name: string }[];
		};
		sortOrder: TriageOrder;
		showDividers: boolean;
		isFiltered: boolean;
		displayActive: boolean;
		onSetUrgentOnly: (urgent: boolean) => void;
		onSetScope: (nextScope: InboxScope) => void;
		onClearFilterMenu: () => void;
		onSetSortOrder: (order: TriageOrder) => void;
		onToggleRowDividers: (checked: boolean) => void;
	} = $props();

	let filterMenuOpen = $state(false);
	let displayMenuOpen = $state(false);

	const menuSurface =
		'triage-menu-surface motion-reduce:animate-none! motion-reduce:transition-none! motion-reduce:data-[state=closed]:animate-none! motion-reduce:data-[state=open]:animate-none!';
	const itemTouch = 'min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11';
</script>

<DropdownMenu.Root bind:open={filterMenuOpen}>
	<Tooltip.Root disabled={filterMenuOpen}>
		<Tooltip.Trigger>
			{#snippet child({ props: tipProps })}
				<DropdownMenu.Trigger>
					{#snippet child({ props: menuProps })}
						<button
							{...mergeProps(tipProps, menuProps)}
							type="button"
							aria-label="Filter inbox"
							data-active={isFiltered ? '' : undefined}
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
				>Filter inbox</Tooltip.Content
			>
		</Tooltip.Portal>
	</Tooltip.Root>
	<DropdownMenu.Content
		preventScroll={false}
		align="end"
		class="{menuSurface} max-h-[min(60dvh,28rem)] w-60 max-w-[calc(100vw-2rem)] overflow-y-auto"
	>
		<DropdownMenu.Label>Urgency</DropdownMenu.Label>
		<DropdownMenu.RadioGroup
			value={urgentOnly ? 'urgent' : 'all'}
			onValueChange={(val) => onSetUrgentOnly(val === 'urgent')}
		>
			<DropdownMenu.RadioItem value="all" closeOnSelect={true} class={itemTouch}>
				All items
			</DropdownMenu.RadioItem>
			<DropdownMenu.RadioItem value="urgent" closeOnSelect={true} class={itemTouch}>
				Urgent only
			</DropdownMenu.RadioItem>
		</DropdownMenu.RadioGroup>
		<DropdownMenu.Separator />
		<DropdownMenu.Label>Project</DropdownMenu.Label>
		<DropdownMenu.RadioGroup
			value={scope.project ?? ''}
			onValueChange={(val) => onSetScope({ ...scope, project: val || undefined })}
		>
			<DropdownMenu.RadioItem value="" closeOnSelect={true} class={itemTouch}>
				All projects
			</DropdownMenu.RadioItem>
			{#each scopeOpts.projects as proj (proj.id)}
				<DropdownMenu.RadioItem value={proj.id} closeOnSelect={true} class={itemTouch}>
					{proj.name}
				</DropdownMenu.RadioItem>
			{/each}
		</DropdownMenu.RadioGroup>
		<DropdownMenu.Separator />
		<DropdownMenu.Label>Team</DropdownMenu.Label>
		<DropdownMenu.RadioGroup
			value={scope.team ?? ''}
			onValueChange={(val) => onSetScope({ ...scope, team: val || undefined })}
		>
			<DropdownMenu.RadioItem value="" closeOnSelect={true} class={itemTouch}>
				All teams
			</DropdownMenu.RadioItem>
			{#each scopeOpts.teams as team (team.id)}
				<DropdownMenu.RadioItem value={team.id} closeOnSelect={true} class={itemTouch}>
					{team.name}
				</DropdownMenu.RadioItem>
			{/each}
		</DropdownMenu.RadioGroup>
		{#if isFiltered}
			<DropdownMenu.Separator />
			<DropdownMenu.Group>
				<DropdownMenu.Item onSelect={onClearFilterMenu} class={itemTouch}>
					Reset filters
				</DropdownMenu.Item>
			</DropdownMenu.Group>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>

<DropdownMenu.Root bind:open={displayMenuOpen}>
	<Tooltip.Root disabled={displayMenuOpen}>
		<Tooltip.Trigger>
			{#snippet child({ props: tipProps })}
				<DropdownMenu.Trigger>
					{#snippet child({ props: menuProps })}
						<button
							{...mergeProps(tipProps, menuProps)}
							type="button"
							aria-label="Inbox display"
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
				>Inbox display</Tooltip.Content
			>
		</Tooltip.Portal>
	</Tooltip.Root>
	<DropdownMenu.Content
		preventScroll={false}
		align="end"
		class="{menuSurface} w-52 max-w-[calc(100vw-2rem)]"
	>
		<DropdownMenu.Label>Grouping</DropdownMenu.Label>
		<DropdownMenu.RadioGroup
			value={sortOrder}
			onValueChange={(val) => {
				if (val === 'priority' || val === 'date') onSetSortOrder(val);
			}}
		>
			<DropdownMenu.RadioItem value="priority" closeOnSelect={true} class={itemTouch}>
				Priority first
			</DropdownMenu.RadioItem>
			<DropdownMenu.RadioItem value="date" closeOnSelect={true} class={itemTouch}>
				By date
			</DropdownMenu.RadioItem>
		</DropdownMenu.RadioGroup>
		<DropdownMenu.Separator />
		<DropdownMenu.CheckboxItem
			checked={showDividers}
			onCheckedChange={(checked) => onToggleRowDividers(Boolean(checked))}
			class={itemTouch}
		>
			Row dividers
		</DropdownMenu.CheckboxItem>
	</DropdownMenu.Content>
</DropdownMenu.Root>

<style>
	.queue-icon-btn {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: var(--triage-control-height);
		min-height: var(--triage-control-height);
		border-radius: var(--exec-ctrl-radius);
		background: transparent;
		color: var(--exec-body);
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.queue-icon-btn:hover,
	.queue-icon-btn[aria-expanded='true'] {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.queue-icon-btn[data-active] {
		color: var(--tone-accent-ink);
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
	.queue-icon-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	@media (forced-colors: active) {
		.queue-icon-btn {
			border: 1px solid ButtonText;
		}
		.queue-icon-btn[data-active] {
			border: 2px solid Highlight;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.queue-icon-btn {
			transition: none;
		}
	}
</style>
