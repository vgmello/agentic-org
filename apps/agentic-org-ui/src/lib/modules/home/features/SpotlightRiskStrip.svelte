<script lang="ts">
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Clock from '@lucide/svelte/icons/clock';
	import Scale from '@lucide/svelte/icons/scale';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Zap from '@lucide/svelte/icons/zap';
	import type { SpotlightDecision } from '$lib/modules/home/model';

	let {
		cells
	}: {
		cells: readonly [
			SpotlightDecision['risk'],
			SpotlightDecision['impact'],
			SpotlightDecision['proof']
		];
	} = $props();
</script>

<div class="exec-well grid grid-cols-1 gap-2.5 p-3 sm:grid-cols-3 sm:gap-0 sm:px-1">
	{#each cells as cell, ci (cell.label + ci)}
		<div class={['flex min-w-0 items-center gap-2.5 sm:px-3', ci > 0 && 'cell-sep']}>
			<span
				class={[
					'well-ico flex size-8 shrink-0 items-center justify-center rounded-full',
					`tone-${cell.tone}`
				]}
			>
				{#if cell.icon === 'shield-check'}
					<ShieldCheck class="size-4" aria-hidden="true" />
				{:else if cell.icon === 'shield-alert'}
					<ShieldAlert class="size-4" aria-hidden="true" />
				{:else if cell.icon === 'bolt'}
					<Zap class="size-4" aria-hidden="true" />
				{:else if cell.icon === 'scale'}
					<Scale class="size-4" aria-hidden="true" />
				{:else if cell.icon === 'check-circle'}
					<CheckCircle2 class="size-4" aria-hidden="true" />
				{:else}
					<Clock class="size-4" aria-hidden="true" />
				{/if}
			</span>
			<div class="min-w-0">
				<div class="truncate text-xs font-semibold text-[var(--exec-ink)]">
					{cell.label}
				</div>
				<div class="truncate text-xs text-[var(--exec-muted)]">
					{cell.sub}
				</div>
			</div>
		</div>
	{/each}
</div>

<style>
	.cell-sep {
		border-top: 1px solid var(--exec-border);
		padding-top: 10px;
	}
	@media (min-width: 640px) {
		.cell-sep {
			border-top: 0;
			padding-top: 0;
			border-left: 1px solid var(--exec-border);
		}
	}
	.well-ico.tone-ok {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}
	.well-ico.tone-warn {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
	}
	.well-ico.tone-sky {
		background: var(--exec-sky-bg);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
		color: var(--exec-sky);
	}
	.well-ico.tone-muted {
		background: var(--exec-surface);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-muted);
	}
</style>
