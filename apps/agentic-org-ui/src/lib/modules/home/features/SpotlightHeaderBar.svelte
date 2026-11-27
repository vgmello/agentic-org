<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import { tooltipContentClass } from '$lib/components/classes';
	import type { SpotlightDecision } from '$lib/modules/home/model';

	interface Props {
		currentDecision: SpotlightDecision;
		spotlight: readonly SpotlightDecision[];
		safeIdx: number;
		handledIds?: ReadonlySet<string>;
		onSelect: (index: number) => void;
	}

	let { currentDecision, spotlight, safeIdx, handledIds, onSelect }: Props = $props();

	const MAX_VISIBLE_DOTS = 6;
	const windowStart = $derived(
		spotlight.length <= MAX_VISIBLE_DOTS
			? 0
			: Math.max(
					0,
					Math.min(safeIdx - Math.floor(MAX_VISIBLE_DOTS / 2), spotlight.length - MAX_VISIBLE_DOTS)
				)
	);
	const visibleDots = $derived(
		spotlight.slice(windowStart, windowStart + MAX_VISIBLE_DOTS).map((decision, offset) => ({
			decision,
			index: windowStart + offset
		}))
	);
	const isCurrentHandled = $derived(Boolean(handledIds?.has(currentDecision.id)));
	const disableNav = $derived(spotlight.length <= 1);
</script>

<div class="flex flex-wrap items-center justify-between gap-2">
	<span
		class={[
			'spot-badge inline-flex items-center gap-1.5',
			isCurrentHandled ? 'is-done' : currentDecision.urgent && 'is-urgent'
		]}
	>
		{#if isCurrentHandled}
			<CheckCircle2 class="size-3.5" aria-hidden="true" />
			<span>Handled</span>
		{:else}
			<Clock class="size-3.5" aria-hidden="true" />
			<span>{currentDecision.badgeText}</span>
		{/if}
	</span>

	<Tooltip.Provider delayDuration={200} ignoreNonKeyboardFocus>
		<div class="flex flex-wrap items-center gap-2.5">
			<div class="queue-dots flex items-center" role="group" aria-label="Priority queue">
				{#each visibleDots as entry (entry.decision.id)}
					{@const cleared = Boolean(handledIds?.has(entry.decision.id))}
					{@const dotLabel = `Decision ${entry.index + 1} of ${spotlight.length}: ${entry.decision.title}`}
					<Tooltip.Root>
						<Tooltip.Trigger>
							{#snippet child({ props })}
								<button
									{...props}
									type="button"
									onclick={() => onSelect(entry.index)}
									aria-label={dotLabel}
									aria-current={entry.index === safeIdx ? 'true' : undefined}
									class={[
										'dot-btn',
										entry.index === safeIdx && 'is-cur',
										entry.decision.urgent && !cleared && 'is-urgent',
										cleared && 'is-cleared'
									]}
								></button>
							{/snippet}
						</Tooltip.Trigger>
						<Tooltip.Portal>
							<Tooltip.Content
								side="top"
								sideOffset={6}
								escapeKeydownBehavior="defer-otherwise-close"
								class={tooltipContentClass}
							>
								{dotLabel}
							</Tooltip.Content>
						</Tooltip.Portal>
					</Tooltip.Root>
				{/each}
			</div>

			<span class="text-xs font-semibold text-[var(--exec-muted)] tabular-nums">
				Priority {safeIdx + 1} of {spotlight.length}
			</span>

			<div class="flex items-center gap-1">
				<Tooltip.Root>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<button
								{...props}
								type="button"
								disabled={disableNav}
								onclick={() => onSelect((safeIdx - 1 + spotlight.length) % spotlight.length)}
								aria-label="Previous decision"
								class="stepper-arrow flex size-9 items-center justify-center rounded-full"
							>
								<ChevronLeft class="size-4" aria-hidden="true" />
							</button>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content
							side="top"
							sideOffset={6}
							escapeKeydownBehavior="defer-otherwise-close"
							class={tooltipContentClass}
						>
							Previous decision
						</Tooltip.Content>
					</Tooltip.Portal>
				</Tooltip.Root>

				<Tooltip.Root>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<button
								{...props}
								type="button"
								disabled={disableNav}
								onclick={() => onSelect((safeIdx + 1) % spotlight.length)}
								aria-label="Next decision"
								class="stepper-arrow flex size-9 items-center justify-center rounded-full"
							>
								<ChevronRight class="size-4" aria-hidden="true" />
							</button>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content
							side="top"
							sideOffset={6}
							escapeKeydownBehavior="defer-otherwise-close"
							class={tooltipContentClass}
						>
							Next decision
						</Tooltip.Content>
					</Tooltip.Portal>
				</Tooltip.Root>
			</div>
		</div>
	</Tooltip.Provider>
</div>

<style>
	.spot-badge {
		padding: 4px 11px;
		border-radius: 999px;
		font-size: 0.75rem;
		font-weight: 600;
		background: var(--exec-sky-bg);
		color: var(--exec-sky);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
	}
	.spot-badge.is-urgent {
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
	}
	.spot-badge.is-done {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	.dot-btn {
		width: 24px;
		height: 24px;
		padding: 0;
		border-radius: 999px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		background: transparent;
		cursor: pointer;
		transition: width 0.2s cubic-bezier(0.22, 1, 0.36, 1);
	}
	.dot-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.dot-btn::after {
		content: '';
		display: block;
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: var(--field-border);
		transition:
			width 0.2s cubic-bezier(0.22, 1, 0.36, 1),
			background-color 0.16s ease;
	}
	.dot-btn:hover::after {
		background: var(--exec-muted);
	}
	.dot-btn.is-urgent::after {
		background: var(--tone-warn-ink);
	}
	.dot-btn.is-cur {
		width: 32px;
	}
	.dot-btn.is-cur::after {
		width: 22px;
		background: var(--tone-accent-ink);
	}
	.dot-btn.is-cur.is-urgent::after {
		background: var(--tone-warn-ink);
	}
	.dot-btn.is-cleared::after {
		background: var(--tone-ok-ink);
	}
	.stepper-arrow {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-body);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.stepper-arrow:hover:not(:disabled) {
		background: var(--exec-surface-hi);
		color: var(--exec-ink);
	}
	.stepper-arrow:disabled {
		opacity: 0.35;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.stepper-arrow {
			min-width: 44px;
			min-height: 44px;
		}
		.dot-btn {
			min-height: 36px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.dot-btn,
		.dot-btn::after,
		.stepper-arrow {
			transition: none;
		}
	}
</style>
