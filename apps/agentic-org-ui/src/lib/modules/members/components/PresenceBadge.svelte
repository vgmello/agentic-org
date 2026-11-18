<!--
  @component
  One presence signal (our-org-in-the-app §5): a shape and a word, never colour alone. Only a reported Working
  dot pulses; reduced motion stops it. Unknown is shown only where the caller asks (an agent's header).
-->
<script lang="ts">
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { PRESENCE, presenceLabel } from '../present';
	import type { Presence } from '../types';

	let { presence, class: className = '' }: { presence: Presence; class?: string } = $props();
	const tone = $derived(PRESENCE[presence.id].tone);
	const pill = {
		amber: 'bg-amber-400/10 text-amber-200 ring-amber-400/30',
		indigo: 'bg-indigo-400/10 text-indigo-300 ring-indigo-400/30',
		green: 'bg-green-400/10 text-green-300 ring-green-400/20',
		gray: 'bg-gray-400/10 text-gray-300 ring-gray-400/20',
		sky: 'bg-sky-400/10 text-sky-300 ring-sky-400/20'
	} as const;
	const dot = {
		amber: 'fill-amber-300',
		indigo: 'fill-indigo-300',
		green: 'fill-green-400',
		gray: 'fill-gray-400',
		sky: 'fill-sky-300'
	} as const;
</script>

<StatusPill label={presenceLabel(presence)} class="{pill[tone]} {className}" data-presence={presence.id}>
	{#snippet icon()}
		{#if presence.id === 'paused'}
			<svg viewBox="0 0 16 16" class="size-4 fill-gray-300" aria-hidden="true"><rect x="4" y="3" width="3" height="10" rx="1" /><rect x="9" y="3" width="3" height="10" rx="1" /></svg>
		{:else if presence.id === 'needs_you'}
			<svg viewBox="0 0 16 16" class="size-4 fill-amber-300" aria-hidden="true"><path fill-rule="evenodd" d="M15 8A7 7 0 1 1 1 8a7 7 0 0 1 14 0ZM8 4a.75.75 0 0 1 .75.75v3.5a.75.75 0 0 1-1.5 0v-3.5A.75.75 0 0 1 8 4Zm0 8a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z" clip-rule="evenodd" /></svg>
		{:else if presence.id === 'waiting_answer'}
			<svg viewBox="0 0 16 16" class="size-4 fill-gray-300" aria-hidden="true"><path fill-rule="evenodd" d="M1 8a7 7 0 1 1 14 0A7 7 0 0 1 1 8Zm7.75-4.25a.75.75 0 0 0-1.5 0V8c0 .414.336.75.75.75h3.25a.75.75 0 0 0 0-1.5h-2.5v-3.5Z" clip-rule="evenodd" /></svg>
		{:else if presence.id === 'unknown'}
			<svg viewBox="0 0 16 16" class="size-4 fill-gray-400" aria-hidden="true"><path fill-rule="evenodd" d="M15 8A7 7 0 1 1 1 8a7 7 0 0 1 14 0Zm-6 3.5a1 1 0 1 1-2 0 1 1 0 0 1 2 0ZM7.293 5.293a1 1 0 1 1 .99 1.667c-.459.134-1.033.566-1.033 1.29v.25a.75.75 0 1 0 1.5 0v-.115a2.5 2.5 0 1 0-2.518-4.153.75.75 0 1 0 1.061 1.06Z" clip-rule="evenodd" /></svg>
		{:else}
			<svg viewBox="0 0 6 6" class="size-1.5 {dot[tone]} {presence.id === 'working' ? 'motion-safe:animate-pulse' : ''}" aria-hidden="true"><circle cx="3" cy="3" r="3" /></svg>
		{/if}
	{/snippet}
</StatusPill>
