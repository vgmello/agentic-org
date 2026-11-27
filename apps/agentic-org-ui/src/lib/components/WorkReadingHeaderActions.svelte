<script lang="ts">
	import { resolve } from '$app/paths';
	import { Tooltip } from 'bits-ui';
	import Check from '@lucide/svelte/icons/check';
	import Copy from '@lucide/svelte/icons/copy';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import Minimize2 from '@lucide/svelte/icons/minimize-2';
	import X from '@lucide/svelte/icons/x';

	let {
		refLabel,
		fullPageHref,
		copied = false,
		canExpand = false,
		expanded = false,
		onCopyRef,
		onToggleExpand,
		onClose
	}: {
		refLabel: string;
		fullPageHref?: string;
		copied?: boolean;
		canExpand?: boolean;
		expanded?: boolean;
		onCopyRef: () => void;
		onToggleExpand: () => void;
		onClose: () => void;
	} = $props();
</script>

<Tooltip.Provider delayDuration={150} ignoreNonKeyboardFocus>
	<div class="work-detail-actions">
		{#if fullPageHref}
			<Tooltip.Root ignoreNonKeyboardFocus>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<a
							{...props}
							href={resolve(fullPageHref as '/')}
							class="work-detail-btn"
							aria-label="Open full page for {refLabel}"
						>
							<ExternalLink class="size-4" aria-hidden="true" />
						</a>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						side="bottom"
						sideOffset={6}
						escapeKeydownBehavior="defer-otherwise-close"
						class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
						>Open full page</Tooltip.Content
					>
				</Tooltip.Portal>
			</Tooltip.Root>
		{/if}
		<Tooltip.Root ignoreNonKeyboardFocus>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						class="work-detail-btn"
						aria-label={copied ? `Copied ${refLabel}` : `Copy ${refLabel}`}
						onclick={onCopyRef}
					>
						{#if copied}
							<Check class="size-4" aria-hidden="true" />
						{:else}
							<Copy class="size-4" aria-hidden="true" />
						{/if}
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					side="bottom"
					sideOffset={6}
					escapeKeydownBehavior="defer-otherwise-close"
					class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
					>{copied ? 'Copied' : `Copy ${refLabel}`}</Tooltip.Content
				>
			</Tooltip.Portal>
		</Tooltip.Root>
		{#if canExpand}
			<Tooltip.Root ignoreNonKeyboardFocus>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							class="work-detail-btn work-panel-expand"
							aria-label={expanded ? 'Restore panel width' : 'Expand panel to full width'}
							aria-pressed={expanded}
							onclick={onToggleExpand}
						>
							{#if expanded}
								<Minimize2 class="size-4" aria-hidden="true" />
							{:else}
								<Maximize2 class="size-4" aria-hidden="true" />
							{/if}
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						side="bottom"
						sideOffset={6}
						escapeKeydownBehavior="defer-otherwise-close"
						class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
						>{expanded ? 'Restore panel width' : 'Expand panel'}</Tooltip.Content
					>
				</Tooltip.Portal>
			</Tooltip.Root>
		{/if}
		<Tooltip.Root ignoreNonKeyboardFocus>
			<Tooltip.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						class="work-detail-btn"
						aria-label="Close work details"
						onclick={onClose}
					>
						<X class="size-4" aria-hidden="true" />
					</button>
				{/snippet}
			</Tooltip.Trigger>
			<Tooltip.Portal>
				<Tooltip.Content
					side="bottom"
					sideOffset={6}
					escapeKeydownBehavior="defer-otherwise-close"
					class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
					>Close</Tooltip.Content
				>
			</Tooltip.Portal>
		</Tooltip.Root>
	</div>
</Tooltip.Provider>

<style>
	.work-detail-actions {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		flex-shrink: 0;
		margin-inline-start: auto;
	}
	.work-detail-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		min-width: 36px;
		min-height: 36px;
		border-radius: var(--exec-ctrl-radius);
		background: transparent;
		color: var(--exec-body);
		cursor: pointer;
		transition:
			background-color 120ms ease-out,
			color 120ms ease-out;
	}
	.work-detail-btn:hover {
		background: var(--exec-btn-ghost-bg-hover);
		color: var(--exec-ink);
	}
	.work-detail-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (max-width: 780px) {
		.work-panel-expand {
			display: none;
		}
	}
	@media (max-width: 720px), (pointer: coarse) {
		.work-detail-btn {
			min-width: 44px;
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.work-detail-btn {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.work-detail-btn {
			border: 1px solid ButtonText;
		}
	}
</style>
