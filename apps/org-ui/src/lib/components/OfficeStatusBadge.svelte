<script lang="ts">
	import Circle from '@lucide/svelte/icons/circle';
	import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
	import CircleHelp from '@lucide/svelte/icons/circle-help';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import Loader from '@lucide/svelte/icons/loader';
	import Mail from '@lucide/svelte/icons/mail';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import { statusIcon, statusInfo, type DisplayStatus } from '$lib/offices/model';

	let { status }: { status: DisplayStatus | undefined } = $props();
	const info = $derived(statusInfo(status));
	const icon = $derived(statusIcon(status));
	// The word is always shown: neither the icon nor its color alone carries the status.
	const color = { ok: 'text-green-400', info: 'text-sky-400', muted: 'text-gray-400', warn: 'text-amber-400', bad: 'text-red-400' } as const;
</script>

<span class="inline-flex items-center gap-x-1.5 text-xs/5 text-gray-200" data-status-icon={icon}>
	{#if icon === 'circle'}<Circle class="size-3.5 shrink-0 fill-current {color[info.tone]}" aria-hidden="true" />
	{:else if icon === 'circle-outline'}<Circle class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
	{:else if icon === 'loader'}<Loader class="size-3.5 shrink-0 animate-spin motion-reduce:animate-none {color[info.tone]}" aria-hidden="true" />
	{:else if icon === 'circle-x'}<CircleX class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
	{:else if icon === 'circle-arrow-up'}<CircleArrowUp class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
	{:else if icon === 'wifi-off'}<WifiOff class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
	{:else if icon === 'mail'}<Mail class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />
	{:else}<CircleHelp class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />{/if}
	{info.label}
</span>
