<script lang="ts">
	// The Organization settings frame (app-shell-ux §0.1): underline tabs below 960 px, a left column of
	// links from 960 px. Each section is its own route; a builder replaces only that section's page.
	// The column also needs the frame to be 20 rem wide (the 14 rem column and room for the section), so at large text
	// in a narrow main column the section is not pushed out (§6a).
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import TabsNav, { type Tab } from '$lib/components/TabsNav.svelte';

	let { children } = $props();
	const sections: Tab[] = [
		{ label: 'General', href: '/settings' },
		{ label: 'Offices', href: '/settings/offices', match: 'prefix' },
		{ label: 'Agent setup', href: '/settings/agents', match: 'prefix' },
		{ label: 'Roles', href: '/settings/roles', match: 'prefix' },
		{ label: 'Processes', href: '/settings/processes', match: 'prefix' },
		{ label: 'Members and access', href: '/settings/access', match: 'prefix' },
		{ label: 'Costs', href: '/settings/costs', match: 'prefix' },
		{ label: 'Updates', href: '/settings/updates', match: 'prefix' }
	];
	const isCurrent = (section: Tab) =>
		page.url.pathname === section.href ||
		(section.match === 'prefix' && page.url.pathname.startsWith(section.href + '/'));
</script>

<div class="@container">
	<div class="min-[960px]:@min-[20rem]:flex">
		<TabsNav
			label="Organization settings"
			tabs={sections}
			class="min-[960px]:@min-[20rem]:hidden"
		/>
		<nav
			aria-label="Organization settings"
			class="hidden w-56 shrink-0 border-r border-line-subtle px-4 py-6 min-[960px]:@min-[20rem]:block"
		>
			<ul role="list" class="space-y-1">
				{#each sections as section (section.href)}
					{@const current = isCurrent(section)}
					<li>
						<a
							href={resolve(section.href as '/')}
							aria-current={current ? 'page' : undefined}
							class="flex min-h-9 items-center rounded-md border-l-2 px-3 py-1.5 text-sm/6 font-semibold focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus {current
								? 'border-brand bg-surface-raised text-fg'
								: 'border-transparent text-fg-muted hover:bg-surface-raised hover:text-fg'}"
							>{section.label}</a
						>
					</li>
				{/each}
			</ul>
		</nav>
		<div class="min-w-0 flex-1">{@render children()}</div>
	</div>
</div>
