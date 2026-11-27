<script lang="ts">
	import { tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { Tooltip, mergeProps } from 'bits-ui';
	import Activity from '@lucide/svelte/icons/activity';
	import BarChart3 from '@lucide/svelte/icons/bar-chart-3';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ClipboardList from '@lucide/svelte/icons/clipboard-list';
	import Ellipsis from '@lucide/svelte/icons/ellipsis';
	import Folder from '@lucide/svelte/icons/folder';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Minus from '@lucide/svelte/icons/minus';
	import Plus from '@lucide/svelte/icons/plus';
	import {
		WORKSPACE_NAV_IDS,
		uiPrefs,
		type WorkspaceNavId
	} from '$lib/platform/workspace/ui-preferences.svelte';

	interface WorkspaceNavMeta {
		id: WorkspaceNavId;
		href: '/work' | '/chat' | '/activity' | '/projects' | '/insights';
		label: string;
		Icon: typeof ClipboardList;
	}

	const WORKSPACE_META: Record<WorkspaceNavId, WorkspaceNavMeta> = {
		work: { id: 'work', href: '/work', label: 'Work', Icon: ClipboardList },
		chat: { id: 'chat', href: '/chat', label: 'Chat', Icon: MessageSquare },
		activity: { id: 'activity', href: '/activity', label: 'Activity', Icon: Activity },
		projects: { id: 'projects', href: '/projects', label: 'Projects', Icon: Folder },
		insights: { id: 'insights', href: '/insights', label: 'Insights', Icon: BarChart3 }
	};

	let {
		collapsed = false,
		isActive,
		onNavigate
	}: {
		collapsed?: boolean;
		isActive: (href: string) => boolean;
		onNavigate?: (event: MouseEvent, href: string) => void;
	} = $props();

	let draggingId = $state<WorkspaceNavId | null>(null);
	let dropHint = $state<{ id: WorkspaceNavId; edge: 'before' | 'after' } | null>(null);
	let moreDropActive = $state(false);
	let moreUserOpen = $state<boolean | null>(null);
	let liveAnnouncement = $state('');
	let navListEl = $state<HTMLUListElement | null>(null);

	const pinnedItems = $derived(uiPrefs.workspaceNav.map((id) => WORKSPACE_META[id]));
	const moreItems = $derived(
		WORKSPACE_NAV_IDS.filter((id) => !uiPrefs.workspaceNav.includes(id)).map(
			(id) => WORKSPACE_META[id]
		)
	);
	const anyMoreActive = $derived(moreItems.some((item) => isActive(item.href)));
	const moreOpen = $derived(moreUserOpen ?? anyMoreActive);

	function announce(text: string) {
		liveAnnouncement = '';
		void tick().then(() => {
			liveAnnouncement = text;
		});
	}

	function focusItem(id: WorkspaceNavId) {
		void tick().then(() => {
			navListEl
				?.querySelector<HTMLAnchorElement>(`[data-ws-link="${id}"]`)
				?.focus({ preventScroll: true });
		});
	}

	function handleDragStart(event: DragEvent, id: WorkspaceNavId) {
		draggingId = id;
		event.dataTransfer?.setData('text/plain', id);
		if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
	}

	function handleItemDragOver(event: DragEvent, targetId: WorkspaceNavId, zone: 'pinned' | 'more') {
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

	function handleItemDrop(event: DragEvent, targetId: WorkspaceNavId, zone: 'pinned' | 'more') {
		if (!draggingId) return;
		event.preventDefault();
		const movedId = draggingId;
		const hint = dropHint;
		draggingId = null;
		dropHint = null;
		moreDropActive = false;
		if (zone === 'more') {
			const wasPinned = uiPrefs.workspaceNav.includes(movedId);
			if (!wasPinned || uiPrefs.workspaceNav.length <= 1) return;
			uiPrefs.moveWorkspaceItem(movedId, null, 'more');
			announce(`Moved ${WORKSPACE_META[movedId].label} to More.`);
			return;
		}
		let beforeId: WorkspaceNavId | null = targetId;
		if (hint?.id === targetId && hint.edge === 'after') {
			const idx = uiPrefs.workspaceNav.indexOf(targetId);
			beforeId =
				idx >= 0 && idx + 1 < uiPrefs.workspaceNav.length ? uiPrefs.workspaceNav[idx + 1] : null;
		}
		if (beforeId === movedId) return;
		uiPrefs.moveWorkspaceItem(movedId, beforeId, 'pinned');
		const pos = uiPrefs.workspaceNav.indexOf(movedId) + 1;
		announce(`Moved ${WORKSPACE_META[movedId].label} to position ${pos} in Workspace.`);
		focusItem(movedId);
	}

	function handleMoreZoneDrop(event: DragEvent) {
		if (!draggingId) return;
		event.preventDefault();
		const movedId = draggingId;
		draggingId = null;
		dropHint = null;
		moreDropActive = false;
		const wasPinned = uiPrefs.workspaceNav.includes(movedId);
		if (!wasPinned || uiPrefs.workspaceNav.length <= 1) return;
		uiPrefs.moveWorkspaceItem(movedId, null, 'more');
		announce(`Moved ${WORKSPACE_META[movedId].label} to More.`);
	}

	function clearDrag() {
		draggingId = null;
		dropHint = null;
		moreDropActive = false;
	}

	function handleItemKeydown(event: KeyboardEvent, id: WorkspaceNavId, zone: 'pinned' | 'more') {
		if (!event.altKey || zone !== 'pinned') return;
		if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
		const idx = uiPrefs.workspaceNav.indexOf(id);
		const nextIdx = event.key === 'ArrowUp' ? idx - 1 : idx + 1;
		if (idx < 0 || nextIdx < 0 || nextIdx >= uiPrefs.workspaceNav.length) return;
		event.preventDefault();
		const beforeId =
			event.key === 'ArrowUp'
				? uiPrefs.workspaceNav[nextIdx]
				: (uiPrefs.workspaceNav[nextIdx + 1] ?? null);
		uiPrefs.moveWorkspaceItem(id, beforeId, 'pinned');
		announce(
			`Moved ${WORKSPACE_META[id].label} to position ${nextIdx + 1} of ${uiPrefs.workspaceNav.length}.`
		);
		focusItem(id);
	}

	function handleTogglePin(item: WorkspaceNavMeta, pinned: boolean) {
		if (pinned && uiPrefs.workspaceNav.length <= 1) return;
		uiPrefs.toggleWorkspacePin(item.id);
		announce(pinned ? `Moved ${item.label} to More.` : `Pinned ${item.label} to Workspace.`);
	}
</script>

<div class={collapsed ? 'w-full' : undefined} data-testid="workspace-nav-section">
	{#if !collapsed}
		<div class="exec-nav-label px-2.5 pb-1.5">Workspace</div>
	{/if}
	<p class="sr-only" aria-live="polite" aria-atomic="true">{liveAnnouncement}</p>
	<ul
		bind:this={navListEl}
		class={['flex flex-col', collapsed ? 'items-center gap-1' : 'gap-0.5']}
		data-ws-zone="pinned"
	>
		{#each pinnedItems as item (item.id)}
			{@const NavIcon = item.Icon}
			{@const active = isActive(item.href)}
			<li
				draggable={!collapsed}
				data-ws-key={item.id}
				data-ws-zone="pinned"
				ondragstart={(e) => handleDragStart(e, item.id)}
				ondragover={(e) => handleItemDragOver(e, item.id, 'pinned')}
				ondrop={(e) => handleItemDrop(e, item.id, 'pinned')}
				ondragend={clearDrag}
				class={[
					'ws-nav-row group relative flex items-center gap-0.5',
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
								{...mergeProps(props, {
									onclick: (e: MouseEvent) => onNavigate?.(e, item.href),
									onkeydown: (e: KeyboardEvent) => handleItemKeydown(e, item.id, 'pinned')
								})}
								data-ws-link={item.id}
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
						<Tooltip.Content
							side="right"
							sideOffset={6}
							class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
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
									class="ws-pin-btn flex size-7 shrink-0 items-center justify-center rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 [@media(pointer:coarse)]:size-11 [@media(pointer:coarse)]:opacity-100"
								>
									<Minus class="size-3.5" aria-hidden="true" />
								</button>
							{/snippet}
						</Tooltip.Trigger>
						<Tooltip.Portal>
							<Tooltip.Content
								side="right"
								sideOffset={6}
								class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
								>Move to More</Tooltip.Content
							>
						</Tooltip.Portal>
					</Tooltip.Root>
				{/if}
			</li>
		{/each}

		{#if moreItems.length > 0 && !collapsed}
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
					aria-controls="exec-nav-more-list"
					data-testid="workspace-more-toggle"
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
						id="exec-nav-more-list"
						class="mt-0.5 flex flex-col gap-0.5 pl-3"
						aria-label="More workspace items"
						data-ws-zone="more"
					>
						{#each moreItems as item (item.id)}
							{@const MoreIcon = item.Icon}
							{@const active = isActive(item.href)}
							<li
								draggable="true"
								data-ws-key={item.id}
								data-ws-zone="more"
								ondragstart={(e) => handleDragStart(e, item.id)}
								ondragover={(e) => handleItemDragOver(e, item.id, 'more')}
								ondrop={(e) => handleItemDrop(e, item.id, 'more')}
								ondragend={clearDrag}
								class="group flex items-center gap-0.5"
							>
								<a
									data-ws-link={item.id}
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
												aria-label="Pin {item.label} to Workspace"
												class="ws-pin-btn flex size-7 shrink-0 items-center justify-center rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 [@media(pointer:coarse)]:size-11 [@media(pointer:coarse)]:opacity-100"
											>
												<Plus class="size-3.5" aria-hidden="true" />
											</button>
										{/snippet}
									</Tooltip.Trigger>
									<Tooltip.Portal>
										<Tooltip.Content
											side="right"
											sideOffset={6}
											class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
											>Pin to Workspace</Tooltip.Content
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
	.ws-pin-btn {
		color: var(--exec-muted);
	}
	.ws-pin-btn:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.ws-nav-row.is-drop-before::before,
	.ws-nav-row.is-drop-after::after {
		content: '';
		position: absolute;
		inset-inline: 8px;
		height: 2px;
		border-radius: 999px;
		background: var(--exec-accent-ink);
		pointer-events: none;
	}
	.ws-nav-row.is-drop-before::before {
		top: -1px;
	}
	.ws-nav-row.is-drop-after::after {
		bottom: -1px;
	}
	.is-more-drop {
		border-radius: 8px;
		box-shadow: inset 0 0 0 1.5px var(--exec-accent-ink);
	}
	@media (prefers-reduced-motion: reduce) {
		.exec-nav-item,
		.ws-pin-btn {
			transition: none;
		}
	}
</style>
