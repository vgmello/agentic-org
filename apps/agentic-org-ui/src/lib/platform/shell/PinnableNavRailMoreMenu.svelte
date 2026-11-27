<!--
  @component
  Collapsed/icon-rail More dropdown for a pinnable sidebar section.
  Keeps unpinned destinations reachable in auto-hide and 200%-text reflow rails without mutating
  the auto-hide dock's expanded-menu lock, suppresses overlapping tooltips while open, and moves
  focus to the destination heading on item selection.
-->
<script lang="ts" generics="TId extends string">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import { tooltipContentClass } from '$lib/components/classes';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { focusDestinationHeading } from '$lib/platform/workspace/focus-destination';
	import type { PinnableNavItem } from './sidebar-nav-items';

	let {
		items,
		ariaLabel,
		testId,
		active = false
	}: {
		items: readonly PinnableNavItem<TId>[];
		ariaLabel: string;
		testId: string;
		active?: boolean;
	} = $props();

	let open = $state(false);
	let tipOpen = $state(false);
	let navigated = false;

	async function selectItem(item: PinnableNavItem<TId>) {
		navigated = true;
		await goto(resolve(item.href));
		await focusDestinationHeading();
	}

	function handleOpenChange(next: boolean) {
		if (next) {
			tipOpen = false;
			navigated = false;
		}
	}

	function handleCloseAutoFocus(event: Event) {
		if (navigated) {
			event.preventDefault();
			navigated = false;
		}
	}
</script>

<li
	class="flex justify-center"
	{@attach () => () => {
		open = false;
		tipOpen = false;
	}}
>
	<DropdownMenu.Root bind:open onOpenChange={handleOpenChange}>
		<Tooltip.Root bind:open={tipOpen} disabled={open}>
			<Tooltip.Trigger>
				{#snippet child({ props: tipProps })}
					<DropdownMenu.Trigger {...tipProps}>
						{#snippet child({ props })}
							<button
								{...props}
								type="button"
								aria-label={ariaLabel}
								data-testid={testId}
								class={['rail-more-trigger', active && 'is-active']}
							>
								<Ellipsis class="size-[18px] shrink-0" aria-hidden="true" />
							</button>
						{/snippet}
					</DropdownMenu.Trigger>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content role="tooltip" side="right" sideOffset={6} class={tooltipContentClass}
					>{ariaLabel}</Tooltip.Content
				>
			</Tooltip.Portal>
		</Tooltip.Root>
		<DropdownMenu.Content
			side="right"
			align="start"
			onCloseAutoFocus={handleCloseAutoFocus}
			class="w-52 bg-surface ring-line"
		>
			{#each items as item (item.id)}
				{@const MoreIcon = item.Icon}
				<DropdownMenu.Item
					class="flex min-h-9 items-center gap-2 max-[639px]:min-h-11"
					onSelect={() => void selectItem(item)}
				>
					<MoreIcon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
					<span>{item.label}</span>
				</DropdownMenu.Item>
			{/each}
		</DropdownMenu.Content>
	</DropdownMenu.Root>
</li>

<style>
	.rail-more-trigger {
		position: relative;
		display: flex;
		align-items: center;
		justify-content: center;
		width: 44px;
		height: 44px;
		padding: 0;
		border-radius: 8px;
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.rail-more-trigger:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.rail-more-trigger.is-active {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.rail-more-trigger.is-active :global(svg) {
		color: var(--exec-accent-ink);
	}
	@media (prefers-reduced-motion: reduce) {
		.rail-more-trigger {
			transition: none;
		}
	}
</style>
