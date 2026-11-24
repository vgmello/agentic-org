<script lang="ts">
	import Circle from '@lucide/svelte/icons/circle';
	import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
	import CircleHelp from '@lucide/svelte/icons/circle-help';
	import CircleX from '@lucide/svelte/icons/circle-x';
	import Loader from '@lucide/svelte/icons/loader';
	import Mail from '@lucide/svelte/icons/mail';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { statusIcon, statusInfo, type DisplayStatus } from '$lib/modules/offices/model';

	let { status }: { status: DisplayStatus | undefined } = $props();
	const info = $derived(statusInfo(status));
	const glyph = $derived(statusIcon(status));
	// The word is always shown: neither the icon nor its color alone carries the status.
	const pill = {
		ok: 't-green',
		info: 'office-pill-info',
		muted: '',
		warn: 't-amber',
		bad: 't-red'
	} as const;
	const color = {
		ok: 'text-success',
		info: 'text-info',
		muted: 'text-fg-muted',
		warn: 'text-warning',
		bad: 'text-danger'
	} as const;
</script>

<StatusPill
	class="min-w-0 max-w-full shrink whitespace-nowrap {pill[info.tone]}"
	label={info.label}
	data-status-icon={glyph}
>
	{#snippet icon()}
		{#if glyph === 'circle'}<Circle
				class="size-3.5 shrink-0 fill-current {color[info.tone]}"
				aria-hidden="true"
			/>
		{:else if glyph === 'circle-outline'}<Circle
				class="size-3.5 shrink-0 {color[info.tone]}"
				aria-hidden="true"
			/>
		{:else if glyph === 'loader'}<Loader
				class="size-3.5 shrink-0 animate-spin motion-reduce:animate-none {color[info.tone]}"
				aria-hidden="true"
			/>
		{:else if glyph === 'circle-x'}<CircleX
				class="size-3.5 shrink-0 {color[info.tone]}"
				aria-hidden="true"
			/>
		{:else if glyph === 'circle-arrow-up'}<CircleArrowUp
				class="size-3.5 shrink-0 {color[info.tone]}"
				aria-hidden="true"
			/>
		{:else if glyph === 'wifi-off'}<WifiOff
				class="size-3.5 shrink-0 {color[info.tone]}"
				aria-hidden="true"
			/>
		{:else if glyph === 'mail'}<Mail
				class="size-3.5 shrink-0 {color[info.tone]}"
				aria-hidden="true"
			/>
		{:else}<CircleHelp class="size-3.5 shrink-0 {color[info.tone]}" aria-hidden="true" />{/if}
	{/snippet}
</StatusPill>

<style>
	:global(.office-pill-info) {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	:global(.t-amber) :global(svg.text-warning) {
		color: var(--tone-warn-ink);
	}
</style>
