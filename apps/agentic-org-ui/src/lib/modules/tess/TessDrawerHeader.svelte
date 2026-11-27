<!--
  @component
  Header bar for the open/expanded Tess companion drawer (`TessDrawerSurface` child).
  Owns all scoped header styles and accessible expand/close tooltips.
-->
<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Minimize2 from '@lucide/svelte/icons/minimize-2';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import X from '@lucide/svelte/icons/x';

	let {
		offline = false,
		statusLabel,
		hasTurns,
		isExpanded,
		expandShortcut,
		showTessPill,
		onOpenChat,
		onResetSession,
		onToggleExpand,
		onClose
	}: {
		offline?: boolean;
		statusLabel: string;
		hasTurns: boolean;
		isExpanded: boolean;
		expandShortcut: string;
		showTessPill: boolean;
		onOpenChat: () => void;
		onResetSession: () => void;
		onToggleExpand: () => void;
		onClose: () => void;
	} = $props();
</script>

<header class="cmp-hd">
	<div class="cmp-tess-who">
		<span class="cmp-tess-mark" aria-hidden="true">
			{#if offline}
				<WifiOff class="size-3.5" />
			{:else}
				<Sparkles class="size-3.5" />
			{/if}
		</span>
		<div class="cmp-tess-txt">
			<strong>Tess</strong>
			<span class={['cmp-tess-st', offline && 'is-offline']}>{statusLabel}</span>
		</div>
	</div>
	<div class="cmp-hd-acts">
		<button
			type="button"
			onclick={onOpenChat}
			aria-label="Open DM with Tess in Chat"
			title="Open DM with Tess in Chat"
			class="cmp-chat-btn"
		>
			<MessageSquare class="size-3.5 shrink-0" aria-hidden="true" />
			<span>Chat</span>
		</button>
		{#if hasTurns}
			<button
				type="button"
				onclick={onResetSession}
				title="Start a new Tess thread"
				class="cmp-text-btn inline-flex items-center gap-1"
			>
				<RotateCcw class="size-3" aria-hidden="true" />
				<span>New thread</span>
			</button>
		{/if}
		<Tooltip.Provider delayDuration={150}>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							onclick={onToggleExpand}
							aria-label="{isExpanded ? 'Restore size' : 'Maximize'} · {expandShortcut}"
							aria-keyshortcuts="Meta+Shift+E Control+Shift+E"
							class="cmp-icon-btn cmp-expand-btn"
						>
							{#if isExpanded}
								<Minimize2 class="size-3.5" aria-hidden="true" />
							{:else}
								<Maximize2 class="size-3.5" aria-hidden="true" />
							{/if}
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						role="tooltip"
						side="bottom"
						sideOffset={6}
						class="z-[110] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
					>
						{isExpanded ? 'Restore size' : 'Maximize'} · {expandShortcut}
					</Tooltip.Content>
				</Tooltip.Portal>
			</Tooltip.Root>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							onclick={onClose}
							aria-label="{showTessPill ? 'Minimize to bottom' : 'Close'} · Esc"
							class="cmp-icon-btn"
						>
							<X class="size-3.5" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						role="tooltip"
						side="bottom"
						sideOffset={6}
						class="z-[110] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
					>
						{showTessPill ? 'Minimize to bottom' : 'Close'} · Esc
					</Tooltip.Content>
				</Tooltip.Portal>
			</Tooltip.Root>
		</Tooltip.Provider>
	</div>
</header>

<style>
	.cmp-hd {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 10px 12px;
		border-bottom: 1px solid var(--exec-border, var(--line));
		flex: none;
	}
	.cmp-tess-who {
		display: flex;
		align-items: center;
		gap: 9px;
		min-width: 0;
	}
	.cmp-tess-mark {
		width: 28px;
		height: 28px;
		border-radius: 8px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		background: var(--tone-accent-bg, var(--exec-accent-soft));
		border: 1px solid var(--tone-accent-ring, var(--exec-accent-ring));
		color: var(--tone-accent-ink, var(--link));
		flex: none;
	}
	.cmp-tess-txt {
		display: grid;
		gap: 1px;
		line-height: 1.2;
		min-width: 0;
	}
	.cmp-tess-txt strong {
		font-size: 0.875rem;
		font-weight: 650;
	}
	.cmp-tess-st {
		font-size: 0.75rem;
		color: var(--exec-muted, var(--fg-muted));
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.cmp-tess-st.is-offline {
		color: var(--tone-bad-ink);
	}
	.cmp-hd-acts {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		flex: none;
	}
	.cmp-chat-btn {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		height: 28px;
		padding: 0 8px;
		border-radius: 6px;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		background: var(--exec-well, var(--surface-raised));
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.75rem;
		font-weight: 550;
		cursor: pointer;
	}
	.cmp-chat-btn:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink, var(--fg));
	}
	.cmp-text-btn {
		padding: 4px 7px;
		border-radius: 6px;
		color: var(--tone-accent-ink, var(--link));
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
	}
	.cmp-text-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	.cmp-icon-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border-radius: 6px;
		color: var(--exec-muted, var(--fg-muted));
		cursor: pointer;
	}
	.cmp-icon-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	@media (max-width: 639px) {
		.cmp-expand-btn {
			display: none;
		}
	}
</style>
