<!--
  @component
  A status pill whose words wrap instead of being cut (app-shell-ux §6a: pills keep whole words at 320 px and 200% text).
  Same look as the shared StatusPill; the tone classes come from `$lib/components/pills`.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/utils';

	let {
		label,
		icon,
		class: className = '',
		testid
	}: { label: string; icon?: Snippet; class?: string; testid?: string } = $props();
</script>

<span
	class={cn(
		'inline-flex max-w-full flex-none items-center gap-x-1.5 rounded-full bg-quiet/10 px-2 py-1 text-xs/4 font-medium text-fg-secondary ring-1 ring-quiet/20 ring-inset',
		className
	)}
	data-testid={testid}
>
	{#if icon}<span class="shrink-0" aria-hidden="true">{@render icon()}</span>{/if}
	<span class="min-w-0 text-pretty">{label}</span>
</span>
