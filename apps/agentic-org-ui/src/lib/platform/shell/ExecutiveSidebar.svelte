<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import Activity from '@lucide/svelte/icons/activity';
	import Bot from '@lucide/svelte/icons/bot';
	import Building2 from '@lucide/svelte/icons/building-2';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import CircleHelp from '@lucide/svelte/icons/circle-help';
	import ClipboardList from '@lucide/svelte/icons/clipboard-list';
	import Folder from '@lucide/svelte/icons/folder';
	import House from '@lucide/svelte/icons/house';
	import Inbox from '@lucide/svelte/icons/inbox';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Network from '@lucide/svelte/icons/network';
	import PanelLeft from '@lucide/svelte/icons/panel-left';
	import Settings from '@lucide/svelte/icons/settings';
	import Star from '@lucide/svelte/icons/star';
	import Users from '@lucide/svelte/icons/users';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import Workflow from '@lucide/svelte/icons/workflow';
	import type { Connection, ProjectSummary } from '$lib/proto';
	import ShellAccountMenu from './ShellAccountMenu.svelte';
	import { UI_THEMES, uiPrefs } from './ui-preferences.svelte';

	let {
		connection,
		needsYouCount = 0,
		projects = [],
		collapsed = false,
		pinned = true,
		onNavigate,
		onToggleCollapse,
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
		onNavigate?: (event: MouseEvent, href: string) => void;
		onToggleCollapse?: () => void;
		onPointerEnter?: () => void;
		onPointerLeave?: () => void;
		onFocusIn?: (event: FocusEvent) => void;
		onFocusOut?: (event: FocusEvent) => void;
	} = $props();

	const pathname = $derived(page.url.pathname);
	const isActive = (href: string) => pathname === href || pathname.startsWith(`${href}/`);
	const isTeamsActive = $derived(
		pathname === '/teams' ||
			(pathname.startsWith('/teams/') &&
				!pathname.startsWith('/teams/members') &&
				!pathname.startsWith('/teams/org-chart'))
	);
	interface FooterOfficeState {
		label: string;
		tone: 'ok' | 'bad' | 'warn' | 'muted';
		icon: 'online' | 'offline' | 'warn' | 'unknown';
	}

	function connectedOfficeStatus(office: Connection['office']): FooterOfficeState {
		switch (office.status) {
			case 'active':
				return { label: 'Online', tone: 'ok', icon: 'online' };
			case 'starting':
				return { label: 'Starting', tone: 'muted', icon: 'unknown' };
			case 'pending':
				return { label: 'Invited', tone: 'muted', icon: 'unknown' };
			case 'unavailable':
				return { label: 'Stopped', tone: 'bad', icon: 'offline' };
			case 'offline':
				return office.role === 'hq'
					? { label: 'Stopped', tone: 'bad', icon: 'offline' }
					: { label: 'Offline', tone: 'muted', icon: 'offline' };
			case 'update_needed':
				return { label: 'Update needed', tone: 'warn', icon: 'warn' };
			default:
				return { label: 'Status unknown', tone: 'muted', icon: 'unknown' };
		}
	}

	const officeStatus = $derived(connectedOfficeStatus(connection.office));
	const officeStatusLabel = $derived(`${connection.office.name} office · ${officeStatus.label}`);

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
			<a
				href={resolve('/home')}
				onclick={(event) => onNavigate?.(event, '/home')}
				class={[
					'exec-logo flex min-w-0 items-center rounded-lg py-1.5',
					collapsed ? 'size-11 justify-center px-0' : 'flex-1 gap-2.5 px-2'
				]}
				title={connection.organization.name}
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
			{#if onToggleCollapse && !collapsed}
				<button
					type="button"
					onclick={onToggleCollapse}
					aria-label={pinned ? 'Auto-hide sidebar' : 'Pin sidebar open'}
					aria-pressed={pinned}
					title={pinned ? 'Auto-hide sidebar' : 'Pin sidebar open'}
					class={[
						'exec-icon-btn flex size-7 shrink-0 items-center justify-center rounded-md',
						pinned && 'is-pinned'
					]}
				>
					<PanelLeft class="size-4" aria-hidden="true" />
				</button>
			{/if}
		</div>

		<nav
			aria-label="Main"
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
						<a
							href={resolve('/home')}
							onclick={(e) => onNavigate?.(e, '/home')}
							aria-current={isActive('/home') ? 'page' : undefined}
							title="Home"
							class={['exec-nav-item', collapsed && 'is-rail', isActive('/home') && 'is-active']}
						>
							<House class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Home</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/inbox')}
							onclick={(e) => onNavigate?.(e, '/inbox')}
							aria-current={isActive('/inbox') ? 'page' : undefined}
							title={needsYouCount > 0 ? `Inbox (${needsYouCount} waiting on you)` : 'Inbox'}
							class={['exec-nav-item', collapsed && 'is-rail', isActive('/inbox') && 'is-active']}
						>
							<Inbox class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Inbox</span>
							{#if needsYouCount > 0}
								<span class={['exec-nav-dot', collapsed && 'is-rail']} aria-hidden="true"></span>
								<span class="sr-only">, {needsYouCount} waiting on you</span>
							{/if}
						</a>
					</li>
				</ul>
			</div>

			<div class={collapsed ? 'w-full' : undefined}>
				{#if !collapsed}
					<div class="exec-nav-label px-2.5 pb-1.5">Workspace</div>
				{/if}
				<ul class={['flex flex-col', collapsed ? 'items-center gap-1' : 'gap-0.5']}>
					<li>
						<a
							href={resolve('/work')}
							onclick={(e) => onNavigate?.(e, '/work')}
							aria-current={isActive('/work') ? 'page' : undefined}
							title="Work"
							class={['exec-nav-item', collapsed && 'is-rail', isActive('/work') && 'is-active']}
						>
							<ClipboardList class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Work</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/chat')}
							onclick={(e) => onNavigate?.(e, '/chat')}
							aria-current={isActive('/chat') ? 'page' : undefined}
							title="Chat"
							class={['exec-nav-item', collapsed && 'is-rail', isActive('/chat') && 'is-active']}
						>
							<MessageSquare class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Chat</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/activity')}
							onclick={(e) => onNavigate?.(e, '/activity')}
							aria-current={isActive('/activity') ? 'page' : undefined}
							title="Activity"
							class={[
								'exec-nav-item',
								collapsed && 'is-rail',
								isActive('/activity') && 'is-active'
							]}
						>
							<Activity class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Activity</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/projects')}
							onclick={(e) => onNavigate?.(e, '/projects')}
							aria-current={isActive('/projects') ? 'page' : undefined}
							title="Projects"
							class={[
								'exec-nav-item',
								collapsed && 'is-rail',
								isActive('/projects') && 'is-active'
							]}
						>
							<Folder class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Projects</span>
						</a>
					</li>
				</ul>
			</div>

			{#if !collapsed && favoriteList.length > 0}
				<div>
					<div class="flex items-center justify-between gap-1 px-2.5 pb-1.5">
						<div class="exec-nav-label flex items-center gap-1.5">
							<Star class="size-3" aria-hidden="true" />
							<span>Favorite projects</span>
						</div>
						<button
							type="button"
							onclick={() => (favoritesOpen = !favoritesOpen)}
							aria-expanded={favoritesOpen}
							aria-controls="exec-nav-favs"
							aria-label={favoritesOpen ? 'Collapse favorite projects' : 'Expand favorite projects'}
							class="exec-icon-btn flex size-6 items-center justify-center rounded-md"
						>
							<ChevronRight
								class={['size-3.5 transition-transform duration-150', favoritesOpen && 'rotate-90']}
								aria-hidden="true"
							/>
						</button>
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
										<button
											type="button"
											onclick={() => uiPrefs.toggleFavorite(proj.id)}
											aria-label="Remove {proj.name} from favorites"
											title="Unpin from favorites"
											class="exec-fav-unpin flex size-6 shrink-0 items-center justify-center rounded-md opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
										>
											<Star class="size-3.5 fill-current" aria-hidden="true" />
										</button>
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
					<li>
						<a
							href={resolve('/teams')}
							onclick={(e) => onNavigate?.(e, '/teams')}
							aria-current={isTeamsActive ? 'page' : undefined}
							title="Teams"
							class={['exec-nav-item', collapsed && 'is-rail', isTeamsActive && 'is-active']}
						>
							<Users class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Teams</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/teams/org-chart')}
							onclick={(e) => onNavigate?.(e, '/teams/org-chart')}
							aria-current={isActive('/teams/org-chart') ? 'page' : undefined}
							title="Org chart"
							class={[
								'exec-nav-item',
								collapsed && 'is-rail',
								isActive('/teams/org-chart') && 'is-active'
							]}
						>
							<Network class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Org chart</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/teams/members')}
							onclick={(e) => onNavigate?.(e, '/teams/members')}
							aria-current={isActive('/teams/members') ? 'page' : undefined}
							title="Members"
							class={[
								'exec-nav-item',
								collapsed && 'is-rail',
								isActive('/teams/members') && 'is-active'
							]}
						>
							<Bot class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Members</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/settings/processes')}
							onclick={(e) => onNavigate?.(e, '/settings/processes')}
							aria-current={isActive('/settings/processes') ? 'page' : undefined}
							title="Processes"
							class={[
								'exec-nav-item',
								collapsed && 'is-rail',
								isActive('/settings/processes') && 'is-active'
							]}
						>
							<Workflow class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Processes</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/settings/offices')}
							onclick={(e) => onNavigate?.(e, '/settings/offices')}
							aria-current={isActive('/settings/offices') ? 'page' : undefined}
							title="Offices"
							class={[
								'exec-nav-item',
								collapsed && 'is-rail',
								isActive('/settings/offices') && 'is-active'
							]}
						>
							<Building2 class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Offices</span>
						</a>
					</li>
					<li>
						<a
							href={resolve('/settings')}
							onclick={(e) => onNavigate?.(e, '/settings')}
							aria-current={pathname === '/settings' ? 'page' : undefined}
							title="Settings"
							class={[
								'exec-nav-item',
								collapsed && 'is-rail',
								pathname === '/settings' && 'is-active'
							]}
						>
							<Settings class="size-[18px] shrink-0" aria-hidden="true" />
							<span class={collapsed ? 'sr-only' : 'truncate'}>Settings</span>
						</a>
					</li>
				</ul>
			</div>
		</nav>

		<div
			class={[
				'exec-sidebar-foot shrink-0 py-2',
				collapsed ? 'flex flex-col items-center gap-1 px-2' : 'px-3'
			]}
		>
			{#if !collapsed}
				<div
					class="exec-theme-pills mb-2 grid grid-cols-3 gap-1 rounded-lg p-1"
					role="group"
					aria-label="Theme"
				>
					{#each UI_THEMES as option (option.id)}
						<button
							type="button"
							onclick={() => uiPrefs.setTheme(option.id)}
							aria-pressed={uiPrefs.theme === option.id}
							title={option.label}
							class={[
								'exec-theme-pill min-h-9 truncate rounded-md px-1.5 py-1 text-[11px] font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid',
								uiPrefs.theme === option.id && 'is-on'
							]}
						>
							{option.shortLabel}
						</button>
					{/each}
				</div>
			{/if}
			<ShellAccountMenu {connection} compact={collapsed} />
			<a
				href={resolve('/settings/offices')}
				onclick={(e) => onNavigate?.(e, '/settings/offices')}
				aria-label={officeStatusLabel}
				class={[
					'exec-foot-link flex items-center rounded-lg text-xs leading-5 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus focus-visible:outline-solid max-sm:min-h-11 [@media(pointer:coarse)]:min-h-11',
					collapsed ? 'size-11 justify-center p-0' : 'mt-1 gap-2 px-2 py-1'
				]}
				title="{connection.office.host} · {officeStatusLabel}"
			>
				{#if officeStatus.icon === 'online'}
					<CheckCircle2
						class="size-3.5 shrink-0"
						style="color: var(--exec-ok);"
						aria-hidden="true"
					/>
				{:else if officeStatus.icon === 'offline'}
					<WifiOff
						class="size-3.5 shrink-0"
						style="color: {officeStatus.tone === 'bad' ? 'var(--exec-bad)' : 'var(--exec-muted)'};"
						aria-hidden="true"
					/>
				{:else if officeStatus.icon === 'warn'}
					<CircleAlert
						class="size-3.5 shrink-0"
						style="color: var(--exec-warn);"
						aria-hidden="true"
					/>
				{:else}
					<CircleHelp
						class="size-3.5 shrink-0"
						style="color: var(--exec-muted);"
						aria-hidden="true"
					/>
				{/if}
				{#if !collapsed}
					<span class="min-w-0 [overflow-wrap:anywhere]" style="color: var(--exec-muted);">
						{officeStatusLabel}
					</span>
				{/if}
			</a>
		</div>
	</aside>
</Tooltip.Provider>

<style>
	.exec-sidebar {
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
	.exec-icon-btn:hover,
	.exec-foot-link:hover {
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
	.exec-sidebar-foot {
		border-top: 1px solid var(--exec-border);
	}
	.exec-theme-pills {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.exec-theme-pill {
		color: var(--exec-muted);
	}
	.exec-theme-pill:hover {
		color: var(--exec-ink);
	}
	.exec-theme-pill.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1.5px var(--tone-accent-ink);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.exec-theme-pill {
			min-height: 44px;
		}
	}
	@media (forced-colors: active) {
		.exec-theme-pill.is-on {
			border: 2px solid ButtonText;
		}
	}
</style>
