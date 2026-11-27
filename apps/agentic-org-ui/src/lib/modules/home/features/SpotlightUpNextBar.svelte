<script lang="ts">
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import type { SpotlightDecision } from '$lib/modules/home/model';

	let {
		decision,
		onSelect
	}: {
		decision: SpotlightDecision;
		onSelect: () => void;
	} = $props();
</script>

<button
	type="button"
	onclick={onSelect}
	class="exec-well next-bar flex min-h-9 items-center justify-between gap-2 px-3 py-2 text-left text-xs"
>
	<span class="flex min-w-0 items-center gap-2">
		<span class="shrink-0 text-xs font-bold tracking-wider text-[var(--exec-dim)] uppercase">
			Up next
		</span>
		<span class="truncate font-medium text-[var(--exec-body)]">
			{decision.title}
		</span>
	</span>
	<span class="flex shrink-0 items-center gap-2">
		{#if decision.expiresShort}
			<span class="next-exp inline-flex items-center gap-1">
				<Clock class="size-3" aria-hidden="true" />
				<span>{decision.expiresShort}</span>
			</span>
		{:else if decision.isChiefOfStaff}
			<span class="next-ai inline-flex items-center gap-1">
				<Sparkles class="size-3" aria-hidden="true" />
				<span>{decision.actorName}</span>
			</span>
		{/if}
		<ChevronRight class="size-3.5 shrink-0 text-[var(--exec-dim)]" aria-hidden="true" />
	</span>
</button>

<style>
	.next-bar:hover {
		background: var(--exec-surface-hi);
	}
	.next-exp {
		padding: 2px 7px;
		border-radius: var(--exec-pill-radius);
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: var(--exec-pill-ring-warn);
		font-size: 0.6875rem;
		font-weight: 600;
		white-space: nowrap;
	}
	.next-ai {
		padding: 2px 7px;
		border-radius: var(--exec-pill-radius);
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		font-size: 0.6875rem;
		font-weight: 600;
		white-space: nowrap;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.next-bar {
			min-height: 44px;
		}
	}
</style>
