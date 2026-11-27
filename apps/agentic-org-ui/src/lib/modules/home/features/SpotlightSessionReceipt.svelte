<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import X from '@lucide/svelte/icons/x';
	import { tooltipContentClass } from '$lib/components/classes';

	interface Props {
		title: string;
		outcome: string;
		note?: string;
		onDismiss: () => void;
	}

	let { title, outcome, note, onDismiss }: Props = $props();
</script>

<div
	class="session-receipt flex items-start justify-between gap-2 px-3 py-2 text-xs focus:outline-none"
	role="status"
	tabindex="-1"
	data-spotlight-receipt
>
	<div class="flex min-w-0 flex-col gap-0.5">
		<span class="inline-flex items-center gap-1.5 font-semibold text-[var(--exec-ok)]">
			<CheckCircle2 class="size-3.5 shrink-0" aria-hidden="true" />
			<span class="truncate">{outcome} — {title}</span>
		</span>
		{#if note}
			<p class="receipt-quote text-[var(--exec-body)]">{note}</p>
		{/if}
	</div>
	<Tooltip.Provider delayDuration={200} ignoreNonKeyboardFocus>
		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						aria-label="Dismiss receipt"
						onclick={onDismiss}
						class="receipt-close inline-flex size-6 shrink-0 items-center justify-center rounded-md"
					>
						<X class="size-3.5" aria-hidden="true" />
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
					Dismiss receipt
				</Tooltip.Content>
			</Tooltip.Portal>
		</Tooltip.Root>
	</Tooltip.Provider>
</div>

<style>
	.session-receipt {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	.receipt-quote {
		margin: 0;
		padding-inline-start: 10px;
		border-inline-start: 2px solid var(--exec-ok-ring);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.receipt-close {
		color: var(--exec-muted);
	}
	.receipt-close:hover {
		background: var(--exec-well);
		color: var(--exec-ink);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.receipt-close {
			min-width: 44px;
			min-height: 44px;
		}
	}
</style>
