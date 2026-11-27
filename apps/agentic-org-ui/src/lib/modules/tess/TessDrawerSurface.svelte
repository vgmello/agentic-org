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
	import Square from '@lucide/svelte/icons/square';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import X from '@lucide/svelte/icons/x';
	import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import { areaFor } from '$lib/nav';
	import { useNotifications } from '$lib/platform/shell/notifications.svelte';
	import { useShellCommand } from '$lib/platform/shell/shell-command';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import TessDrawerHeader from './TessDrawerHeader.svelte';
	import TessTranscript from './TessTranscript.svelte';
	import type { TessPanelState } from './tess-panel.svelte';

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

<TessDrawerHeader
	{offline}
	{statusLabel}
	hasTurns={panel.turns.length > 0}
	{isExpanded}
	{expandShortcut}
	showTessPill={uiPrefs.showTessPill}
	onOpenChat={() => void openTessDmInChat()}
	onResetSession={() => void panel.resetSession(organizationId)}
	onToggleExpand={() => panel.toggleExpand()}
	onClose={() => onClose(true)}
/>

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

<style>
	.cmp-ctx-row {
		padding: 6px 12px;
		border-bottom: 1px solid var(--exec-border, var(--line));
		flex: none;
	}
	.cmp-ctx-pill {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		max-width: 100%;
		padding: 2px 6px 2px 9px;
		border-radius: 999px;
		background: var(--exec-well, var(--surface-raised));
		border: 1px solid var(--exec-border, var(--line));
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
	}
	.cmp-ctx-lbl {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.cmp-ctx-lbl strong {
		color: var(--exec-ink, var(--fg));
		font-weight: 600;
	}
	.cmp-ctx-pill button {
		display: inline-flex;
		padding: 2px;
		border-radius: 50%;
		color: var(--exec-muted, var(--fg-muted));
		flex: none;
		cursor: pointer;
	}
	.cmp-ctx-pill button:hover {
		color: var(--exec-ink, var(--fg));
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
	}
	.tess-busy-strip,
	.tess-offline-note {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 7px 12px;
		border-bottom: 1px solid var(--exec-border, var(--line));
		font-size: 0.75rem;
		font-weight: 550;
		flex: none;
	}
	.tess-busy-strip {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
	}
	.tess-busy-spin {
		width: 14px;
		height: 14px;
		border-radius: 50%;
		border: 2px solid var(--tone-accent-ring);
		border-top-color: var(--tone-accent);
		flex: none;
		animation: tess-spin 0.85s linear infinite;
	}
	@keyframes tess-spin {
		to {
			transform: rotate(360deg);
		}
	}
	.tess-offline-note {
		background: var(--tone-warn-bg);
		color: var(--tone-warn-ink);
	}
	.tess-connect-link {
		flex: none;
		font-weight: 600;
		color: var(--exec-accent-ink);
		text-decoration: none;
		white-space: nowrap;
	}
	.tess-connect-link:hover {
		text-decoration: underline;
	}
	.tess-compose {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 9px 10px;
		border-top: 1px solid var(--exec-border, var(--line));
		flex: none;
	}
	.tess-compose input {
		flex: 1 1 auto;
		min-width: 0;
		height: 36px;
		padding: 0 11px;
		border: 1px solid var(--exec-tess-card-border, var(--exec-border-hi, var(--line-strong)));
		border-radius: 10px;
		background: var(--exec-tess-card-bg, var(--exec-well, var(--surface-raised)));
		color: var(--exec-ink, var(--fg));
		font: inherit;
		font-size: 0.875rem;
		outline: none;
	}
	.tess-compose input::placeholder {
		color: var(--exec-muted, var(--fg-muted));
	}
	.tess-compose input:focus {
		border-color: var(--tone-accent, var(--focus));
	}
	.tess-send-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 36px;
		height: 36px;
		border-radius: 10px;
		flex: none;
		background: var(--exec-cta-bg, var(--primary));
		color: var(--exec-on-accent, var(--on-emphasis));
		cursor: pointer;
	}
	.tess-send-btn:disabled {
		opacity: 0.4;
		cursor: not-allowed;
	}
	.tess-send-btn.is-stop {
		background: var(--tone-bad);
		color: var(--exec-on-accent);
	}
	@media (max-width: 639px) {
		.tess-compose input {
			font-size: 1rem;
			height: 44px;
		}
		.tess-send-btn {
			width: 44px;
			height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.tess-busy-spin {
			animation: none;
		}
	}
</style>
