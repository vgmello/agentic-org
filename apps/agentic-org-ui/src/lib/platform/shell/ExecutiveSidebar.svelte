<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import Activity from '@lucide/svelte/icons/activity';
	import Bot from '@lucide/svelte/icons/bot';
	import Building2 from '@lucide/svelte/icons/building-2';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ClipboardList from '@lucide/svelte/icons/clipboard-list';
	import Folder from '@lucide/svelte/icons/folder';
	import House from '@lucide/svelte/icons/house';
	import Inbox from '@lucide/svelte/icons/inbox';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Network from '@lucide/svelte/icons/network';
	import PanelLeft from '@lucide/svelte/icons/panel-left';
	import Settings from '@lucide/svelte/icons/settings';
	import Star from '@lucide/svelte/icons/star';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import Workflow from '@lucide/svelte/icons/workflow';
	import type { Connection, ProjectSummary } from '$lib/proto';
	import { isOfficeOnline } from '$lib/home/model';
	import { UI_THEMES, uiPrefs } from './ui-preferences.svelte';

	let {
		connection,
		needsYouCount = 0,
		projects = [],
		collapsed = false,
		onNavigate,
		onToggleCollapse
	}: {
		connection: Connection;
		needsYouCount?: number;
		projects?: ProjectSummary[];
		collapsed?: boolean;
		onNavigate?: (event: MouseEvent, href: string) => void;
		onToggleCollapse?: () => void;
	} = $props();

	const pathname = $derived(page.url.pathname);
	const isActive = (href: string) => pathname === href || pathname.startsWith(`${href}/`);
	const officeOnline = $derived(isOfficeOnline(connection.office));

	const activeProjects = $derived(projects.filter((p) => !p.archived));
	const favoriteList = $derived.by(() => {
		if (uiPrefs.favoriteProjects.length > 0) {
			const favSet = new Set(uiPrefs.favoriteProjects);
			return activeProjects.filter((p) => favSet.has(p.id));
		}
		return activeProjects.slice(0, 2);
	});
</script>

<aside
	aria-label="Sidebar"
	class="exec-sidebar flex h-full min-w-0 flex-col overflow-hidden"
	data-collapsed={collapsed ? 'true' : undefined}
>
	<div class="flex min-h-14 shrink-0 items-center justify-between gap-2 px-3 pt-2">
		<a
			href={resolve('/home')}
			onclick={(event) => onNavigate?.(event, '/home')}
			class="exec-logo flex min-w-0 flex-1 items-center gap-2.5 rounded-lg px-2 py-1.5"
			title={connection.organization.name}
		>
			<span class="exec-brand-mark flex size-7 shrink-0 items-center justify-center rounded-lg text-xs font-bold text-on-emphasis" aria-hidden="true">
				A
			</span>
			{#if !collapsed}
				<span class="truncate text-sm font-semibold" style="color: var(--exec-ink);">
					{connection.organization.name}
				</span>
			{/if}
		</a>
		{#if onToggleCollapse}
			<button
				type="button"
				onclick={onToggleCollapse}
				aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
				aria-pressed={collapsed}
				title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
				class="exec-icon-btn flex size-7 shrink-0 items-center justify-center rounded-md"
			>
				<PanelLeft class="size-4" aria-hidden="true" />
			</button>
		{/if}
	</div>

	<nav aria-label="Main" class="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-3 py-3">
		<div>
			{#if !collapsed}
				<div class="exec-nav-label px-2.5 pb-1.5">Focus</div>
			{/if}
			<ul class="flex flex-col gap-0.5">
				<li>
					<a
						href={resolve('/home')}
						onclick={(e) => onNavigate?.(e, '/home')}
						aria-current={isActive('/home') ? 'page' : undefined}
						title="Home"
						class={['exec-nav-item', isActive('/home') && 'is-active']}
					>
						<House class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Home</span>{/if}
					</a>
				</li>
				<li>
					<a
						href={resolve('/inbox')}
						onclick={(e) => onNavigate?.(e, '/inbox')}
						aria-current={isActive('/inbox') ? 'page' : undefined}
						title="Inbox"
						class={['exec-nav-item', isActive('/inbox') && 'is-active']}
					>
						<Inbox class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}
							<span class="truncate">Inbox</span>
							{#if needsYouCount > 0}
								<span class="exec-inbox-badge ml-auto" aria-label="{needsYouCount} items need you">
									{needsYouCount}
								</span>
							{/if}
						{/if}
					</a>
				</li>
			</ul>
		</div>

		<div>
			{#if !collapsed}
				<div class="exec-nav-label px-2.5 pb-1.5">Workspace</div>
			{/if}
			<ul class="flex flex-col gap-0.5">
				<li>
					<a
						href={resolve('/work')}
						onclick={(e) => onNavigate?.(e, '/work')}
						aria-current={isActive('/work') ? 'page' : undefined}
						title="Work"
						class={['exec-nav-item', isActive('/work') && 'is-active']}
					>
						<ClipboardList class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Work</span>{/if}
					</a>
				</li>
				<li>
					<a
						href={resolve('/chat')}
						onclick={(e) => onNavigate?.(e, '/chat')}
						aria-current={isActive('/chat') ? 'page' : undefined}
						title="Chat"
						class={['exec-nav-item', isActive('/chat') && 'is-active']}
					>
						<MessageSquare class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Chat</span>{/if}
					</a>
				</li>
				<li>
					<a
						href={resolve('/activity')}
						onclick={(e) => onNavigate?.(e, '/activity')}
						aria-current={isActive('/activity') ? 'page' : undefined}
						title="Activity"
						class={['exec-nav-item', isActive('/activity') && 'is-active']}
					>
						<Activity class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Activity</span>{/if}
					</a>
				</li>
				<li>
					<a
						href={resolve('/projects')}
						onclick={(e) => onNavigate?.(e, '/projects')}
						aria-current={isActive('/projects') ? 'page' : undefined}
						title="Projects"
						class={['exec-nav-item', isActive('/projects') && 'is-active']}
					>
						<Folder class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Projects</span>{/if}
					</a>
				</li>
			</ul>
		</div>

		{#if !collapsed && favoriteList.length > 0}
			<div>
				<div class="exec-nav-label flex items-center gap-1.5 px-2.5 pb-1.5">
					<Star class="size-3" aria-hidden="true" />
					<span>Favorite projects</span>
				</div>
				<ul class="flex flex-col gap-0.5">
					{#each favoriteList as proj (proj.id)}
						{@const href = `/projects/${proj.id}`}
						<li>
							<a
								href={resolve(href as '/projects')}
								onclick={(e) => onNavigate?.(e, href)}
								aria-current={isActive(href) ? 'page' : undefined}
								class={['exec-nav-item', isActive(href) && 'is-active']}
							>
								<Folder class="size-4 shrink-0" aria-hidden="true" />
								<span class="truncate">{proj.name}</span>
							</a>
						</li>
					{/each}
				</ul>
			</div>
		{/if}

		<div>
			{#if !collapsed}
				<div class="exec-nav-label px-2.5 pb-1.5">Organization</div>
			{/if}
			<ul class="flex flex-col gap-0.5">
				<li>
					<a
						href={resolve('/org-chart')}
						onclick={(e) => onNavigate?.(e, '/org-chart')}
						aria-current={isActive('/org-chart') ? 'page' : undefined}
						title="Org chart"
						class={['exec-nav-item', isActive('/org-chart') && 'is-active']}
					>
						<Network class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Org chart</span>{/if}
					</a>
				</li>
				<li>
					<a
						href={resolve('/agents')}
						onclick={(e) => onNavigate?.(e, '/agents')}
						aria-current={isActive('/agents') ? 'page' : undefined}
						title="Agents"
						class={['exec-nav-item', isActive('/agents') && 'is-active']}
					>
						<Bot class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Agents</span>{/if}
					</a>
				</li>
				<li>
					<a
						href={resolve('/processes')}
						onclick={(e) => onNavigate?.(e, '/processes')}
						aria-current={isActive('/processes') ? 'page' : undefined}
						title="Processes"
						class={['exec-nav-item', isActive('/processes') && 'is-active']}
					>
						<Workflow class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Processes</span>{/if}
					</a>
				</li>
				<li>
					<a
						href={resolve('/offices')}
						onclick={(e) => onNavigate?.(e, '/offices')}
						aria-current={isActive('/offices') ? 'page' : undefined}
						title="Offices"
						class={['exec-nav-item', isActive('/offices') && 'is-active']}
					>
						<Building2 class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Offices</span>{/if}
					</a>
				</li>
				<li>
					<a
						href={resolve('/settings')}
						onclick={(e) => onNavigate?.(e, '/settings')}
						aria-current={isActive('/settings') ? 'page' : undefined}
						title="Settings"
						class={['exec-nav-item', isActive('/settings') && 'is-active']}
					>
						<Settings class="size-[18px] shrink-0" aria-hidden="true" />
						{#if !collapsed}<span class="truncate">Settings</span>{/if}
					</a>
				</li>
			</ul>
		</div>
	</nav>

	<div class="exec-sidebar-foot shrink-0 px-3 py-2.5">
		{#if !collapsed}
			<div class="exec-theme-pills mb-2.5 grid grid-cols-3 gap-1 rounded-lg p-1" role="group" aria-label="Theme">
				{#each UI_THEMES as option (option.id)}
					<button
						type="button"
						onclick={() => uiPrefs.setTheme(option.id)}
						aria-pressed={uiPrefs.theme === option.id}
						class={['exec-theme-pill rounded-md px-2 py-1 text-[11px] font-semibold', uiPrefs.theme === option.id && 'is-on']}
					>
						{option.shortLabel}
					</button>
				{/each}
			</div>
		{/if}
		<a
			href={resolve('/welcome')}
			onclick={(e) => onNavigate?.(e, '/welcome')}
			class="exec-foot-link flex items-center gap-2.5 rounded-lg px-2 py-1.5"
			title="Switch organization ({connection.organization.name})"
		>
			<span class="exec-foot-av flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold" style="color: var(--exec-ink);">
				Y
			</span>
			{#if !collapsed}
				<span class="min-w-0 flex-1">
					<span class="block truncate text-xs font-semibold" style="color: var(--exec-ink);">
						{connection.organization.name}
					</span>
					<span class="block truncate text-[11px]" style="color: var(--exec-muted);">
						Switch organization
					</span>
				</span>
			{/if}
		</a>
		<a
			href={resolve('/offices')}
			onclick={(e) => onNavigate?.(e, '/offices')}
			class="exec-foot-link mt-1 flex items-center gap-2 rounded-lg px-2 py-1 text-[11px]"
			title="{connection.office.host} · {connection.office.name} · {officeOnline ? 'Online' : 'Offline'}"
		>
			{#if officeOnline}
				<CheckCircle2 class="size-3.5 shrink-0" style="color: var(--exec-ok);" aria-hidden="true" />
			{:else}
				<WifiOff class="size-3.5 shrink-0" style="color: var(--exec-bad);" aria-hidden="true" />
			{/if}
			{#if !collapsed}
				<span class="truncate" style="color: var(--exec-muted);">
					{connection.office.host} · {connection.office.name} · {officeOnline ? 'Online' : 'Offline'}
				</span>
			{/if}
		</a>
	</div>
</aside>

<style>
	.exec-sidebar {
		background: var(--exec-sidebar-bg);
		backdrop-filter: var(--exec-blur);
		-webkit-backdrop-filter: var(--exec-blur);
		box-shadow: inset -1px 0 0 var(--exec-border);
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
	}
	.exec-nav-label {
		font-size: 0.6875rem;
		line-height: 1.125rem;
		font-weight: 650;
		letter-spacing: 0.07em;
		text-transform: uppercase;
		color: var(--exec-dim);
	}
	.exec-nav-item {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 10px;
		border-radius: 8px;
		font-size: 0.875rem;
		font-weight: 500;
		color: var(--exec-muted);
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
	.exec-inbox-badge {
		border-radius: 999px;
		padding: 0 7px;
		font-size: 11px;
		line-height: 18px;
		font-weight: 650;
		font-variant-numeric: tabular-nums;
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
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
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.exec-foot-av {
		background: var(--exec-avatar-bg);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
</style>
