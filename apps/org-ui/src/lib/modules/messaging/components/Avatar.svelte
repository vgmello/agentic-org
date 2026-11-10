<script lang="ts">
	import { initialsOf } from '$lib/format';
	import { PRESENCE_WORD } from '../model';
	import type { Presence } from '../types';

	/** Agents are squares, people are circles (chat-ux §1). A status dot shows when `presence` is given. */
	let { name, kind, presence, size = 'md' }: { name: string; kind: 'agent' | 'person'; presence?: Presence; size?: 'sm' | 'md' } = $props();

	const dot: Record<Presence, string> = {
		working: 'bg-emerald-400',
		online: 'bg-emerald-400',
		waiting: 'bg-amber-400',
		asleep: 'bg-sky-300/70',
		paused: 'bg-amber-600',
		offline: 'bg-gray-500'
	};
</script>

<span class="relative inline-flex shrink-0 items-center justify-center bg-indigo-500/20 font-semibold text-indigo-200 {kind === 'agent' ? 'rounded-md' : 'rounded-full'} {size === 'sm' ? 'size-6 text-[10px]' : 'size-9 text-xs'}" data-kind={kind} aria-hidden={presence ? undefined : 'true'}>
	{initialsOf(name)}
	{#if presence}
		<span class="absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full ring-2 ring-gray-800 {dot[presence]}" data-testid="presence-dot" data-presence={presence}></span>
		<span class="sr-only">{PRESENCE_WORD[presence]}</span>
	{/if}
</span>
