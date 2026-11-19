<!--
  @component
  One status word in a pill (app-shell-ux §6a). Unlike the shared StatusPill it never truncates: the words wrap
  between words, so "Needs update" stays whole at 320 px and 200% text.
-->
<script lang="ts">
	import { tone as tones } from '$lib/components/pills';
	import type { Tone } from '../../present';

	let { label, tone }: { label: string; tone: Tone } = $props();

	const toneClass = $derived(
		tone === 'neutral' ? 'bg-quiet/10 text-fg-secondary ring-quiet/20' : tones[tone]
	);
</script>

<span
	class="inline-flex max-w-full shrink-0 items-center rounded-full px-2 py-1 text-xs/4 font-medium ring-1 ring-inset {toneClass}"
	>{label}</span
>
