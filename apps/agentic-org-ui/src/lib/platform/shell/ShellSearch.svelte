<!--
  @component
  Top-bar Search (app-shell-ux §0.2): a combobox whose results are grouped Projects, Work, Members, chosen
  with the arrow keys and Enter; the last option is "See all results", which opens /search?q=. Below
  640 px it is a 44 px icon button that opens a full-width field over the top bar. `focusSearch()` is
  what the `/` shortcut calls.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { tick } from 'svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import { shellApi } from '$lib/modules/shell/api';
	import type { SearchGroup, SearchHit, SearchResults } from '$lib/modules/shell/types';
	import { focusDestinationHeading } from './focus-destination';

	let { organizationId, shortcutHint = false }: { organizationId: string; shortcutHint?: boolean } =
		$props();

	const GROUPS: { key: SearchGroup; label: string }[] = [
		{ key: 'projects', label: 'Projects' },
		{ key: 'work', label: 'Work' },
		{ key: 'members', label: 'Members' }
	];
	const id = $props.id();

	let query = $state('');
	let open = $state(false);
	let narrowOpen = $state(false);
	let phase = $state<'idle' | 'searching' | 'done' | 'failed'>('idle');
	let results = $state.raw<SearchResults | undefined>();
	let active = $state(-1);
	let input = $state<HTMLInputElement | null>(null);
	let opener = $state<HTMLButtonElement | null>(null);
	let container = $state<HTMLDivElement | null>(null);

	const trimmed = $derived(query.trim());
	type Option = { id: string; href: string; hit?: SearchHit };
	const seeAllHref = $derived(`/search?${new URLSearchParams({ q: trimmed })}`);
	const options = $derived.by<Option[]>(() => {
		const list: Option[] = [];
		for (const group of GROUPS)
			for (const hit of results?.[group.key] ?? [])
				list.push({ id: `${id}-${group.key}-${hit.id}`, href: hit.href, hit });
		if (trimmed) list.push({ id: `${id}-all`, href: seeAllHref });
		return list;
	});
	const total = $derived(
		GROUPS.reduce((sum, group) => sum + (results?.[group.key].length ?? 0), 0)
	);
	const expanded = $derived(open && trimmed !== '');

	let timer: ReturnType<typeof setTimeout> | undefined;
	let request = 0;
	function changed() {
		open = true;
		active = -1;
		clearTimeout(timer);
		if (!trimmed) {
			results = undefined;
			phase = 'idle';
			return;
		}
		phase = 'searching';
		const asked = trimmed;
		const mine = ++request;
		timer = setTimeout(async () => {
			try {
				const found = await shellApi.search(organizationId, asked);
				if (mine === request) {
					results = found;
					phase = 'done';
				}
			} catch {
				if (mine === request) {
					results = undefined;
					phase = 'failed';
				}
			}
		}, 150);
	}

	async function choose(href: string) {
		open = false;
		narrowOpen = false;
		query = '';
		results = undefined;
		phase = 'idle';
		await goto(resolve(href as '/'));
		await focusDestinationHeading();
	}

	function keydown(event: KeyboardEvent) {
		if (event.isComposing) return;
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			if (!options.length) return;
			event.preventDefault();
			open = true;
			const step = event.key === 'ArrowDown' ? 1 : -1;
			active =
				active < 0
					? step > 0
						? 0
						: options.length - 1
					: (active + step + options.length) % options.length;
			document.getElementById(options[active].id)?.scrollIntoView({ block: 'nearest' });
		} else if (event.key === 'Enter') {
			if (!trimmed) return;
			event.preventDefault();
			void choose(active >= 0 && options[active] ? options[active].href : seeAllHref);
		} else if (event.key === 'Escape') {
			event.preventDefault();
			if (expanded) {
				open = false;
				active = -1;
			} else if (narrowOpen) closeNarrow();
			else if (query) query = '';
		}
	}

	function focusout(event: FocusEvent) {
		if (event.relatedTarget instanceof Node && container?.contains(event.relatedTarget)) return;
		open = false;
		active = -1;
		if (
			narrowOpen &&
			!(event.relatedTarget instanceof Node && container?.contains(event.relatedTarget))
		)
			narrowOpen = false;
	}

	async function openNarrow() {
		narrowOpen = true;
		await tick();
		input?.focus();
	}
	function closeNarrow() {
		narrowOpen = false;
		open = false;
		opener?.focus();
	}

	/** Focuses the field (opening it first below 640 px). The `/` shortcut calls this. */
	export async function focusSearch() {
		if (opener && opener.offsetParent !== null) await openNarrow();
		else input?.focus();
	}
</script>

<button
	bind:this={opener}
	type="button"
	aria-label="Search"
	aria-keyshortcuts={shortcutHint ? '/' : undefined}
	onclick={openNarrow}
	class="inline-flex size-11 shrink-0 items-center justify-center rounded-md text-gray-400 hover:bg-white/5 hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 min-[640px]:hidden"
>
	<MiniIcon name="search" class="size-5" />
</button>

<div
	bind:this={container}
	onfocusout={focusout}
	class="relative min-w-0 flex-1 min-[640px]:max-w-md {narrowOpen
		? 'max-[639px]:fixed max-[639px]:inset-x-0 max-[639px]:top-0 max-[639px]:z-50 max-[639px]:flex max-[639px]:h-14 max-[639px]:items-center max-[639px]:gap-x-2 max-[639px]:border-b max-[639px]:border-white/5 max-[639px]:bg-gray-800 max-[639px]:px-4'
		: 'max-[639px]:hidden'}"
>
	<form role="search" class="relative min-w-0 flex-1" onsubmit={(event) => event.preventDefault()}>
		<label for="{id}-input" class="sr-only">Search</label>
		<MiniIcon
			name="search"
			class="pointer-events-none absolute top-1/2 left-2.5 size-5 -translate-y-1/2 text-gray-400"
		/>
		<input
			bind:this={input}
			bind:value={query}
			id="{id}-input"
			type="search"
			role="combobox"
			autocomplete="off"
			spellcheck="false"
			placeholder="Search"
			aria-autocomplete="list"
			aria-expanded={expanded}
			aria-controls="{id}-list"
			aria-activedescendant={expanded && active >= 0 ? options[active]?.id : undefined}
			aria-keyshortcuts={shortcutHint ? '/' : undefined}
			oninput={changed}
			onfocus={() => (open = true)}
			onkeydown={keydown}
			class="block h-9 w-full rounded-md bg-white/5 pr-9 pl-9 text-base text-white outline-1 -outline-offset-1 outline-gray-500 placeholder:text-gray-400 focus:outline-2 focus:-outline-offset-2 focus:outline-indigo-400 min-[640px]:text-sm/6 max-[639px]:h-11 [&::-webkit-search-cancel-button]:hidden"
		/>
		{#if shortcutHint && !query}
			<kbd
				aria-hidden="true"
				class="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 rounded border border-white/10 px-1.5 font-sans text-xs/5 text-gray-400 max-[639px]:hidden"
				>/</kbd
			>
		{/if}
	</form>
	{#if narrowOpen}
		<button
			type="button"
			aria-label="Close search"
			onclick={closeNarrow}
			class="inline-flex size-11 shrink-0 items-center justify-center rounded-md text-gray-400 hover:bg-white/5 hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 min-[640px]:hidden"
		>
			<MiniIcon name="xMark" class="size-5" />
		</button>
	{/if}

	<div
		hidden={!expanded}
		class="absolute top-full right-0 left-0 z-50 mt-2 max-h-[min(28rem,calc(100dvh-5rem))] overflow-y-auto rounded-md bg-gray-800 py-2 ring-1 ring-white/10 max-[639px]:fixed max-[639px]:top-14 max-[639px]:mt-0 max-[639px]:rounded-none"
	>
		{#if phase === 'searching' && !results}
			<p class="px-3 py-1.5 text-sm/6 text-gray-400">Searching…</p>
		{:else if phase === 'done' && total === 0}
			<p class="px-3 py-1.5 text-sm/6 text-gray-400">
				No projects, work or members match “{trimmed}”.
			</p>
		{:else if phase === 'failed'}
			<p class="px-3 py-1.5 text-sm/6 text-gray-300">
				Search isn't available right now. Try again in a moment, or open all results.
			</p>
		{/if}
		<div id="{id}-list" role="listbox" aria-label="Search results">
			{#each GROUPS as group (group.key)}
				{@const hits = results?.[group.key] ?? []}
				{#if hits.length}
					<div role="group" aria-labelledby="{id}-{group.key}-label">
						<div
							id="{id}-{group.key}-label"
							class="px-3 pt-2 pb-1 text-xs/5 font-semibold text-gray-400"
						>
							{group.label}
						</div>
						{#each hits as hit (hit.id)}
							{@const optionId = `${id}-${group.key}-${hit.id}`}
							{@const selected = options[active]?.id === optionId}
							<div
								id={optionId}
								role="option"
								tabindex="-1"
								aria-selected={selected}
								onmousedown={(event) => event.preventDefault()}
								onclick={() => choose(hit.href)}
								onkeydown={() => {}}
								class="mx-1 flex cursor-default flex-col rounded-sm px-2 py-1.5 text-sm/6 max-[639px]:min-h-11 {selected
									? 'bg-white/10 text-white'
									: 'text-gray-200 hover:bg-white/5'}"
							>
								<span class="min-w-0 truncate">{hit.title}</span>
								{#if hit.meta}<span class="text-xs/5 text-gray-400">{hit.meta}</span>{/if}
							</div>
						{/each}
					</div>
				{/if}
			{/each}
			{#if trimmed}
				{@const selected = options[active]?.id === `${id}-all`}
				<div class="mt-1 border-t border-white/5 pt-1">
					<div
						id="{id}-all"
						role="option"
						tabindex="-1"
						aria-selected={selected}
						onmousedown={(event) => event.preventDefault()}
						onclick={() => choose(seeAllHref)}
						onkeydown={() => {}}
						class="mx-1 flex min-h-9 cursor-default items-center rounded-sm px-2 text-sm/6 font-semibold text-indigo-300 max-[639px]:min-h-11 {selected
							? 'bg-white/10'
							: 'hover:bg-white/5'}"
					>
						See all results
					</div>
				</div>
			{/if}
		</div>
	</div>
	<p class="sr-only" role="status">
		{#if expanded && phase === 'done'}{total === 0
				? `No matches for ${trimmed}`
				: `${total} ${total === 1 ? 'result' : 'results'}`}
		{:else if expanded && phase === 'failed'}Search isn't available right now{/if}
	</p>
</div>
