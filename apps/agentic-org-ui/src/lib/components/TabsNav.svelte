<!--
  @component
  Underline link tabs driven by the URL (Prototype Release 1149b4f1, app-shell-ux §6a):
  each tab is a link using the Inbox primary-tab 2px underline at the divider, top rounded corners,
  `aria-current="page"`, ArrowLeft/ArrowRight/Home/End focus movement, `M` page-menu registration
  (`data-page-menu` + `data-menu-label`), and auto-scrolling the active tab into view on narrow screens.
-->
<script lang="ts" module>
	export interface Tab {
		label: string;
		href: string;
		match?: 'exact' | 'prefix';
		/** A real count shown after the label. */
		count?: number;
	}
</script>

<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { focusDestinationHeading } from '$lib/platform/workspace/focus-destination';
	import { singleKeyShortcutsOn } from '$lib/platform/workspace/shortcuts';

	let {
		tabs,
		label,
		focusHeading = false,
		pageMenu = true,
		class: className = ''
	}: {
		tabs: Tab[];
		label: string;
		focusHeading?: boolean;
		/** Marks this strip as a primary page menu reachable via the `M` shortcut. */
		pageMenu?: boolean;
		class?: string;
	} = $props();

	let navEl = $state<HTMLElement | null>(null);
	let focusedHref = $state<string | null>(null);
	const shortcutsEnabled = $derived(singleKeyShortcutsOn());

	const isCurrent = (tab: Tab) =>
		page.url.pathname === tab.href ||
		(tab.match === 'prefix' && page.url.pathname.startsWith(tab.href + '/'));

	const currentHref = $derived(tabs.find(isCurrent)?.href ?? tabs[0]?.href ?? null);
	// Single sequential Tab stop for the strip (WAI-ARIA roving tabindex), resetting to current route on exit.
	const tabStopHref = $derived(
		focusedHref && tabs.some((t) => t.href === focusedHref) ? focusedHref : currentHref
	);

	async function choose(event: MouseEvent, tab: Tab) {
		if (!focusHeading || event.defaultPrevented || event.button !== 0) return;
		if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
		event.preventDefault();
		if (page.url.pathname !== tab.href) await goto(resolve(tab.href as '/'));
		await focusDestinationHeading();
	}

	function handleKeydown(event: KeyboardEvent, index: number) {
		if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
		if (
			event.key !== 'ArrowLeft' &&
			event.key !== 'ArrowRight' &&
			event.key !== 'Home' &&
			event.key !== 'End'
		) {
			return;
		}
		const links = navEl ? [...navEl.querySelectorAll<HTMLAnchorElement>('a[data-menu-label]')] : [];
		if (links.length === 0) return;
		const nextIdx =
			event.key === 'Home'
				? 0
				: event.key === 'End'
					? links.length - 1
					: (index + (event.key === 'ArrowRight' ? 1 : -1) + links.length) % links.length;
		event.preventDefault();
		const nextTab = tabs[nextIdx];
		if (nextTab) focusedHref = nextTab.href;
		links[nextIdx]?.focus();
		links[nextIdx]?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
	}

	function handleFocusOut(event: FocusEvent) {
		const related = event.relatedTarget;
		if (!navEl || !(related instanceof Node) || !navEl.contains(related)) {
			focusedHref = null;
		}
	}

	// Keep the focused tab (when focus is inside the strip) or the active route tab visible on navigation & resize.
	$effect(() => {
		void page.url.pathname;
		void tabs.length;
		const el = navEl;
		if (!el) return;
		const revealTab = () => {
			if (el.offsetParent === null) return;
			const active = document.activeElement;
			const target =
				active instanceof HTMLElement && el.contains(active)
					? active
					: el.querySelector<HTMLElement>('a[aria-current="page"]');
			if (!target) return;
			const bounds = el.getBoundingClientRect();
			if (bounds.width <= 0) return;
			const { left, right } = target.getBoundingClientRect();
			if (left < bounds.left || right > bounds.right) {
				target.scrollIntoView({ block: 'nearest', inline: 'center' });
			}
		};
		revealTab();
		const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(revealTab) : undefined;
		ro?.observe(el);
		return () => ro?.disconnect();
	});
</script>

<nav
	bind:this={navEl}
	aria-label={label}
	data-page-menu={pageMenu ? '' : undefined}
	aria-keyshortcuts={pageMenu && shortcutsEnabled ? 'M' : undefined}
	onfocusout={handleFocusOut}
	class="tabs-nav flex gap-x-1 overflow-x-auto border-b border-line px-4 min-[640px]:px-6 min-[1024px]:px-8 {className}"
>
	{#each tabs as tab, index (tab.href)}
		{@const current = isCurrent(tab)}
		<a
			href={resolve(tab.href as '/')}
			data-menu-label={tab.label}
			aria-current={current ? 'page' : undefined}
			tabindex={tab.href === tabStopHref ? 0 : -1}
			onfocus={() => (focusedHref = tab.href)}
			onclick={(event) => choose(event, tab)}
			onkeydown={(event) => handleKeydown(event, index)}
			class="tabs-nav-link flex min-h-11 shrink-0 items-center gap-x-1.5 rounded-t-md border-b-2 px-3 pt-1.5 pb-[5px] text-sm/6 font-semibold whitespace-nowrap min-[640px]:min-h-9 [@media(pointer:coarse)]:min-h-11 {current
				? 'is-current'
				: ''}"
		>
			<span>{tab.label}</span>
			{#if tab.count !== undefined}
				<span class="tabs-nav-count tabular-nums">{tab.count}</span>
			{/if}
		</a>
	{/each}
</nav>

<style>
	.tabs-nav {
		scrollbar-width: none;
	}
	.tabs-nav::-webkit-scrollbar {
		display: none;
	}
	.tabs-nav-link {
		border-bottom-color: transparent;
		color: var(--exec-body, var(--fg-muted));
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.tabs-nav-link:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-raised));
		color: var(--exec-ink, var(--fg));
	}
	.tabs-nav-link.is-current {
		border-bottom-color: var(--tone-accent-ink);
		color: var(--exec-ink, var(--fg));
	}
	.tabs-nav-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -4px;
	}
	.tabs-nav-count {
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 600;
	}
	.tabs-nav-link.is-current .tabs-nav-count {
		color: var(--tone-accent-ink);
	}
	@media (prefers-reduced-motion: reduce) {
		.tabs-nav-link {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.tabs-nav-link {
			border-bottom-color: Canvas;
		}
		.tabs-nav-link.is-current {
			border-bottom-color: Highlight;
		}
	}
</style>
