<script lang="ts">
	import { onDestroy, tick, type Snippet } from 'svelte';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Copy from '@lucide/svelte/icons/copy';
	import FileCheck2 from '@lucide/svelte/icons/file-check-2';
	import PauseCircle from '@lucide/svelte/icons/pause-circle';
	import X from '@lucide/svelte/icons/x';
	import { toast } from 'svelte-sonner';
	import type { WorkStatusRingTone } from '$lib/work/compact';

	let {
		itemId,
		refLabel,
		kindLabel,
		ariaLabel,
		statusTone,
		needsAttention = false,
		onclose,
		children
	}: {
		itemId: string;
		refLabel: string;
		kindLabel: 'Task' | 'Bug' | 'Request';
		ariaLabel: string;
		statusTone?: WorkStatusRingTone;
		needsAttention?: boolean;
		onclose: () => void;
		children: Snippet;
	} = $props();

	let panelEl = $state<HTMLElement | null>(null);
	let copiedForId = $state<string | null>(null);
	const copied = $derived(copiedForId === itemId);
	let copyTimer: ReturnType<typeof setTimeout> | undefined;

	onDestroy(() => {
		clearTimeout(copyTimer);
	});

	$effect(() => {
		void itemId;
		let cancelled = false;
		void tick().then(() => {
			if (cancelled || !panelEl) return;
			panelEl.scrollTop = 0;
			if (!panelEl.contains(document.activeElement)) {
				panelEl.querySelector<HTMLElement>('#work-detail-title')?.focus({ preventScroll: true });
			}
		});
		return () => {
			cancelled = true;
		};
	});

	async function copyRef() {
		try {
			await navigator.clipboard.writeText(refLabel);
			const targetId = itemId;
			copiedForId = targetId;
			clearTimeout(copyTimer);
			copyTimer = setTimeout(() => {
				if (copiedForId === targetId) copiedForId = null;
			}, 3000);
		} catch {
			toast.error("Couldn't copy reference.");
		}
	}
</script>

<div
	bind:this={panelEl}
	class="work-reading"
	role="dialog"
	aria-modal="false"
	aria-label={ariaLabel}
	data-testid="work-reading-panel"
>
	<article class="work-document">
		<div class="work-detail-bar">
			<div class="work-detail-lead">
				<button type="button" class="work-back-btn" onclick={onclose}>
					<ArrowLeft class="size-4 shrink-0" aria-hidden="true" />
					<span>Back to Work</span>
				</button>
				<p class="work-detail-id">
					{#if statusTone}
						<span
							class="work-reading-status"
							data-tone={statusTone}
							data-attention={needsAttention ? '' : undefined}
							aria-hidden="true"
						>
							{#if statusTone === 'done'}
								<CheckCircle2 class="size-3.5" />
							{:else if statusTone === 'blocked'}
								<PauseCircle class="size-3.5" />
							{:else if statusTone === 'review'}
								<FileCheck2 class="size-3.5" />
							{:else}
								<span class="work-reading-status-ring"></span>
							{/if}
						</span>
					{/if}
					<span class="work-detail-ref">{refLabel}</span>
					<span class="work-kind">{kindLabel}</span>
				</p>
			</div>

			<div class="work-detail-actions">
				<button
					type="button"
					class="work-detail-btn"
					aria-label={copied ? `Copied ${refLabel}` : `Copy ${refLabel}`}
					title={copied ? 'Copied' : `Copy ${refLabel}`}
					onclick={copyRef}
				>
					{#if copied}
						<Check class="size-4" aria-hidden="true" />
					{:else}
						<Copy class="size-4" aria-hidden="true" />
					{/if}
				</button>
				<button
					type="button"
					class="work-detail-btn"
					aria-label="Close work details"
					title="Close work details"
					onclick={onclose}
				>
					<X class="size-4" aria-hidden="true" />
				</button>
			</div>
		</div>

		<p class="sr-only" role="status">{copied ? `${refLabel} copied.` : ''}</p>

		<div class="work-detail-body">
			{@render children()}
		</div>
	</article>
</div>

<style>
	.work-reading {
		min-width: 0;
		width: 100%;
		max-width: 100%;
		background: var(--exec-surface);
		color: var(--exec-ink);
		container: work-reading / inline-size;
		overflow-x: hidden;
		overflow-y: auto;
		overscroll-behavior-y: contain;
	}
	.work-document {
		width: 100%;
		max-width: 100%;
		min-width: 0;
		padding: clamp(16px, 3cqi, 28px) clamp(16px, 4cqi, 32px) 36px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.work-detail-bar {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 12px;
		min-width: 0;
	}
	.work-detail-lead {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 10px;
		min-width: 0;
	}
	.work-back-btn {
		display: none;
		align-items: center;
		gap: 6px;
		min-height: 44px;
		padding: 0 8px 0 2px;
		border-radius: var(--exec-ctrl-radius);
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
	}
	.work-back-btn:hover {
		color: var(--exec-ink);
		background: var(--exec-btn-ghost-bg-hover);
	}
	.work-detail-id {
		display: inline-flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		min-width: 0;
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-variant-numeric: tabular-nums;
	}
	.work-detail-ref {
		font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
		font-weight: 600;
		color: var(--exec-ink);
		overflow-wrap: anywhere;
	}
	.work-kind {
		padding-inline-start: 8px;
		border-inline-start: 1px solid var(--exec-border-hi);
		color: var(--exec-muted);
		font-weight: 500;
	}
	.work-detail-actions {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		flex-shrink: 0;
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
	.work-detail-btn:focus-visible,
	.work-back-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.work-detail-body {
		min-width: 0;
		max-width: 100%;
	}
	.work-reading-status {
		position: relative;
		display: inline-flex;
		flex: none;
		align-items: center;
		justify-content: center;
		width: 1rem;
		height: 1rem;
		color: var(--exec-muted);
	}
	.work-reading-status[data-tone='active'] {
		color: var(--tone-warn-ink);
	}
	.work-reading-status[data-tone='done'] {
		color: var(--tone-ok-ink);
	}
	.work-reading-status[data-tone='blocked'] {
		color: var(--tone-bad-ink);
	}
	.work-reading-status[data-tone='review'] {
		color: var(--tone-accent-ink);
	}
	.work-reading-status[data-attention]::after {
		content: '';
		position: absolute;
		inset: 0 0 auto auto;
		width: 0.35rem;
		height: 0.35rem;
		border-radius: 50%;
		background: var(--tone-warn-ink);
		box-shadow: 0 0 0 1.5px var(--exec-surface);
	}
	.work-reading-status-ring {
		display: inline-grid;
		place-items: center;
		width: 0.875rem;
		height: 0.875rem;
		border: 1.5px solid currentColor;
		border-radius: 50%;
	}
	.work-reading-status[data-tone='active'] .work-reading-status-ring::before {
		content: '';
		height: 75%;
		aspect-ratio: 1 / 2;
		border-radius: 0 100% 100% 0;
		border-inline-end: 0.24rem solid currentColor;
		transform: translateX(50%);
	}
	@media (max-width: 959px) {
		.work-document {
			padding: 12px 16px 32px;
		}
		.work-back-btn {
			display: inline-flex;
		}
	}
	@media (max-width: 720px), (pointer: coarse) {
		.work-detail-btn {
			min-width: 44px;
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: no-preference) {
		.work-document {
			animation: work-detail-fade 0.18s ease-out;
		}
		@keyframes work-detail-fade {
			from {
				opacity: 0;
			}
			to {
				opacity: 1;
			}
		}
		@media (min-width: 960px) {
			.work-document {
				animation: work-detail-enter 0.22s ease-out;
			}
			@keyframes work-detail-enter {
				from {
					opacity: 0;
					transform: translateX(16px);
				}
				to {
					opacity: 1;
					transform: translateX(0);
				}
			}
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.work-detail-btn {
			transition: none;
		}
	}
	@media (forced-colors: active) {
		.work-detail-btn,
		.work-back-btn {
			border: 1px solid ButtonText;
		}
		.work-reading-status[data-attention]::after {
			forced-color-adjust: none;
			background: CanvasText;
			box-shadow: 0 0 0 1px Canvas;
		}
	}
</style>
