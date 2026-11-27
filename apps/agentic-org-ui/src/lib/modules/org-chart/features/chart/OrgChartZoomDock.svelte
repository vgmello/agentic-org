<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import Home from '@lucide/svelte/icons/home';
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import Minus from '@lucide/svelte/icons/minus';
	import Plus from '@lucide/svelte/icons/plus';

	let {
		zoom,
		onReset,
		onZoomOut,
		onZoomIn,
		onFit
	}: {
		zoom: number;
		onReset: () => void;
		onZoomOut: () => void;
		onZoomIn: () => void;
		onFit: () => void;
	} = $props();
</script>

<Tooltip.Provider delayDuration={150}>
	<div class="org-zoom-dock" role="group" aria-label="Canvas zoom controls">
		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						onclick={onReset}
						aria-label="Reset view"
						class="org-zoom-btn"
					>
						<Home class="size-3.5" aria-hidden="true" />
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					role="tooltip"
					side="top"
					sideOffset={6}
					class="z-[100] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
				>
					Reset view
				</Tooltip.Content>
			</Tooltip.Portal>
		</Tooltip.Root>
		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						onclick={onZoomOut}
						aria-label="Zoom out"
						class="org-zoom-btn"
					>
						<Minus class="size-3.5" aria-hidden="true" />
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					role="tooltip"
					side="top"
					sideOffset={6}
					class="z-[100] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
				>
					Zoom out
				</Tooltip.Content>
			</Tooltip.Portal>
		</Tooltip.Root>
		<span class="org-zoom-pct tabular-nums">{Math.round(zoom * 100)}%</span>
		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						onclick={onZoomIn}
						aria-label="Zoom in"
						class="org-zoom-btn"
					>
						<Plus class="size-3.5" aria-hidden="true" />
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					role="tooltip"
					side="top"
					sideOffset={6}
					class="z-[100] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
				>
					Zoom in
				</Tooltip.Content>
			</Tooltip.Portal>
		</Tooltip.Root>
		<Tooltip.Root>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						onclick={onFit}
						aria-label="Fit chart"
						class="org-zoom-btn"
					>
						<Maximize2 class="size-3.5" aria-hidden="true" />
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					role="tooltip"
					side="top"
					sideOffset={6}
					class="z-[100] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
				>
					Fit chart
				</Tooltip.Content>
			</Tooltip.Portal>
		</Tooltip.Root>
	</div>
</Tooltip.Provider>

<style>
	.org-zoom-dock {
		position: absolute;
		left: 50%;
		bottom: 14px;
		transform: translateX(-50%);
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding: 4px;
		border-radius: 999px;
		background: var(--exec-surface);
		box-shadow:
			inset 0 0 0 1px var(--exec-border-hi),
			var(--tooltip-shadow);
	}
	.org-zoom-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border-radius: 999px;
		color: var(--exec-body);
		cursor: pointer;
	}
	.org-zoom-btn:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.org-zoom-pct {
		min-width: 44px;
		text-align: center;
		font-size: 0.72rem;
		font-weight: 600;
		color: var(--exec-muted);
	}
</style>
