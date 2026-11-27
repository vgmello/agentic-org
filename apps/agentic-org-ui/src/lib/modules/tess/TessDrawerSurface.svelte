<!--
  @component
  Open / Expanded Tess companion drawer surface (header, context pill, status banners, transcript, composer).
  Lazy-loaded by `TessPanel.svelte` on first open so shell first-paint JS stays minimal.
-->
<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { Tooltip } from 'bits-ui';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import Maximize2 from '@lucide/svelte/icons/maximize-2';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Minimize2 from '@lucide/svelte/icons/minimize-2';
	import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Square from '@lucide/svelte/icons/square';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import X from '@lucide/svelte/icons/x';
	import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import { areaFor } from '$lib/nav';
	import { useNotifications } from '$lib/platform/shell/notifications.svelte';
	import { useShellCommand } from '$lib/platform/shell/shell-command';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import TessTranscript from './TessTranscript.svelte';
	import type { TessPanelState } from './tess-panel.svelte';
	import './tess-drawer.css';

	let {
		panel,
		organizationId,
		snapshot,
		items = [],
		inboxItems = [],
		offices = [],
		ownerId = 'owner',
		offline = false,
		isExpanded,
		onClose
	}: {
		panel: TessPanelState;
		organizationId?: string;
		snapshot?: OrgSnapshot;
		items?: readonly WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: readonly OfficeInfo[];
		ownerId?: string;
		offline?: boolean;
		isExpanded: boolean;
		onClose: (restoreFocus?: boolean) => void;
	} = $props();

	const shellCommand = useShellCommand();
	const notifications = useNotifications();
	const routeLabel = $derived(areaFor(page.url.pathname) ?? 'Home');
	const activeTessJobs = $derived(notifications?.activeTessJobs ?? []);
	const isMac = $derived(
		typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/i.test(navigator.platform || '')
	);
	const expandShortcut = $derived(isMac ? '⌘⇧E' : 'Ctrl Shift E');

	const ambientLabel = $derived(
		panel.contextDismissed
			? undefined
			: panel.decisionContext
				? panel.decisionContext.title
				: routeLabel
	);

	const statusLabel = $derived(
		activeTessJobs.length > 0
			? `Refining (${activeTessJobs.length})`
			: panel.isStreaming
				? 'Thinking…'
				: offline
					? 'Offline · Local summary'
					: panel.runtimeStatus === 'ready'
						? 'Chief of Staff · Ready'
						: panel.runtimeStatus === 'no_runtime'
							? 'No runtime · Local summary'
							: 'Offline · Local summary'
	);

	const focusOnMount: Attachment<HTMLInputElement> = (el) => {
		el.focus({ preventScroll: true });
	};

	async function openTessDmInChat() {
		onClose(false);
		await shellCommand?.openTessChat();
	}

	function submitPrompt(raw: string) {
		const trimmed = raw.trim();
		if (!trimmed) return;
		panel.ask(trimmed, {
			organizationId,
			snapshot,
			items,
			offices,
			ownerId,
			domain: { inboxItems },
			routeLabel,
			routePath: page.url.pathname,
			offline
		});
	}

	function handleArtifactNavigate() {
		if (isExpanded) panel.toggleExpand();
	}
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
			onclick={() => void openTessDmInChat()}
			aria-label="Open DM with Tess in Chat"
			title="Open DM with Tess in Chat"
			class="cmp-chat-btn"
		>
			<MessageSquare class="size-3.5 shrink-0" aria-hidden="true" />
			<span>Chat</span>
		</button>
		{#if panel.turns.length > 0}
			<button
				type="button"
				onclick={() => void panel.resetSession(organizationId)}
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
							onclick={() => panel.toggleExpand()}
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
							onclick={() => onClose(true)}
							aria-label="{uiPrefs.showTessPill ? 'Minimize to bottom' : 'Close'} · Esc"
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
						{uiPrefs.showTessPill ? 'Minimize to bottom' : 'Close'} · Esc
					</Tooltip.Content>
				</Tooltip.Portal>
			</Tooltip.Root>
		</Tooltip.Provider>
	</div>
</header>

{#if ambientLabel}
	<div class="cmp-ctx-row">
		<span class="cmp-ctx-pill">
			<span class="cmp-ctx-lbl">Context: <strong>{ambientLabel}</strong></span>
			<Tooltip.Provider delayDuration={150}>
				<Tooltip.Root>
					<Tooltip.Trigger>
						{#snippet child({ props })}
							<button
								{...props}
								type="button"
								onclick={() => panel.dismissContext()}
								aria-label="Detach page context"
							>
								<X class="size-3" aria-hidden="true" />
							</button>
						{/snippet}
					</Tooltip.Trigger>
					<Tooltip.Portal>
						<Tooltip.Content
							role="tooltip"
							side="top"
							sideOffset={6}
							class="z-[110] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
						>
							Detach page context
						</Tooltip.Content>
					</Tooltip.Portal>
				</Tooltip.Root>
			</Tooltip.Provider>
		</span>
	</div>
{/if}

{#if activeTessJobs.length > 0}
	<div class="tess-busy-strip" role="status">
		<span class="tess-busy-spin" aria-hidden="true"></span>
		<span>
			{activeTessJobs.length > 1
				? `Refining ${activeTessJobs.length} tasks in background…`
				: `${activeTessJobs[0].title} — ${activeTessJobs[0].step}`}
		</span>
	</div>
{/if}

{#if offline || panel.runtimeStatus !== 'ready'}
	<div class="tess-offline-note" role="status">
		<WifiOff class="size-3.5 shrink-0" aria-hidden="true" />
		<span class="min-w-0 flex-1">
			{panel.runtimeStatus === 'no_runtime'
				? 'No runtime configured · Offline quick summary'
				: 'Offline quick summary · Local data only'}
		</span>
		<a
			href={resolve('/settings/agents')}
			onclick={handleArtifactNavigate}
			class="tess-connect-link"
		>
			Connect runtime →
		</a>
	</div>
{/if}

<span class="sr-only" aria-live="polite">{panel.liveAnnouncement}</span>

<TessTranscript {panel} onSubmitPrompt={submitPrompt} onArtifactNavigate={handleArtifactNavigate} />

<form
	onsubmit={(event) => {
		event.preventDefault();
		submitPrompt(panel.draft);
	}}
	class="tess-compose"
>
	<input
		{@attach focusOnMount}
		bind:value={panel.draft}
		type="text"
		autocomplete="off"
		aria-label="Ask Tess anything"
		placeholder="Ask Tess anything…"
	/>
	<Tooltip.Provider delayDuration={150}>
		{#if panel.isStreaming}
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="button"
							onclick={() => void panel.stopActiveTurn()}
							aria-label="Stop Tess turn"
							class="tess-send-btn is-stop"
						>
							<Square class="size-3.5 fill-current" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						role="tooltip"
						side="top"
						sideOffset={6}
						class="z-[110] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
					>
						Stop Tess turn
					</Tooltip.Content>
				</Tooltip.Portal>
			</Tooltip.Root>
		{:else}
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							type="submit"
							disabled={!panel.draft.trim()}
							aria-label="Send to Tess"
							class="tess-send-btn"
						>
							<ArrowUp class="size-4" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						role="tooltip"
						side="top"
						sideOffset={6}
						class="z-[110] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
					>
						Send to Tess
					</Tooltip.Content>
				</Tooltip.Portal>
			</Tooltip.Root>
		{/if}
	</Tooltip.Provider>
</form>
