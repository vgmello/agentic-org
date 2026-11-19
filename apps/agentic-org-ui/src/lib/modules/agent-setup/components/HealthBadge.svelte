<!--
  @component
  One Health word with its shape (our-org-in-the-app §5; app-shell-ux §6a: a word, a colour and a 16 px icon, never
  colour alone). The pill keeps whole words and wraps between them instead of truncating; its corners are rounded
  rather than round so a wrapped pill still reads as one tag, as the profile header's presence pill does. Amber means
  the owner has something to do; Paused and unknown are quiet grey with different shapes.
-->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { HEALTH_WORD, type Health } from '../health';

	let {
		health,
		class: className = '',
		...attributes
	}: Omit<HTMLAttributes<HTMLSpanElement>, 'children'> & { health: Health } = $props();

	const TONE: Record<Health['kind'], string> = {
		ready: 'bg-green-500/10 text-green-300 ring-green-500/20',
		needs_setup: 'bg-amber-400/10 text-amber-200 ring-amber-400/30',
		paused: 'bg-gray-400/10 text-gray-300 ring-gray-400/20',
		unknown: 'bg-gray-400/10 text-gray-300 ring-gray-400/20'
	};
</script>

<span
	class="inline-flex max-w-full items-start gap-x-1.5 rounded-lg px-2 py-1 text-left text-xs/4 font-medium ring-1 ring-inset {TONE[
		health.kind
	]} {className}"
	data-health={health.kind}
	{...attributes}
>
	<svg viewBox="0 0 16 16" class="size-4 shrink-0 fill-current" aria-hidden="true">
		{#if health.kind === 'ready'}
			<path
				fill-rule="evenodd"
				d="M8 15A7 7 0 1 0 8 1a7 7 0 0 0 0 14Zm3.844-8.791a.75.75 0 0 0-1.188-.918l-3.7 4.79-1.649-1.833a.75.75 0 1 0-1.114 1.004l2.25 2.5a.75.75 0 0 0 1.15-.043l4.25-5.5Z"
				clip-rule="evenodd"
			/>
		{:else if health.kind === 'needs_setup'}
			<path
				fill-rule="evenodd"
				d="M6.701 2.25c.577-1 2.02-1 2.598 0l5.196 9a1.5 1.5 0 0 1-1.299 2.25H2.804a1.5 1.5 0 0 1-1.3-2.25l5.197-9ZM8 4a.75.75 0 0 1 .75.75v3a.75.75 0 1 1-1.5 0v-3A.75.75 0 0 1 8 4Zm0 8a1 1 0 1 0 0-2 1 1 0 0 0 0 2Z"
				clip-rule="evenodd"
			/>
		{:else if health.kind === 'paused'}
			<rect x="4" y="3" width="3" height="10" rx="1" /><rect
				x="9"
				y="3"
				width="3"
				height="10"
				rx="1"
			/>
		{:else}
			<path
				fill-rule="evenodd"
				d="M15 8A7 7 0 1 1 1 8a7 7 0 0 1 14 0Zm-6 3.5a1 1 0 1 1-2 0 1 1 0 0 1 2 0ZM7.293 5.293a1 1 0 1 1 .99 1.667c-.459.134-1.033.566-1.033 1.29v.25a.75.75 0 1 0 1.5 0v-.115a2.5 2.5 0 1 0-2.518-4.153.75.75 0 1 0 1.061 1.06Z"
				clip-rule="evenodd"
			/>
		{/if}
	</svg>
	<span class="min-w-0">{HEALTH_WORD[health.kind]}</span>
</span>
