<script lang="ts">
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import Clock from '@lucide/svelte/icons/clock';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { computeSnoozePresets, snoozeExceedsExpiry } from './snooze-presets';
	import type { TriageItem } from './triage-model';

	let {
		item,
		triggerLabel,
		menuHeading,
		preserveNote,
		testId,
		controlsDisabled,
		zone,
		nowMs,
		isAskReplyCutoff,
		onDefer,
		onOpenCustomSnooze
	}: {
		item: TriageItem;
		triggerLabel: string;
		menuHeading: string;
		preserveNote: boolean;
		testId?: string;
		controlsDisabled: boolean;
		zone: string;
		nowMs: number;
		isAskReplyCutoff: boolean;
		onDefer: (item: TriageItem, untilIso: string, note?: string) => void;
		onOpenCustomSnooze: (item: TriageItem, from: HTMLElement | null) => void;
	} = $props();

	let snoozeBtnEl = $state<HTMLButtonElement | null>(null);
	let presetNowMs = $state(Date.now());
	const snoozePresets = $derived(
		computeSnoozePresets(new Date(Math.max(nowMs, presetNowMs)), zone)
	);

	const btnTouch =
		'[@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11';
	const menuItemTouch = 'min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11';
</script>

<DropdownMenu.Root
	onOpenChange={(open) => {
		if (open) presetNowMs = Date.now();
	}}
>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			<button
				{...props}
				bind:this={snoozeBtnEl}
				type="button"
				disabled={controlsDisabled}
				data-testid={testId}
				class="triage-secondary-btn inline-flex min-h-9 min-w-9 items-center justify-center gap-1.5 px-3.5 text-xs font-semibold disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-60 {btnTouch}"
			>
				<Clock class="size-3.5 shrink-0" aria-hidden="true" />
				<span>{triggerLabel}</span>
				<ChevronDown class="size-3.5 shrink-0 opacity-75" aria-hidden="true" />
			</button>
		{/snippet}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content
		preventScroll={false}
		align="start"
		class="w-64 max-w-[calc(100vw-2rem)] motion-reduce:animate-none! motion-reduce:transition-none!"
	>
		<DropdownMenu.Label>{menuHeading}</DropdownMenu.Label>
		{#each snoozePresets as preset (preset.id)}
			{@const afterExpiry = snoozeExceedsExpiry(preset.iso, item.expiresAtIso)}
			<DropdownMenu.Item
				onSelect={() => onDefer(item, preset.iso, preserveNote ? item.snoozed?.note : undefined)}
				class="flex items-center justify-between gap-3 {menuItemTouch}"
			>
				<span>{preset.label}</span>
				<span class="text-xs text-fg-muted tabular-nums">
					{preset.detail}{afterExpiry
						? isAskReplyCutoff
							? ' · after reply cutoff'
							: ' · after expiry'
						: ''}
				</span>
			</DropdownMenu.Item>
		{/each}
		<DropdownMenu.Separator />
		<DropdownMenu.Item onSelect={() => onOpenCustomSnooze(item, snoozeBtnEl)} class={menuItemTouch}>
			Custom date &amp; time…
		</DropdownMenu.Item>
	</DropdownMenu.Content>
</DropdownMenu.Root>

<style>
	.triage-secondary-btn {
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-secondary-btn:hover:not(:disabled) {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.triage-secondary-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (forced-colors: active) {
		.triage-secondary-btn {
			border: 1px solid ButtonText;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.triage-secondary-btn {
			transition: none;
		}
	}
</style>
