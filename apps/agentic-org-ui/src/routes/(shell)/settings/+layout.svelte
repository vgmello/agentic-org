<script lang="ts">
	// The Organization settings frame (app-shell-ux §0.1): underline tabs below 960 px, a left column of
	// links from 960 px. Each section is its own route; a builder replaces only that section's page.
	// The column also needs the frame to be 28 rem wide (the 14 rem column plus at least 14 rem for the section), so
	// at enlarged text (200%+) the section keeps adequate room instead of being squeezed beside two sidebars (SHELL-30).
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import TabsNav, { type Tab } from '$lib/components/TabsNav.svelte';
	import { singleKeyShortcutsOn } from '$lib/platform/workspace/shortcuts';

	let { children } = $props();
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());
	const sections: Tab[] = [
		{ label: 'General', href: '/settings' },
		{ label: 'Offices', href: '/settings/offices', match: 'prefix' },
		{ label: 'Agent setup', href: '/settings/agents', match: 'prefix' },
		{ label: 'Roles', href: '/settings/roles', match: 'prefix' },
		{ label: 'Processes', href: '/settings/processes', match: 'prefix' },
		{ label: 'Members and access', href: '/settings/access', match: 'prefix' },
		{ label: 'Security & Access', href: '/settings/security', match: 'prefix' },
		{ label: 'Costs', href: '/settings/costs', match: 'prefix' },
		{ label: 'Updates', href: '/settings/updates', match: 'prefix' }
	];
	const isCurrent = (section: Tab) =>
		page.url.pathname === section.href ||
		(section.match === 'prefix' && page.url.pathname.startsWith(section.href + '/'));
</script>

<div class="@container">
	<div class="min-[960px]:@min-[28rem]:flex">
		<div class="section-header settings-picker-header min-[960px]:@min-[28rem]:hidden">
			<TabsNav label="Organization settings" tabs={sections} />
		</div>
		<nav
			aria-label="Organization settings"
			data-page-menu=""
			aria-keyshortcuts={shortcutsEnabled ? 'M' : undefined}
			class="hidden w-56 shrink-0 border-r border-line px-3.5 py-5 min-[960px]:@min-[28rem]:block"
		>
			<p
				aria-hidden="true"
				class="px-3 pb-2 text-xs/4 font-semibold tracking-wider text-fg-subtle uppercase"
			>
				Organization
			</p>
			<ul role="list" class="flex flex-col gap-1">
				{#each sections as section (section.href)}
					{@const current = isCurrent(section)}
					<li>
						<a
							href={resolve(section.href as '/')}
							data-menu-label={section.label}
							aria-current={current ? 'page' : undefined}
							class={[
								'settings-nav-item flex min-h-9 items-center rounded-lg px-3 py-1.5 text-sm/6 font-semibold transition-colors focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus',
								current ? 'is-current' : 'text-fg-muted hover:bg-surface-raised hover:text-fg'
							]}>{section.label}</a
						>
					</li>
				{/each}
			</ul>
		</nav>
		<div class="min-w-0 flex-1">{@render children()}</div>
	</div>
</div>

<style>
	.settings-nav-item.is-current {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow:
			inset 2.5px 0 0 var(--tone-accent),
			inset 0 0 0 1px var(--exec-accent-ring);
	}
</style>
