<!--
  @component
  One status word in the shared StatusPill (app-shell-ux §6a), with the shared tones (`pills.ts`). As Agent setup's
  HealthBadge does, the pill keeps whole words and wraps between them instead of truncating, with rounded corners so
  a wrapped pill still reads as one tag.
-->
<script lang="ts">
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { tone as tones } from '$lib/components/pills';
	import { cn } from '$lib/utils';
	import type { Tone } from '../../present';

	let { label, tone }: { label: string; tone: Tone } = $props();

	/** StatusPill truncates by default; a status word is never cut (§6a). */
	const wraps =
		'items-start rounded-lg text-left [&>span]:overflow-visible [&>span]:whitespace-normal';
</script>

<StatusPill {label} class={cn(tones[tone], wraps)} data-testid="version-pill" />
