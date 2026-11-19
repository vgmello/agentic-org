<!--
  @component
  Underline link tabs driven by the URL (app-shell-ux §6a): each tab is a link, the current one has
  `aria-current="page"` and an indigo underline as well as white text. For in-place filters that do not
  change the URL, use buttons with `aria-selected` instead, not this.

  `match: 'prefix'` keeps a tab current on the pages under it (a project's Work tab on /projects/p/work/…).
  `focusHeading` moves focus to the destination heading after the move (a view switch, §0).
  The strip scrolls inside itself when the tabs do not fit; the page never scrolls sideways.
  Usage: <TabsNav label="Teams views" tabs={[{ label: 'Teams', href: '/teams' }, …]} focusHeading />
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
	import { focusDestinationHeading } from '$lib/platform/shell/focus-destination';

	let {
		tabs,
		label,
		focusHeading = false,
		class: className = ''
	}: { tabs: Tab[]; label: string; focusHeading?: boolean; class?: string } = $props();

	const isCurrent = (tab: Tab) =>
		page.url.pathname === tab.href ||
		(tab.match === 'prefix' && page.url.pathname.startsWith(tab.href + '/'));

	async function choose(event: MouseEvent, tab: Tab) {
		if (!focusHeading || event.defaultPrevented || event.button !== 0) return;
		if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
		event.preventDefault();
		if (page.url.pathname !== tab.href) await goto(resolve(tab.href as '/'));
		await focusDestinationHeading();
	}
</script>

<nav
	aria-label={label}
	class="flex gap-x-6 overflow-x-auto border-b border-white/10 px-4 min-[640px]:px-6 min-[1024px]:px-8 {className}"
>
	{#each tabs as tab (tab.href)}
		{@const current = isCurrent(tab)}
		<a
			href={resolve(tab.href as '/')}
			aria-current={current ? 'page' : undefined}
			onclick={(event) => choose(event, tab)}
			class="-mb-px flex min-h-11 items-center gap-x-2 border-b-2 py-3 text-sm/6 font-semibold whitespace-nowrap focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 {current
				? 'border-indigo-400 text-white'
				: 'border-transparent text-gray-400 hover:border-white/20 hover:text-white'}"
		>
			{tab.label}
			{#if tab.count !== undefined}
				<span class="rounded-full bg-white/10 px-2 text-xs/5 font-medium text-gray-300"
					>{tab.count}</span
				>
			{/if}
		</a>
	{/each}
</nav>
