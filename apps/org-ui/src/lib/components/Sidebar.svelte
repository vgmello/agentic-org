<script lang="ts">
	// Ported from the Tailwind Plus sidebar layout (licensed; allowed inside apps/org-ui only).
	import { page } from '$app/state';
	import { innerWidth } from 'svelte/reactivity/window';
	import { Tooltip, mergeProps } from 'bits-ui';
	import type { Connection } from '$lib/proto';
	import Logo from './Logo.svelte';
	import { icons } from './icons';
	import { NAV, navFor } from '$lib/nav';

	let { connection, presentation = 'responsive', onNavigate }: {
		connection: Connection;
		presentation?: 'responsive' | 'drawer';
		onNavigate?: (event: MouseEvent, href: string) => void;
	} = $props();

	const current = $derived(navFor(page.url.pathname));
	const responsive = $derived(presentation === 'responsive');
	const rail = $derived(responsive && (innerWidth.current ?? 960) >= 640 && (innerWidth.current ?? 960) < 960);
	const switchLabel = $derived(`Switch organization (now ${connection.organization.name}, ${connection.office.name} office on ${connection.office.host})`);

	const itemCurrent = 'group flex gap-x-3 rounded-md bg-white/5 p-2 text-sm/6 font-semibold text-white';
	const itemDefault =
		'group flex gap-x-3 rounded-md p-2 text-sm/6 font-semibold text-gray-400 hover:bg-white/5 hover:text-white';
	const focusRing = 'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-500';

	function revealRailLink(event: FocusEvent) {
		if (!rail || !(event.currentTarget instanceof HTMLAnchorElement)) return;
		// Scroll before Bits UI opens the tooltip. WebKit can otherwise deliver the
		// focus scroll after the provider's frame guard and dismiss the tooltip.
		event.currentTarget.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
	}
</script>

<Tooltip.Provider delayDuration={150}>
<div class="flex min-w-0 grow flex-col gap-y-5 overflow-y-auto bg-black/10 px-6 ring-1 ring-white/5 {responsive ? 'max-[960px]:px-2' : ''}">
	<!-- Application identity. -->
	<div class="relative flex h-24 shrink-0 items-center gap-x-3 pt-8 {responsive ? 'max-[960px]:hidden' : 'pr-12'}">
		<Logo class="h-8 w-auto" />
		<span class="truncate text-base/7 font-semibold text-white" title={connection.organization.name}>
			{connection.organization.name}
		</span>
	</div>
	{#if responsive}
		<div class="hidden h-24 shrink-0 items-center pt-8 min-[640px]:max-[960px]:flex">
			<Tooltip.Root disabled={!rail}>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<a {...mergeProps({ onfocus: revealRailLink }, props)} type={undefined} href="/welcome" aria-label={switchLabel} class="flex size-12 items-center justify-center rounded-md hover:bg-white/5 {focusRing}">
							<Logo class="size-8" />
						</a>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal><Tooltip.Content role="tooltip" side="right" sideOffset={12} class="z-[100] max-w-xs rounded-md bg-gray-950 px-3 py-2 text-xs text-white shadow-lg wrap-break-word">{connection.organization.name} · Switch organization</Tooltip.Content></Tooltip.Portal>
			</Tooltip.Root>
		</div>
	{/if}
	<nav class="relative flex flex-1 flex-col" aria-label="Main">
		<ul role="list" class="flex flex-1 flex-col gap-y-7">
			<li>
				<div class="text-xs/6 font-semibold text-gray-400 {responsive ? 'max-[960px]:sr-only' : ''}">Navigation</div>
				<ul role="list" class="-mx-2 mt-2 space-y-1 {responsive ? 'max-[960px]:mx-0' : ''}">
					{#each NAV as item (item.href)}
						{@const active = current === item}
						<li>
							<Tooltip.Root disabled={!rail}>
							<Tooltip.Trigger>
							{#snippet child({ props })}
							<a {...mergeProps({ onfocus: revealRailLink }, props, { onclick: (event: MouseEvent) => onNavigate?.(event, item.href) })}
								type={undefined} href={item.href} aria-label={item.badge ? `${item.name}, ${item.badge.label}` : item.name}
								aria-current={active ? 'page' : undefined}
								class="relative {active ? itemCurrent : itemDefault} {focusRing} {responsive ? 'max-[960px]:size-12 max-[960px]:items-center max-[960px]:justify-center max-[960px]:gap-x-0' : ''}">
								<span class="relative flex size-6 shrink-0 items-center justify-center">
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
								</span>
								<span class={responsive ? 'max-[960px]:sr-only' : ''}>{item.name}</span>
								{#if item.badge}
									<span aria-hidden="true" class="ml-auto max-w-10 shrink-0 self-center truncate rounded-full bg-amber-300 px-1.5 text-xs/4 font-semibold text-gray-950 {responsive ? 'max-[960px]:absolute max-[960px]:top-0 max-[960px]:right-0' : ''}">{item.badge.text}</span>
								{/if}
							</a>
							{/snippet}
							</Tooltip.Trigger>
							<Tooltip.Portal><Tooltip.Content role="tooltip" side="right" sideOffset={12} class="z-[100] rounded-md bg-gray-950 px-3 py-2 text-xs text-white shadow-lg">{item.name}</Tooltip.Content></Tooltip.Portal>
							</Tooltip.Root>
						</li>
					{/each}
				</ul>
			</li>
			<li class="-mx-6 mt-auto {responsive ? 'max-[960px]:hidden' : ''}">
				<a
					href="/welcome"
					onclick={(event) => onNavigate?.(event, '/welcome')}
					class="flex items-center gap-x-3 px-6 py-3 text-sm/6 hover:bg-white/5 {focusRing}"
					aria-label={switchLabel}
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
</Tooltip.Provider>
