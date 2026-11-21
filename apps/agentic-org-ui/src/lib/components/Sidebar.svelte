<!--
  @component
  The main navigation (app-shell-ux §0, §1): the full sidebar from 960 px, a 64 px icon rail from 640 to
  959 px, and the same content inside the drawer below 640 px. Projects and Teams list up to five places
  and View all; under vertical pressure Teams' list collapses first, then Projects', before the list
  scrolls. The footer holds the account menu and the office line, and stays in view.
  Ported from the Tailwind Plus sidebar layout (licensed; allowed inside apps/agentic-org-ui only).
-->
<script lang="ts">
	import { onDestroy, tick, untrack } from 'svelte';
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { innerHeight, innerWidth } from 'svelte/reactivity/window';
	import { Tooltip, mergeProps } from 'bits-ui';
	import type { Connection } from '$lib/proto';
	import type { ShellNavigation } from '$lib/modules/shell/types';
	import ShellAccountMenu from '$lib/platform/shell/ShellAccountMenu.svelte';
	import Logo from './Logo.svelte';
	import { icons } from './icons';
	import { navFor, navItems, sublistLinks, type NavItem } from '$lib/nav';

	let {
		connection,
		navigation,
		officeStatus,
		presentation = 'responsive',
		onNavigate
	}: {
		connection: Connection;
		/** Inbox count and the Projects/Teams lists; undefined while unknown or not served. */
		navigation?: ShellNavigation;
		/** The connected office's status word and tone, for the footer line. */
		officeStatus?: { label: string; tone: 'ok' | 'info' | 'muted' | 'warn' | 'bad' };
		presentation?: 'responsive' | 'drawer';
		onNavigate?: (event: MouseEvent, href: string) => void;
	} = $props();

	const sidebarId = $props.id();
	const items = $derived(navItems(navigation));
	const current = $derived(navFor(page.url.pathname));
	const responsive = $derived(presentation === 'responsive');
	let root = $state<HTMLDivElement | null>(null);
	let observedRail = $state<boolean | undefined>(undefined);
	$effect(() => {
		if (!responsive || !root) {
			observedRail = undefined;
			return;
		}
		const el = root;
		const measure = () => {
			const width = el.getBoundingClientRect().width;
			const rootFont = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
			observedRail = width > 0 && width < 10 * rootFont;
		};
		measure();
		const observer = new ResizeObserver(measure);
		observer.observe(el);
		return () => observer.disconnect();
	});
	const rail = $derived(
		responsive &&
			(observedRail ?? ((innerWidth.current ?? 960) >= 640 && (innerWidth.current ?? 960) < 960))
	);
	const homeLabel = $derived(`${connection.organization.name} home`);

	const sublists = $derived({
		projects: sublistLinks(navigation?.projects ?? [], '/projects'),
		teams: sublistLinks(navigation?.teams ?? [], '/teams')
	});
	const within = (pathname: string, href: string) =>
		pathname === href || pathname.startsWith(href + '/');
	const currentSub = $derived(
		[...sublists.projects, ...sublists.teams].find((link) => within(page.url.pathname, link.href))
	);

	// Vertical pressure: 0 shows both lists, 1 hides Teams' list, 2 hides Projects' list too.
	let scroller = $state<HTMLElement | null>(null);
	let collapsed = $state(0);
	async function fit() {
		if (!scroller) return;
		collapsed = 0;
		await tick();
		while (collapsed < 2 && scroller.scrollHeight > scroller.clientHeight + 1) {
			collapsed += 1;
			await tick();
		}
	}
	$effect(() => {
		void innerHeight.current;
		void innerWidth.current;
		void sublists;
		void scroller;
		untrack(() => void fit());
	});
	const showSublist = (item: NavItem) =>
		!rail &&
		!!item.sublist &&
		sublists[item.sublist].length > 0 &&
		(item.sublist === 'teams' ? collapsed < 1 : collapsed < 2);

	const statusDot = {
		ok: 'bg-success',
		info: 'bg-info',
		muted: 'bg-quiet',
		warn: 'bg-warning',
		bad: 'bg-danger'
	} as const;
	const focusRing =
		'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus';
	const itemClass = (active: boolean) =>
		`group flex items-center gap-x-3 rounded-md p-2 text-sm/6 font-semibold ${active ? 'bg-surface-raised text-fg' : 'text-fg-muted hover:bg-surface-raised hover:text-fg'}`;
	const subClass = (active: boolean) =>
		`flex min-h-8 items-center rounded-md py-1 pr-2 pl-11 text-sm/6 ${active ? 'bg-surface-raised font-semibold text-fg' : 'text-fg-muted hover:bg-surface-raised hover:text-fg'} max-[639px]:min-h-11`;

	// While focus scrolls the rail, the link that slides under a resting pointer must not open its tooltip and take
	// over from the focused link's: pointer tooltips other than the focused one wait until the scroll has settled.
	let focusScroll = $state<{ href: string; timer: ReturnType<typeof setTimeout> } | undefined>();
	const pointerHeld = (href: string) => !!focusScroll && focusScroll.href !== href;
	function holdPointerTooltips(href: string) {
		if (focusScroll) clearTimeout(focusScroll.timer);
		focusScroll = { href, timer: setTimeout(() => (focusScroll = undefined), 400) };
	}
	onDestroy(() => {
		if (focusScroll) clearTimeout(focusScroll.timer);
	});

	function revealRailLink(event: FocusEvent) {
		if (!rail || !(event.currentTarget instanceof HTMLElement)) return;
		holdPointerTooltips(event.currentTarget.getAttribute('href') ?? '');
		// Scroll before Bits UI opens the tooltip. WebKit can otherwise deliver the
		// focus scroll after the provider's frame guard and dismiss the tooltip.
		event.currentTarget.scrollIntoView({
			block: 'nearest',
			inline: 'nearest',
			behavior: 'instant'
		});
	}
</script>

<Tooltip.Provider delayDuration={150}>
	<div
		bind:this={root}
		class="@container flex min-h-0 min-w-0 grow flex-col bg-shade/10 ring-1 ring-line-subtle"
	>
		<!-- Application identity: the logo and organization lead Home. -->
		<div
			class="flex h-16 shrink-0 items-center px-4 {responsive
				? '@max-[10rem]:justify-center @max-[10rem]:px-2'
				: ''}"
		>
			<Tooltip.Root disabled={!rail || pointerHeld(resolve('/home'))}>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<a
							{...mergeProps({ onfocus: revealRailLink }, props, {
								onclick: (event: MouseEvent) => onNavigate?.(event, '/home')
							})}
							type={undefined}
							href={resolve('/home')}
							aria-label={homeLabel}
							class="flex min-h-11 min-w-0 items-center gap-x-3 rounded-md px-2 hover:bg-surface-raised {focusRing} {responsive
								? '@max-[10rem]:size-12 @max-[10rem]:justify-center @max-[10rem]:px-0'
								: ''}"
						>
							<Logo class="size-8 shrink-0" />
							<span
								class="truncate text-base/7 font-semibold text-fg {responsive
									? '@max-[10rem]:sr-only'
									: ''}"
								title={connection.organization.name}>{connection.organization.name}</span
							>
						</a>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal
					><Tooltip.Content
						role="tooltip"
						side="right"
						sideOffset={12}
						class="z-[100] max-w-xs rounded-md bg-tooltip px-3 py-2 text-xs text-fg wrap-break-word"
						>{homeLabel}</Tooltip.Content
					></Tooltip.Portal
				>
			</Tooltip.Root>
		</div>

		<nav
			bind:this={scroller}
			aria-label="Main"
			class="min-h-0 flex-1 overflow-y-auto px-4 py-2 {responsive ? '@max-[10rem]:px-2' : ''}"
		>
			<ul role="list" class="space-y-1">
				{#each items as item, index (item.href)}
					{@const active = current?.href === item.href}
					{@const childCurrent =
						active && !!currentSub && !!item.sublist && sublists[item.sublist].includes(currentSub)}
					{@const badgeId = `${sidebarId}-badge-${index}`}
					<li>
						<Tooltip.Root disabled={!rail || pointerHeld(resolve(item.href as '/'))}>
							<Tooltip.Trigger>
								{#snippet child({ props })}
									{@const tooltipDescription =
										typeof props['aria-describedby'] === 'string'
											? props['aria-describedby']
											: undefined}
									<a
										{...mergeProps({ onfocus: revealRailLink }, props, {
											onclick: (event: MouseEvent) => onNavigate?.(event, item.href)
										})}
										type={undefined}
										href={resolve(item.href as '/')}
										aria-label={item.name}
										aria-describedby={item.badge
											? [badgeId, tooltipDescription].filter(Boolean).join(' ')
											: tooltipDescription}
										aria-current={active ? (childCurrent ? 'true' : 'page') : undefined}
										class="relative {itemClass(active)} {focusRing} {responsive
											? '@max-[10rem]:size-12 @max-[10rem]:justify-center @max-[10rem]:gap-x-0'
											: ''} max-[639px]:min-h-11"
									>
										<svg
											viewBox="0 0 24 24"
											fill="none"
											stroke="currentColor"
											stroke-width="1.5"
											aria-hidden="true"
											class="size-6 shrink-0 {active
												? 'text-fg'
												: 'text-fg-muted group-hover:text-fg'}"
										>
											<path d={icons[item.icon]} stroke-linecap="round" stroke-linejoin="round" />
										</svg>
										<span class="min-w-0 wrap-break-word {responsive ? '@max-[10rem]:sr-only' : ''}"
											>{item.name}</span
										>
										{#if item.badge}
											<span id={badgeId} hidden>{item.badge.label}</span>
											<span
												aria-hidden="true"
												class="ml-auto shrink-0 rounded-full bg-warning-text px-1.5 text-xs/5 font-semibold text-on-warning-strong {responsive
													? '@max-[10rem]:absolute @max-[10rem]:top-0.5 @max-[10rem]:right-0.5 @max-[10rem]:text-xs/4'
													: ''}">{item.badge.text}</span
											>
										{/if}
									</a>
								{/snippet}
							</Tooltip.Trigger>
							<Tooltip.Portal
								><Tooltip.Content
									role="tooltip"
									side="right"
									sideOffset={12}
									class="z-[100] rounded-md bg-tooltip px-3 py-2 text-xs text-fg"
									>{item.name}</Tooltip.Content
								></Tooltip.Portal
							>
						</Tooltip.Root>
						{#if item.sublist && showSublist(item)}
							<ul role="list" aria-label={item.name} class="mt-1 space-y-1">
								{#each sublists[item.sublist] as sub (sub.href)}
									<li>
										<a
											href={resolve(sub.href as '/')}
											onclick={(event) => onNavigate?.(event, sub.href)}
											aria-current={currentSub === sub ? 'page' : undefined}
											class="{subClass(currentSub === sub)} {focusRing}"
											><span class="min-w-0 truncate" title={sub.name}>{sub.name}</span></a
										>
									</li>
								{/each}
								<li>
									<a
										href={resolve(item.href as '/')}
										onclick={(event) => onNavigate?.(event, item.href)}
										class="{subClass(false)} {focusRing}"
										>View all<span class="sr-only">&nbsp;{item.name.toLowerCase()}</span></a
									>
								</li>
							</ul>
						{/if}
					</li>
				{/each}
			</ul>
		</nav>

		<div
			class="shrink-0 space-y-1 border-t border-line-subtle px-4 py-3 {responsive
				? '@max-[10rem]:flex @max-[10rem]:justify-center @max-[10rem]:px-2'
				: ''}"
		>
			<ShellAccountMenu {connection} compact={rail} />
			<a
				href={resolve('/settings/offices')}
				onclick={(event) => onNavigate?.(event, '/settings/offices')}
				class="flex min-h-9 items-center gap-x-2 rounded-md px-2 py-1 text-xs/5 text-fg-muted hover:bg-surface-raised hover:text-fg {focusRing} {responsive
					? '@max-[10rem]:hidden'
					: ''} max-[639px]:min-h-11"
			>
				<span
					aria-hidden="true"
					class="size-2 shrink-0 rounded-full {statusDot[officeStatus?.tone ?? 'muted']}"
				></span>
				<span class="min-w-0 wrap-break-word"
					>{connection.office.name} office · {officeStatus?.label ?? 'Checking'}</span
				>
			</a>
		</div>
	</div>
</Tooltip.Provider>
