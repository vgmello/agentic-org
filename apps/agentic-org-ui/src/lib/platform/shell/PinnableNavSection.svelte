<!--
  @component
  Shared pinnable & reorderable sidebar navigation section (used by Workspace and Organization).
  Supports drag-and-drop and Alt+ArrowUp/ArrowDown reordering, pin/unpin focus restoration,
  WCAG 1.4.13 tooltips, and a compact rail More dropdown so unpinned items stay reachable in
  collapsed and 200%-text icon-rail modes.
-->
<script lang="ts" generics="TId extends string">
	import { tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { Tooltip, mergeProps } from 'bits-ui';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import Minus from '@lucide/svelte/icons/minus';
	import Plus from '@lucide/svelte/icons/plus';
	import { tooltipContentClass } from '$lib/components/classes';
	import PinnableNavRailMoreMenu from './PinnableNavRailMoreMenu.svelte';
	import type { PinnableNavItem, PinnableNavSectionConfig } from './sidebar-nav-items';
	import { tooltipLinkProps } from './tooltip-link-props';

	let {
		config,
		pinnedIds,
		collapsed = false,
		isItemActive,
		onMoveItem,
		onTogglePin,
		onNavigate
	}: {
		config: PinnableNavSectionConfig<TId>;
		pinnedIds: readonly TId[];
		collapsed?: boolean;
		isItemActive: (item: PinnableNavItem<TId>) => boolean;
		onMoveItem: (id: TId, beforeId: TId | null, zone: 'pinned' | 'more') => void;
		onTogglePin: (id: TId) => void;
		onNavigate?: (event: MouseEvent, href: string) => void;
	} = $props();

	let draggingId = $state<TId | null>(null);
	let dropHint = $state<{ id: TId; edge: 'before' | 'after' } | null>(null);
	let moreDropActive = $state(false);
	let moreUserOpen = $state<boolean | null>(null);
	let liveAnnouncement = $state('');
	const uid = $props.id();
	const moreListId = `${uid}-more`;
	let navListEl = $state<HTMLUListElement | null>(null);

	const pinnedItems = $derived(pinnedIds.map((id) => config.meta[id]));
	const moreItems = $derived(
		config.allIds.filter((id) => !pinnedIds.includes(id)).map((id) => config.meta[id])
	);
	const anyMoreActive = $derived(moreItems.some((item) => isItemActive(item)));
	const moreOpen = $derived(moreUserOpen ?? anyMoreActive);

	function zoneAttrs(zone: 'pinned' | 'more', key?: TId): Record<string, string> {
		if (config.zoneKind === 'ws') {
			return key ? { 'data-ws-zone': zone, 'data-ws-key': key } : { 'data-ws-zone': zone };
		}
		return key ? { 'data-org-zone': zone, 'data-org-key': key } : { 'data-org-zone': zone };
	}

	function announce(text: string) {
		liveAnnouncement = '';
		void tick().then(() => {
			liveAnnouncement = text;
		});
	}

	function focusItem(id: TId) {
		void tick().then(() => {
			navListEl
				?.querySelector<HTMLAnchorElement>(`[data-nav-link="${id}"]`)
				?.focus({ preventScroll: true });
		});
	}

	function handleDragStart(event: DragEvent, id: TId) {
		draggingId = id;
		event.dataTransfer?.setData('text/plain', id);
		if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
	}

	function handleItemDragOver(event: DragEvent, targetId: TId, zone: 'pinned' | 'more') {
		if (!draggingId || draggingId === targetId) return;
		event.preventDefault();
		if (zone === 'more') {
			dropHint = null;
			moreDropActive = true;
			return;
		}
		moreDropActive = false;
		const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
		const edge = event.clientY > rect.top + rect.height / 2 ? 'after' : 'before';
		dropHint = { id: targetId, edge };
	}

	function handleItemDrop(event: DragEvent, targetId: TId, zone: 'pinned' | 'more') {
		if (!draggingId) return;
		event.preventDefault();
		const movedId = draggingId;
		const hint = dropHint;
		draggingId = null;
		dropHint = null;
		moreDropActive = false;
		if (zone === 'more') {
			if (!pinnedIds.includes(movedId) || pinnedIds.length <= 1) return;
			onMoveItem(movedId, null, 'more');
			announce(`Moved ${config.meta[movedId].label} to More.`);
			return;
		}
		let beforeId: TId | null = targetId;
		if (hint?.id === targetId && hint.edge === 'after') {
			const idx = pinnedIds.indexOf(targetId);
			beforeId = idx >= 0 && idx + 1 < pinnedIds.length ? pinnedIds[idx + 1] : null;
		}
		if (beforeId === movedId) return;
		onMoveItem(movedId, beforeId, 'pinned');
		const pos = pinnedIds.indexOf(movedId) + 1;
		announce(`Moved ${config.meta[movedId].label} to position ${pos} in ${config.sectionLabel}.`);
		focusItem(movedId);
	}

	function handleMoreZoneDrop(event: DragEvent) {
		if (!draggingId) return;
		event.preventDefault();
		const movedId = draggingId;
		draggingId = null;
		dropHint = null;
		moreDropActive = false;
		if (!pinnedIds.includes(movedId) || pinnedIds.length <= 1) return;
		onMoveItem(movedId, null, 'more');
		announce(`Moved ${config.meta[movedId].label} to More.`);
	}

	function clearDrag() {
		draggingId = null;
		dropHint = null;
		moreDropActive = false;
	}

	function handleItemKeydown(event: KeyboardEvent, id: TId, zone: 'pinned' | 'more') {
		if (!event.altKey || zone !== 'pinned') return;
		if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
		const idx = pinnedIds.indexOf(id);
		const nextIdx = event.key === 'ArrowUp' ? idx - 1 : idx + 1;
		if (idx < 0 || nextIdx < 0 || nextIdx >= pinnedIds.length) return;
		event.preventDefault();
		const beforeId =
			event.key === 'ArrowUp' ? pinnedIds[nextIdx] : (pinnedIds[nextIdx + 1] ?? null);
		onMoveItem(id, beforeId, 'pinned');
		announce(`Moved ${config.meta[id].label} to position ${nextIdx + 1} of ${pinnedIds.length}.`);
		focusItem(id);
	}

	function handleTogglePin(item: PinnableNavItem<TId>, pinned: boolean) {
		if (pinned && pinnedIds.length <= 1) return;
		onTogglePin(item.id);
		announce(
			pinned ? `Moved ${item.label} to More.` : `Pinned ${item.label} to ${config.sectionLabel}.`
		);
		if (pinned) {
			void tick().then(() => {
				navListEl
					?.querySelector<HTMLElement>(`[data-testid="${config.moreToggleTestId}"]`)
					?.focus();
			});
		} else {
			focusItem(item.id);
		}
	}
</script>

<div class={collapsed ? 'w-full' : undefined} data-testid={config.sectionTestId}>
	{#if !collapsed}
		<div class="exec-nav-label px-2.5 pb-1.5">{config.sectionLabel}</div>
	{/if}
	<p class="sr-only" aria-live="polite" aria-atomic="true">{liveAnnouncement}</p>
	<ul
		bind:this={navListEl}
		class={['flex flex-col', collapsed ? 'items-center gap-1' : 'gap-0.5']}
		{...zoneAttrs('pinned')}
	>
		{#each pinnedItems as item (item.id)}
			{@const NavIcon = item.Icon}
			{@const active = isItemActive(item)}
			<li
				draggable={!collapsed}
				{...zoneAttrs('pinned', item.id)}
				ondragstart={(e) => handleDragStart(e, item.id)}
				ondragover={(e) => handleItemDragOver(e, item.id, 'pinned')}
				ondrop={(e) => handleItemDrop(e, item.id, 'pinned')}
				ondragend={clearDrag}
				class={[
					'pin-nav-row group relative flex items-center gap-0.5',
					collapsed && 'justify-center',
					draggingId === item.id && 'opacity-50',
					dropHint?.id === item.id && dropHint.edge === 'before' && 'is-drop-before',
					dropHint?.id === item.id && dropHint.edge === 'after' && 'is-drop-after'
				]}
			>
				<Tooltip.Root disabled={!collapsed}>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<a
								{...mergeProps(tooltipLinkProps(props), {
									onclick: (e: MouseEvent) => onNavigate?.(e, item.href),
									onkeydown: (e: KeyboardEvent) => handleItemKeydown(e, item.id, 'pinned')
								})}
								data-nav-link={item.id}
								href={resolve(item.href)}
								aria-current={active ? 'page' : undefined}
								title={collapsed ? undefined : `${item.label} (drag or Alt+↑/↓ to reorder)`}
								class={[
									'exec-nav-item',
									collapsed ? 'is-rail' : 'min-w-0 flex-1',
									active && 'is-active'
								]}
							>
								<NavIcon class="size-[18px] shrink-0" aria-hidden="true" />
								<span class={collapsed ? 'sr-only' : 'truncate'}>{item.label}</span>
							</a>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content role="tooltip" side="right" sideOffset={6} class={tooltipContentClass}
							>{item.label}</Tooltip.Content
						>
					</Tooltip.Portal>
				</Tooltip.Root>
				{#if !collapsed && pinnedItems.length > 1}
					<Tooltip.Root>
						<Tooltip.Trigger>
							{#snippet child({ props })}
								<button
									{...mergeProps(props, {
										onclick: () => handleTogglePin(item, true)
									})}
									type="button"
									aria-label="Move {item.label} to More"
									class="pin-action-btn flex size-7 shrink-0 items-center justify-center rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 [@media(pointer:coarse)]:size-11 [@media(pointer:coarse)]:opacity-100 [@media(width<=720px)]:size-11"
								>
									<Minus class="size-3.5" aria-hidden="true" />
								</button>
							{/snippet}
						</Tooltip.Trigger>
						<Tooltip.Portal>
							<Tooltip.Content
								role="tooltip"
								side="right"
								sideOffset={6}
								class={tooltipContentClass}>Move to More</Tooltip.Content
							>
						</Tooltip.Portal>
					</Tooltip.Root>
				{/if}
			</li>
		{/each}

		{#if moreItems.length > 0 && collapsed}
			<PinnableNavRailMoreMenu
				items={moreItems}
				ariaLabel={config.moreListAriaLabel}
				testId="{config.moreToggleTestId}-rail"
				active={anyMoreActive}
				{isItemActive}
			/>
		{:else if moreItems.length > 0}
			<li
				class={['mt-0.5', moreDropActive && 'is-more-drop']}
				ondragover={(e) => {
					if (!draggingId) return;
					e.preventDefault();
					dropHint = null;
					moreDropActive = true;
				}}
				ondragleave={() => (moreDropActive = false)}
				ondrop={handleMoreZoneDrop}
			>
				<button
					type="button"
					onclick={() => (moreUserOpen = !moreOpen)}
					aria-expanded={moreOpen}
					aria-controls={moreListId}
					data-testid={config.moreToggleTestId}
					class="exec-nav-item w-full"
				>
					<Ellipsis class="size-[18px] shrink-0" aria-hidden="true" />
					<span class="min-w-0 flex-1 truncate text-left">More</span>
					<ChevronDown
						class={[
							'size-3.5 shrink-0 transition-transform duration-150 motion-reduce:transition-none',
							moreOpen && 'rotate-180'
						]}
						aria-hidden="true"
					/>
				</button>
				{#if moreOpen}
					<ul
						id={moreListId}
						data-testid={config.moreListTestId}
						class="mt-0.5 flex flex-col gap-0.5 pl-3"
						aria-label={config.moreListAriaLabel}
						{...zoneAttrs('more')}
					>
						{#each moreItems as item (item.id)}
							{@const MoreIcon = item.Icon}
							{@const active = isItemActive(item)}
							<li
								draggable="true"
								{...zoneAttrs('more', item.id)}
								ondragstart={(e) => handleDragStart(e, item.id)}
								ondragover={(e) => handleItemDragOver(e, item.id, 'more')}
								ondrop={(e) => handleItemDrop(e, item.id, 'more')}
								ondragend={clearDrag}
								class="group flex items-center gap-0.5"
							>
								<a
									data-nav-link={item.id}
									href={resolve(item.href)}
									onclick={(e) => onNavigate?.(e, item.href)}
									aria-current={active ? 'page' : undefined}
									class={['exec-nav-item min-w-0 flex-1', active && 'is-active']}
								>
									<MoreIcon class="size-[18px] shrink-0" aria-hidden="true" />
									<span class="truncate">{item.label}</span>
								</a>
								<Tooltip.Root>
									<Tooltip.Trigger>
										{#snippet child({ props })}
											<button
												{...mergeProps(props, {
													onclick: () => handleTogglePin(item, false)
												})}
												type="button"
												aria-label="Pin {item.label} to {config.sectionLabel}"
												class="pin-action-btn flex size-7 shrink-0 items-center justify-center rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 [@media(pointer:coarse)]:size-11 [@media(pointer:coarse)]:opacity-100 [@media(width<=720px)]:size-11"
											>
												<Plus class="size-3.5" aria-hidden="true" />
											</button>
										{/snippet}
									</Tooltip.Trigger>
									<Tooltip.Portal>
										<Tooltip.Content
											role="tooltip"
											side="right"
											sideOffset={6}
											class={tooltipContentClass}>Pin to {config.sectionLabel}</Tooltip.Content
										>
									</Tooltip.Portal>
								</Tooltip.Root>
							</li>
						{/each}
					</ul>
				{/if}
			</li>
		{/if}
	</ul>
</div>

<style>
	.exec-nav-label {
		font-size: var(--exec-nav-label-size);
		line-height: 1.125rem;
		font-weight: 650;
		letter-spacing: var(--exec-nav-label-tracking);
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.exec-nav-item {
		position: relative;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 10px;
		border-radius: 8px;
		font-size: 0.875rem;
		line-height: 1.35rem;
		font-weight: 500;
		color: var(--exec-muted);
		white-space: nowrap;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.exec-nav-item.is-rail {
		width: 44px;
		height: 44px;
		justify-content: center;
		padding: 0;
		gap: 0;
	}
	.exec-nav-item:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.exec-nav-item.is-active {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		font-weight: 600;
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.exec-nav-item.is-active :global(svg) {
		color: var(--exec-accent-ink);
	}
	.pin-action-btn {
		color: var(--exec-muted);
	}
	.pin-action-btn:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.pin-nav-row.is-drop-before::before,
	.pin-nav-row.is-drop-after::after {
		content: '';
		position: absolute;
		inset-inline: 8px;
		height: 2px;
		border-radius: 999px;
		background: var(--exec-accent-ink);
		pointer-events: none;
	}
	.pin-nav-row.is-drop-before::before {
		top: -1px;
	}
	.pin-nav-row.is-drop-after::after {
		bottom: -1px;
	}
	.is-more-drop {
		border-radius: 8px;
		box-shadow: inset 0 0 0 1.5px var(--exec-accent-ink);
	}
	@media (prefers-reduced-motion: reduce) {
		.exec-nav-item,
		.pin-action-btn {
			transition: none;
		}
	}
</style>
