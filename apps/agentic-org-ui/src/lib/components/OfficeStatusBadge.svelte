<script lang="ts">
	import Circle from '@lucide/svelte/icons/circle';
	import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
	import CircleHelp from '@lucide/svelte/icons/circle-help';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import Loader from '@lucide/svelte/icons/loader';
	import Mail from '@lucide/svelte/icons/mail';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import StatusPill from './StatusPill.svelte';
	import { statusIcon, statusInfo, type DisplayStatus } from '$lib/offices/model';

	let { status }: { status: DisplayStatus | undefined } = $props();
	const info = $derived(statusInfo(status));
	const glyph = $derived(statusIcon(status));
	// The word is always shown: neither the icon nor its color alone carries the status.
	const color = { ok: 'text-green-400', info: 'text-sky-400', muted: 'text-gray-400', warn: 'text-amber-400', bad: 'text-red-400' } as const;
</script>

<StatusPill class="min-w-0 max-w-full shrink whitespace-nowrap" label={info.label} data-status-icon={glyph}>
	{#snippet icon()}
		{#if glyph === 'circle'}<Circle class="size-3.5 shrink-0 fill-current {color[info.tone]}" aria-hidden="true" />
		{:else if glyph === 'circle-outline'}<Circle class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
		{:else if glyph === 'loader'}<Loader class="size-3.5 shrink-0 animate-spin motion-reduce:animate-none {color[info.tone]}" aria-hidden="true" />
		{:else if glyph === 'circle-x'}<CircleX class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
		{:else if glyph === 'circle-arrow-up'}<CircleArrowUp class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
		{:else if glyph === 'wifi-off'}<WifiOff class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
		{:else if glyph === 'mail'}<Mail class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
		{:else}<CircleHelp class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />{/if}
	{/snippet}
</StatusPill>
