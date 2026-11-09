<script lang="ts">
	import { getContext } from 'svelte';
	import Bot from '@lucide/svelte/icons/bot';
	import Pause from '@lucide/svelte/icons/pause';
	import { initialsOf } from '$lib/format';
	import { arrived } from '$lib/import/arrived.svelte';
	import type { Member, Team } from '$lib/proto';

	let {
		member,
		size = 'md',
		selected = false,
		dimmed = false,
		highlighted = false,
		highlightLabel = 'here',
		alsoIn = [],
		onselect
	}: {
		member: Member;
		size?: 'sm' | 'md';
		selected?: boolean;
		dimmed?: boolean;
		/** Ring and tag from the office filter, and what the tag says (the office's name). */
		highlighted?: boolean;
		highlightLabel?: string;
		/** The member's other teams: shown as a "+N teams" badge. */
		alsoIn?: Team[];
		onselect?: (id: string) => void;
	} = $props();

	// The search (TEAM-11) is shared by every tile on the page: a match is ringed, everyone else is dimmed.
	const search = getContext<{ readonly matches: ReadonlySet<string> | undefined } | undefined>('member-search');
	const searchHit = $derived(search?.matches?.has(member.id) === true);
	const searchMiss = $derived(!!search?.matches && !search.matches.has(member.id));
	const agent = $derived(member.kind === 'agent');
	// Just brought in by an import: ringed and tagged "New" for a while, unless the office filter ringed it already.
	const isNew = $derived(arrived.has(member.id));
	const ringed = $derived(highlighted || isNew);
	const tag = $derived(highlighted ? highlightLabel : 'New');
	// Agents and people are told apart by shape and glyph, not color: a square tile with a robot, or
	// a round avatar; both show initials; screen readers hear "agent" or "person" after the name.
	const label = $derived(
		`${member.name}, ${agent ? 'agent' : member.kind === 'person' ? 'person' : 'member'}` +
			(member.agent?.status === 'paused' ? ', paused' : '') +
			(alsoIn.length ? `, also in ${alsoIn.map((t) => t.name).join(', ')}` : '') +
			(ringed ? `, highlighted: ${tag}` : '') +
			(searchHit ? ', matches the search' : '')
	);
	const box = $derived(size === 'sm' ? 'size-8' : 'size-10');
</script>

<span class="inline-flex shrink-0 items-center gap-1">
<button
	type="button"
	data-member-tile
	title={label}
	aria-label={label}
	aria-pressed={selected}
	onclick={() => onselect?.(member.id)}
	class="relative inline-flex shrink-0 items-center justify-center text-xs font-semibold outline-1 -outline-offset-1 focus-visible:outline-2 focus-visible:outline-indigo-400 {box}
		{agent ? 'rounded-md bg-indigo-500/20 text-indigo-200 outline-indigo-400/30' : 'rounded-full bg-emerald-500/15 text-emerald-200 outline-emerald-400/30'}
		{selected ? 'ring-2 ring-white' : ringed ? 'ring-2 ring-amber-300' : searchHit ? 'ring-2 ring-sky-300' : ''} {dimmed || searchMiss ? 'opacity-40' : member.agent?.status === 'paused' ? 'opacity-60' : ''}"
>
	<span aria-hidden="true">{initialsOf(member.name)}</span>
	{#if agent}
		<!-- A square tile with a small robot: told apart from a person by shape and glyph, not color or initials alone. -->
		<Bot class="absolute top-0 left-0.5 size-2.5 text-indigo-200" aria-hidden="true" />
	{/if}
	{#if member.agent?.status === 'paused'}
		<Pause class="absolute -right-1 -bottom-1 size-3.5 rounded-full bg-gray-700 p-0.5 text-amber-200" aria-hidden="true" data-testid="paused-mark" />
	{/if}
	{#if ringed}
		<span class="absolute -bottom-2 left-1/2 max-w-16 -translate-x-1/2 truncate rounded bg-amber-300 px-1 text-[0.55rem] leading-3 font-bold text-gray-900" aria-hidden="true">{tag}</span>
	{/if}
</button>
	<!-- Beside the tile, in the flow: it never covers the next tile. The tile's label already says "also in …". -->
	{#if alsoIn.length > 0}
		<span class="rounded-sm bg-gray-700 px-1 text-[0.6rem] leading-4 font-semibold whitespace-nowrap text-white outline-1 outline-white/20" aria-hidden="true">
			+{alsoIn.length} {alsoIn.length === 1 ? 'team' : 'teams'}
		</span>
	{/if}
</span>
