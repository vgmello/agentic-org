<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import House from '@lucide/svelte/icons/house';
	import Inbox from '@lucide/svelte/icons/inbox';
	import PanelLeft from '@lucide/svelte/icons/panel-left';
	import Search from '@lucide/svelte/icons/search';
	import { tooltipContentClass } from '$lib/components/classes';
	import type { Connection, ProjectSummary, ProjectTeam, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/platform/workspace/inbox-types';
	import ExecutiveSidebarFavorites from './ExecutiveSidebarFavorites.svelte';
	import ExecutiveSidebarFooter from './ExecutiveSidebarFooter.svelte';
	import OrganizationSwitcher from './OrganizationSwitcher.svelte';
	import PinnableNavSection from './PinnableNavSection.svelte';
	import {
		ORG_SECTION_CONFIG,
		WORKSPACE_SECTION_CONFIG,
		isOrgNavItemActive
	} from './sidebar-nav-items';
	import { tooltipLinkProps } from './tooltip-link-props';
	import { useNotifications } from '$lib/platform/workspace/notifications.svelte';
	import { singleKeyShortcutsOn } from '$lib/platform/workspace/shortcuts';
	import { uiPrefs } from '$lib/platform/workspace/ui-preferences.svelte';

	let {
		connection,
		needsYouCount = 0,
		projects = [],
		workItems = [],
		inboxItems = [],
		projectTeams,
		projectDueById,
		collapsed = false,
		pinned = true,
		tessOpen = false,
		offline = false,
		onNavigate,
		onOpenSearch,
		onToggleTess,
		onToggleCollapse,
		onMenuOpenChange,
		onPointerEnter,
		onPointerLeave,
		onFocusIn,
		onFocusOut
	}: {
		connection: Connection;
		needsYouCount?: number;
		projects?: ProjectSummary[];
		workItems?: readonly WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		projectTeams?: readonly ProjectTeam[];
		projectDueById?: Readonly<Record<string, string>>;
		collapsed?: boolean;
		pinned?: boolean;
		tessOpen?: boolean;
		offline?: boolean;
		onNavigate?: (event: MouseEvent, href: string) => void;
		onOpenSearch?: () => void;
		onToggleTess?: () => void;
		onToggleCollapse?: () => void;
		onMenuOpenChange?: (open: boolean) => void;
		onPointerEnter?: () => void;
		onPointerLeave?: () => void;
		onFocusIn?: (event: FocusEvent) => void;
		onFocusOut?: (event: FocusEvent) => void;
	} = $props();

	const notifications = useNotifications();
	const activeTessJobs = $derived(notifications?.activeTessJobs.length ?? 0);
	const totalInboxAttention = $derived(
		needsYouCount + (notifications?.unreadCount ?? 0) + (notifications?.runningJobs.length ?? 0)
	);

	let orgMenuOpen = $state(false);
	let accountMenuOpen = $state(false);
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());

	function notifyMenuOpen() {
		onMenuOpenChange?.(orgMenuOpen || accountMenuOpen);
	}

	const pathname = $derived(page.url.pathname);
	const isActive = (href: string) => pathname === href || pathname.startsWith(`${href}/`);
</script>

<Tooltip.Provider delayDuration={150}>
	<aside
		aria-label="Sidebar"
		onpointerenter={onPointerEnter}
		onpointerleave={onPointerLeave}
		onfocusin={onFocusIn}
		onfocusout={onFocusOut}
		class="exec-sidebar flex h-full w-full min-w-0 flex-col overflow-clip"
		data-collapsed={collapsed ? 'true' : undefined}
	>
		<div
			class={[
				'flex min-h-14 shrink-0 items-center pt-2',
				collapsed ? 'justify-center px-2' : 'justify-between gap-2 px-3'
			]}
		>
			<Tooltip.Root disabled={!collapsed}>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<a
							{...tooltipLinkProps(props)}
							href={resolve('/home')}
							onclick={(event) => onNavigate?.(event, '/home')}
							aria-label="{connection.organization.name} home"
							class={[
								'exec-logo flex min-w-0 items-center rounded-lg py-1.5',
								collapsed ? 'size-11 justify-center px-0' : 'flex-1 gap-2.5 px-2'
							]}
						>
							<span
								class="exec-brand-mark flex size-7 shrink-0 items-center justify-center rounded-lg text-xs font-bold text-on-emphasis"
								aria-hidden="true"
							>
								A
							</span>
							<span class={collapsed ? 'sr-only' : 'exec-logo-name truncate'}>
								{connection.organization.name}
							</span>
						</a>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content role="tooltip" side="right" sideOffset={6} class={tooltipContentClass}
						>{connection.organization.name}</Tooltip.Content
					>
				</Tooltip.Portal>
			</Tooltip.Root>
			{#if !collapsed}
				<div class="flex shrink-0 items-center gap-0.5">
					<OrganizationSwitcher
						{connection}
						onOpenChange={(next) => {
							orgMenuOpen = next;
							notifyMenuOpen();
						}}
					/>
					{#if onToggleCollapse}
						<Tooltip.Root>
							<Tooltip.Trigger>
								{#snippet child({ props })}
									<button
										{...props}
										type="button"
										onclick={onToggleCollapse}
										aria-label={pinned ? 'Auto-hide sidebar' : 'Pin sidebar open'}
										aria-pressed={pinned}
										class={[
											'exec-icon-btn flex size-9 shrink-0 items-center justify-center rounded-md [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11',
											pinned && 'is-pinned'
										]}
									>
										<PanelLeft class="size-4" aria-hidden="true" />
									</button>
								{/snippet}
							</Tooltip.Trigger>
							<Tooltip.Portal>
								<Tooltip.Content
									role="tooltip"
									side="bottom"
									sideOffset={6}
									class={tooltipContentClass}
									>{pinned ? 'Auto-hide sidebar' : 'Pin sidebar open'}</Tooltip.Content
								>
							</Tooltip.Portal>
						</Tooltip.Root>
					{/if}
				</div>
			{/if}
		</div>

		{#if onOpenSearch}
			<div class={['shrink-0 pt-2 pb-1', collapsed ? 'flex justify-center px-2' : 'px-3']}>
				<Tooltip.Root disabled={!collapsed && !offline}>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<button
								{...props}
								type="button"
								aria-disabled={offline ? 'true' : undefined}
								onclick={() => {
									if (!offline) onOpenSearch();
								}}
								aria-label={offline ? 'Search unavailable while offline' : 'Search (⌘K)'}
								data-testid="sidebar-search-trigger"
								class={[
									'exec-sidebar-search flex items-center rounded-lg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid',
									collapsed
										? 'size-10 justify-center p-0 [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11'
										: 'w-full gap-2 px-2.5 py-1.5 text-left',
									offline && 'is-offline'
								]}
							>
								<Search class="size-4 shrink-0" aria-hidden="true" />
								{#if !collapsed}
									<span class="min-w-0 flex-1 truncate text-xs/5">Search…</span>
									<kbd class="exec-sidebar-kbd">⌘K</kbd>
								{/if}
							</button>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content role="tooltip" side="right" sideOffset={6} class={tooltipContentClass}
							>{offline ? 'Search unavailable while offline' : 'Search (⌘K)'}</Tooltip.Content
						>
					</Tooltip.Portal>
				</Tooltip.Root>
			</div>
		{/if}

		<nav
			data-exec-sidebar-nav=""
			aria-label="Main"
			aria-keyshortcuts={shortcutsEnabled ? 'S' : undefined}
			class={[
				'flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto py-3',
				collapsed ? 'items-center gap-3 px-2' : 'gap-3.5 px-3'
			]}
		>
			<div class={collapsed ? 'w-full' : undefined}>
				{#if !collapsed}
					<div class="exec-nav-label px-2.5 pb-1.5">Focus</div>
				{/if}
				<ul class={['flex flex-col', collapsed ? 'items-center gap-1' : 'gap-0.5']}>
					<li>
						<Tooltip.Root disabled={!collapsed}>
							<Tooltip.Trigger>
								{#snippet child({ props })}
									<a
										{...tooltipLinkProps(props)}
										href={resolve('/home')}
										onclick={(e) => onNavigate?.(e, '/home')}
										aria-current={isActive('/home') ? 'page' : undefined}
										class={[
											'exec-nav-item',
											collapsed && 'is-rail',
											isActive('/home') && 'is-active'
										]}
									>
										<House class="size-[18px] shrink-0" aria-hidden="true" />
										<span class={collapsed ? 'sr-only' : 'truncate'}>Home</span>
									</a>
								{/snippet}
							</Tooltip.Trigger>
							<Tooltip.Portal>
								<Tooltip.Content
									role="tooltip"
									side="right"
									sideOffset={6}
									class={tooltipContentClass}>Home</Tooltip.Content
								>
							</Tooltip.Portal>
						</Tooltip.Root>
					</li>
					<li>
						<Tooltip.Root disabled={!collapsed}>
							<Tooltip.Trigger>
								{#snippet child({ props })}
									<a
										{...tooltipLinkProps(props)}
										href={resolve('/inbox')}
										onclick={(e) => onNavigate?.(e, '/inbox')}
										aria-current={isActive('/inbox') ? 'page' : undefined}
										class={[
											'exec-nav-item',
											collapsed && 'is-rail',
											isActive('/inbox') && 'is-active'
										]}
									>
										<Inbox class="size-[18px] shrink-0" aria-hidden="true" />
										<span class={collapsed ? 'sr-only' : 'truncate'}>Inbox</span>
										{#if totalInboxAttention > 0}
											<span class={['exec-nav-dot', collapsed && 'is-rail']} aria-hidden="true"
											></span>
											<span class="sr-only">, {totalInboxAttention} needing attention</span>
										{/if}
									</a>
								{/snippet}
							</Tooltip.Trigger>
							<Tooltip.Portal>
								<Tooltip.Content
									role="tooltip"
									side="right"
									sideOffset={6}
									class={tooltipContentClass}
									>{totalInboxAttention > 0
										? `Inbox (${totalInboxAttention} needing attention)`
										: 'Inbox'}</Tooltip.Content
								>
							</Tooltip.Portal>
						</Tooltip.Root>
					</li>
				</ul>
			</div>

			<PinnableNavSection
				config={WORKSPACE_SECTION_CONFIG}
				pinnedIds={uiPrefs.workspaceNav}
				{collapsed}
				isItemActive={(item) => isActive(item.href)}
				onMoveItem={(id, beforeId, zone) => uiPrefs.moveWorkspaceItem(id, beforeId, zone)}
				onTogglePin={(id) => uiPrefs.toggleWorkspacePin(id)}
				{onNavigate}
			/>

			{#if !collapsed}
				<ExecutiveSidebarFavorites
					{projects}
					{workItems}
					{inboxItems}
					{projectTeams}
					{projectDueById}
					{offline}
					{isActive}
					{onNavigate}
				/>
			{/if}

			<PinnableNavSection
				config={ORG_SECTION_CONFIG}
				pinnedIds={uiPrefs.orgNav}
				{collapsed}
				isItemActive={(item) => isOrgNavItemActive(item.id, pathname)}
				onMoveItem={(id, beforeId, zone) => uiPrefs.moveOrgItem(id, beforeId, zone)}
				onTogglePin={(id) => uiPrefs.toggleOrgPin(id)}
				{onNavigate}
			/>
		</nav>

		<ExecutiveSidebarFooter
			{connection}
			{collapsed}
			{tessOpen}
			{offline}
			{activeTessJobs}
			{onToggleTess}
			{onNavigate}
			onAccountMenuOpenChange={(next) => {
				accountMenuOpen = next;
				notifyMenuOpen();
			}}
		/>
	</aside>
</Tooltip.Provider>

<style>
	.exec-sidebar {
		position: relative;
		isolation: isolate;
		background-color: var(--exec-sidebar-bg);
		background-image: var(--exec-sidebar-sheen);
		-webkit-backdrop-filter: var(--exec-sidebar-blur);
		backdrop-filter: var(--exec-sidebar-blur);
		box-shadow: inset -1px 0 0 var(--exec-border);
	}
	.exec-logo-name {
		font-size: 0.9375rem;
		line-height: 1.5rem;
		font-weight: var(--exec-logo-weight);
		letter-spacing: var(--exec-logo-tracking);
		color: var(--exec-ink);
	}
	.exec-brand-mark {
		background: var(--exec-cta-bg);
	}
	.exec-logo:hover,
	.exec-icon-btn:hover {
		background: var(--exec-accent-soft);
	}
	.exec-icon-btn {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.exec-icon-btn:hover {
		color: var(--exec-ink);
	}
	.exec-icon-btn.is-pinned {
		color: var(--exec-accent-ink);
		background: var(--exec-accent-soft);
	}
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
	.exec-nav-dot {
		margin-left: auto;
		width: 7px;
		height: 7px;
		border-radius: 999px;
		flex: none;
		background: var(--exec-nav-dot);
		box-shadow: 0 0 0 3px var(--exec-nav-dot-halo);
	}
	.exec-nav-dot.is-rail {
		position: absolute;
		top: 9px;
		right: 9px;
		margin-left: 0;
		box-shadow: 0 0 0 2px var(--exec-nav-dot-ring);
	}
	.exec-sidebar-search {
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.exec-sidebar-search:hover:not([aria-disabled='true']) {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.exec-sidebar-search.is-offline,
	.exec-sidebar-search[aria-disabled='true'] {
		opacity: 0.55;
		cursor: not-allowed;
	}
	.exec-sidebar-kbd {
		font-family: var(--font-mono);
		font-size: 0.65rem;
		line-height: 1;
		padding: 2px 5px;
		border-radius: 5px;
		background: var(--exec-cmdk-kbd-bg);
		color: var(--exec-cmdk-kbd-ink);
		box-shadow: var(--exec-cmdk-kbd-shadow);
	}
	@media (prefers-reduced-motion: reduce) {
		.exec-icon-btn,
		.exec-nav-item,
		.exec-sidebar-search {
			transition: none;
		}
	}
</style>
