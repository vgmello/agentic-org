<script lang="ts">
	// Ported from the Tailwind Plus sidebar layout (licensed; allowed inside apps/org-ui only).
	import { page } from '$app/state';
	import type { Connection } from '$lib/proto';
	import Logo from './Logo.svelte';
	import { icons } from './icons';
	import { NAV, navFor } from '$lib/nav';

	let { connection }: { connection: Connection } = $props();

	const current = $derived(navFor(page.url.pathname));

	const itemCurrent = 'group flex gap-x-3 rounded-md bg-white/5 p-2 text-sm/6 font-semibold text-white';
	const itemDefault =
		'group flex gap-x-3 rounded-md p-2 text-sm/6 font-semibold text-gray-400 hover:bg-white/5 hover:text-white';
	const focusRing = 'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-500';
</script>

<div class="flex grow flex-col gap-y-5 overflow-y-auto bg-black/10 px-6 ring-1 ring-white/5">
	<!-- Application identity. -->
	<div class="relative flex h-24 shrink-0 items-center gap-x-3 pt-8">
		<Logo class="h-8 w-auto" />
		<span class="truncate text-base/7 font-semibold text-white" title={connection.organization.name}>
			{connection.organization.name}
		</span>
	</div>
	<nav class="relative flex flex-1 flex-col" aria-label="Main">
		<ul role="list" class="flex flex-1 flex-col gap-y-7">
			<li>
				<div class="text-xs/6 font-semibold text-gray-400">Navigation</div>
				<ul role="list" class="-mx-2 mt-2 space-y-1">
					{#each NAV as item (item.href)}
						{@const active = current === item}
						<li>
							<a
								href={item.href}
								aria-current={active ? 'page' : undefined}
								class="{active ? itemCurrent : itemDefault} {focusRing}"
							>
								<svg
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									stroke-width="1.5"
									aria-hidden="true"
									class="size-6 shrink-0 {active ? 'text-white' : 'text-gray-400 group-hover:text-white'}"
								>
									<path d={icons[item.icon]} stroke-linecap="round" stroke-linejoin="round" />
								</svg>
								{item.name}
							</a>
						</li>
					{/each}
				</ul>
			</li>
			<li class="-mx-6 mt-auto">
				<a
					href="/welcome"
					class="flex items-center gap-x-3 px-6 py-3 text-sm/6 hover:bg-white/5 {focusRing}"
					aria-label="Switch organization (now {connection.organization.name}, {connection.office.name} office on {connection.office.host})"
				>
					<span class="min-w-0">
						<span class="block truncate font-semibold text-white">{connection.office.host}</span>
						<span class="block text-xs/5 text-gray-400">{connection.office.name} office · Switch organization</span>
					</span>
				</a>
			</li>
		</ul>
	</nav>
</div>
