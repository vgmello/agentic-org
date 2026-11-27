<script lang="ts">
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';

	let {
		isReconnecting,
		subFailed,
		onReconnect
	}: {
		isReconnecting: boolean;
		subFailed: boolean;
		onReconnect: (event: MouseEvent) => void;
	} = $props();
</script>

<div class="cmd-live-banner" role="status" tabindex="-1">
	<span class="cmd-live-text">
		<AlertTriangle class="size-3.5 shrink-0" aria-hidden="true" />
		<span>
			{#if isReconnecting}
				Reconnecting live updates…
			{:else if subFailed}
				Live updates paused
			{:else}
				Live catch-up failed — snapshots may be stale
			{/if}
		</span>
	</span>
	<button
		type="button"
		disabled={isReconnecting}
		aria-busy={isReconnecting}
		onclick={onReconnect}
		class="cmd-live-reconnect min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
	>
		{#if isReconnecting}
			Reconnecting…
		{:else if subFailed}
			Reconnect
		{:else}
			Retry
		{/if}
	</button>
</div>

<style>
	.cmd-live-banner {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px 12px;
		padding: 6px 12px;
		border: 1px solid var(--exec-border-hi);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well);
		color: var(--exec-body);
		font-size: 0.8125rem;
	}
	.cmd-live-banner:focus {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.cmd-live-text {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		color: var(--exec-warn);
		font-weight: 550;
	}
	.cmd-live-reconnect {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		padding: 0 12px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-surface);
		color: var(--tone-accent-ink);
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
	}
	.cmd-live-reconnect:disabled {
		opacity: 0.72;
		cursor: wait;
	}
	.cmd-live-reconnect:hover {
		background: var(--exec-surface-hi);
		color: var(--exec-ink);
	}
	.cmd-live-reconnect:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (forced-colors: active) {
		.cmd-live-banner,
		.cmd-live-reconnect {
			border: 1px solid CanvasText;
		}
	}
</style>
