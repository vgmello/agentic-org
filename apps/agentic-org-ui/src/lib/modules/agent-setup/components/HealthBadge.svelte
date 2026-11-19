<!--
  @component
  One Health word with its shape (our-org-in-the-app §5; app-shell-ux §6a: a word, a colour and a 16 px icon, never
  colour alone). The shared StatusPill with the shared tones (`pills.ts`): Ready is `ok`, Needs setup is `owner`
  (amber: the owner has something to do), Paused and unknown are the neutral pill with different shapes. As in the
  profile header, the pill keeps whole words and wraps between them instead of truncating, with rounded corners so a
  wrapped pill still reads as one tag.
-->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { tone } from '$lib/components/pills';
	import { cn } from '$lib/utils';
	import { HEALTH_WORD, type Health } from '../health';

	let {
		health,
		class: className = '',
		...attributes
	}: Omit<HTMLAttributes<HTMLSpanElement>, 'children'> & { health: Health } = $props();

	const TONE: Record<Health['kind'], string> = {
		ready: tone.ok,
		needs_setup: tone.owner,
		paused: tone.neutral,
		unknown: tone.neutral
	};
	/** StatusPill truncates by default; a Health word is never cut (§6a). */
	const wraps =
		'items-start rounded-lg text-left [&>span]:overflow-visible [&>span]:whitespace-normal';
</script>

<StatusPill
	label={HEALTH_WORD[health.kind]}
	class={cn(TONE[health.kind], wraps, className)}
	data-health={health.kind}
	{...attributes}
>
	{#snippet icon()}
		<svg viewBox="0 0 16 16" class="size-4 fill-current" aria-hidden="true">
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
	{/snippet}
</StatusPill>
