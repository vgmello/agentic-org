<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import Activity from '@lucide/svelte/icons/activity';
	import Bot from '@lucide/svelte/icons/bot';
	import Building2 from '@lucide/svelte/icons/building-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import ClipboardList from '@lucide/svelte/icons/clipboard-list';
	import Folder from '@lucide/svelte/icons/folder';
	import House from '@lucide/svelte/icons/house';
	import Inbox from '@lucide/svelte/icons/inbox';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Network from '@lucide/svelte/icons/network';
	import PanelLeft from '@lucide/svelte/icons/panel-left';
	import Search from '@lucide/svelte/icons/search';
	import Settings from '@lucide/svelte/icons/settings';
	import Star from '@lucide/svelte/icons/star';
	import Users from '@lucide/svelte/icons/users';
	import Workflow from '@lucide/svelte/icons/workflow';
	import type { Connection, ProjectSummary } from '$lib/proto';
	import ExecutiveSidebarFooter from './ExecutiveSidebarFooter.svelte';
	import OrganizationSwitcher from './OrganizationSwitcher.svelte';
	import { useNotifications } from './notifications.svelte';
	import { singleKeyShortcutsOn } from './shortcuts';
	import { uiPrefs } from './ui-preferences.svelte';

	let {
		connection,
		needsYouCount = 0,
		projects = [],
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
	const isTeamsActive = $derived(
		pathname === '/teams' ||
			(pathname.startsWith('/teams/') &&
				!pathname.startsWith('/teams/members') &&
				!pathname.startsWith('/teams/org-chart'))
	);

	const activeProjects = $derived(projects.filter((p) => !p.archived));
	let favoritesOpen = $state(true);
	const favoriteList = $derived.by(() => {
		if (uiPrefs.favoriteProjects.length > 0) {
			const favSet = new Set(uiPrefs.favoriteProjects);
			return activeProjects.filter((p) => favSet.has(p.id));
		}
		return activeProjects.slice(0, 2);
	});
</script>

<Tooltip.Provider delayDuration={150}>
	<aside
		aria-label="Sidebar"
		onpointerenter={onPointerEnter}
		onpointerleave={onPointerLeave}
		onfocusin={onFocusIn}
		onfocusout={onFocusOut}
		class="exec-sidebar flex h-full min-w-0 flex-col overflow-hidden"
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
							{...props}
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
					<Tooltip.Content
						side="right"
						sideOffset={6}
						class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
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
									side="bottom"
									sideOffset={6}
									class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
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
				<Tooltip.Root disabled={!collapsed}>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<button
								{...props}
								type="button"
								onclick={onOpenSearch}
								aria-label="Search (⌘K)"
								class={[
									'exec-sidebar-search flex items-center rounded-lg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid',
									collapsed ? 'size-10 justify-center p-0' : 'w-full gap-2 px-2.5 py-1.5 text-left'
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
						<Tooltip.Content
							side="right"
							sideOffset={6}
							class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
							>Search (⌘K)</Tooltip.Content
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
										{...props}
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
									side="right"
									sideOffset={6}
									class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
									>Home</Tooltip.Content
								>
							</Tooltip.Portal>
						</Tooltip.Root>
					</li>
					<li>
						<Tooltip.Root disabled={!collapsed}>
							<Tooltip.Trigger>
								{#snippet child({ props })}
									<a
										{...props}
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
									side="right"
									sideOffset={6}
									class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
									>{totalInboxAttention > 0
										? `Inbox (${totalInboxAttention} needing attention)`
										: 'Inbox'}</Tooltip.Content
								>
							</Tooltip.Portal>
						</Tooltip.Root>
					</li>
				</ul>
			</div>

			<div class={collapsed ? 'w-full' : undefined}>
				{#if !collapsed}
					<div class="exec-nav-label px-2.5 pb-1.5">Workspace</div>
				{/if}
				<ul class={['flex flex-col', collapsed ? 'items-center gap-1' : 'gap-0.5']}>
					{#each [{ href: '/work', label: 'Work', Icon: ClipboardList }, { href: '/chat', label: 'Chat', Icon: MessageSquare }, { href: '/activity', label: 'Activity', Icon: Activity }, { href: '/projects', label: 'Projects', Icon: Folder }] as const as item (item.href)}
						{@const NavIcon = item.Icon}
						<li>
							<Tooltip.Root disabled={!collapsed}>
								<Tooltip.Trigger>
									{#snippet child({ props })}
										<a
											{...props}
											href={resolve(item.href)}
											onclick={(e) => onNavigate?.(e, item.href)}
											aria-current={isActive(item.href) ? 'page' : undefined}
											class={[
												'exec-nav-item',
												collapsed && 'is-rail',
												isActive(item.href) && 'is-active'
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
						</li>
					{/each}
				</ul>
			</div>

			{#if !collapsed && favoriteList.length > 0}
				<div>
					<div class="flex items-center justify-between gap-1 px-2.5 pb-1.5">
						<div class="exec-nav-label flex items-center gap-1.5">
							<Star class="size-3" aria-hidden="true" />
							<span>Favorite projects</span>
						</div>
						<Tooltip.Root>
							<Tooltip.Trigger>
								{#snippet child({ props })}
									<button
										{...props}
										type="button"
										onclick={() => (favoritesOpen = !favoritesOpen)}
										aria-expanded={favoritesOpen}
										aria-controls="exec-nav-favs"
										aria-label={favoritesOpen
											? 'Collapse favorite projects'
											: 'Expand favorite projects'}
										class="exec-icon-btn flex size-9 items-center justify-center rounded-md [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11"
									>
										<ChevronRight
											class={[
												'size-3.5 transition-transform duration-150',
												favoritesOpen && 'rotate-90'
											]}
											aria-hidden="true"
										/>
									</button>
								{/snippet}
							</Tooltip.Trigger>
							<Tooltip.Portal>
								<Tooltip.Content
									side="right"
									sideOffset={6}
									class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
									>{favoritesOpen
										? 'Collapse favorite projects'
										: 'Expand favorite projects'}</Tooltip.Content
								>
							</Tooltip.Portal>
						</Tooltip.Root>
					</div>
					{#if favoritesOpen}
						<ul id="exec-nav-favs" class="flex flex-col gap-0.5">
							{#each favoriteList as proj (proj.id)}
								{@const href = `/projects/${proj.id}/work`}
								<li class="group flex items-center gap-0.5">
									<a
										href={resolve(href as '/projects')}
										onclick={(e) => onNavigate?.(e, href)}
										aria-current={isActive(`/projects/${proj.id}`) ? 'page' : undefined}
										class={[
											'exec-nav-item min-w-0 flex-1',
											isActive(`/projects/${proj.id}`) && 'is-active'
										]}
									>
										<Folder class="size-4 shrink-0" aria-hidden="true" />
										<span class="truncate">{proj.name}</span>
									</a>
									{#if uiPrefs.isFavorite(proj.id)}
										<Tooltip.Root>
											<Tooltip.Trigger>
												{#snippet child({ props })}
													<button
														{...props}
														type="button"
														onclick={() => uiPrefs.toggleFavorite(proj.id)}
														aria-label="Remove {proj.name} from favorites"
														class="exec-fav-unpin flex size-9 shrink-0 items-center justify-center rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11"
													>
														<Star class="size-3.5 fill-current" aria-hidden="true" />
													</button>
												{/snippet}
											</Tooltip.Trigger>
											<Tooltip.Portal>
												<Tooltip.Content
													side="right"
													sideOffset={6}
													class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
													>Unpin from favorites</Tooltip.Content
												>
											</Tooltip.Portal>
										</Tooltip.Root>
									{/if}
								</li>
							{/each}
						</ul>
					{/if}
				</div>
			{/if}

			<div class={collapsed ? 'w-full' : undefined}>
				{#if !collapsed}
					<div class="exec-nav-label px-2.5 pb-1.5">Organization</div>
				{/if}
				<ul class={['flex flex-col', collapsed ? 'items-center gap-1' : 'gap-0.5']}>
					{#each [{ href: '/teams', label: 'Teams', Icon: Users, active: isTeamsActive }, { href: '/teams/org-chart', label: 'Org chart', Icon: Network, active: isActive('/teams/org-chart') }, { href: '/teams/members', label: 'Members', Icon: Bot, active: isActive('/teams/members') }, { href: '/settings/processes', label: 'Processes', Icon: Workflow, active: isActive('/settings/processes') }, { href: '/settings/offices', label: 'Offices', Icon: Building2, active: isActive('/settings/offices') }, { href: '/settings', label: 'Settings', Icon: Settings, active: pathname === '/settings' }] as const as item (item.href)}
						{@const OrgIcon = item.Icon}
						<li>
							<Tooltip.Root disabled={!collapsed}>
								<Tooltip.Trigger>
									{#snippet child({ props })}
										<a
											{...props}
											href={resolve(item.href)}
											onclick={(e) => onNavigate?.(e, item.href)}
											aria-current={item.active ? 'page' : undefined}
											class={['exec-nav-item', collapsed && 'is-rail', item.active && 'is-active']}
										>
											<OrgIcon class="size-[18px] shrink-0" aria-hidden="true" />
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
						</li>
					{/each}
				</ul>
			</div>
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
	.exec-fav-unpin {
		color: var(--tone-warn);
	}
	.exec-fav-unpin:hover {
		color: var(--tone-warn-ink);
		background: var(--tone-warn-bg);
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
	.exec-sidebar-search:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
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
</style>
