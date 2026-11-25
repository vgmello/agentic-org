<script lang="ts">
	import { tick } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import Compass from '@lucide/svelte/icons/compass';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import X from '@lucide/svelte/icons/x';
	import type { OfficeInfo, OrgSnapshot, WorkItemSummary } from '$lib/proto';
	import type { InboxItem } from '$lib/modules/inbox/types';
	import { areaFor } from '$lib/nav';
	import { useShellCommand } from '$lib/platform/shell/shell-command';
	import { TESS_STARTERS, type TessPanelState } from './tess-panel.svelte';

	interface Props {
		panel: TessPanelState;
		snapshot?: OrgSnapshot;
		items?: readonly WorkItemSummary[];
		inboxItems?: readonly InboxItem[];
		offices?: readonly OfficeInfo[];
		ownerId?: string;
		onCloseRestoreFocus?: () => void;
	}

	let {
		panel,
		snapshot,
		items = [],
		inboxItems = [],
		offices = [],
		ownerId = 'owner',
		onCloseRestoreFocus
	}: Props = $props();

	const shellCommand = useShellCommand();
	const routeLabel = $derived(areaFor(page.url.pathname) ?? 'Home');

	async function openTessDmInChat() {
		panel.close();
		await shellCommand?.openTessChat();
	}
	const isTessChatActive = $derived(
		page.url.pathname === '/chat' &&
			(page.url.searchParams.get('c')?.toLowerCase().includes('tess') ?? false)
	);
	const effectiveOpen = $derived(panel.open && !isTessChatActive);

	const ambientLabel = $derived(
		panel.contextDismissed
			? undefined
			: panel.decisionContext
				? panel.decisionContext.title
				: routeLabel
	);

	const askedSet = $derived(new Set(panel.turns.map((t) => t.query.toLowerCase())));
	const remainingStarters = $derived(TESS_STARTERS.filter((s) => !askedSet.has(s.toLowerCase())));

	const focusOnMount: Attachment<HTMLInputElement> = (el) => {
		el.focus();
	};

	function scrollOnTurns(count: number): Attachment<HTMLDivElement> {
		return (el) => {
			if (count > 0) {
				el.scrollTop = el.scrollHeight;
			}
		};
	}

	function submitPrompt(raw: string) {
		const trimmed = raw.trim();
		if (!trimmed) return;
		panel.ask(trimmed, {
			snapshot,
			items,
			offices,
			ownerId,
			domain: { inboxItems },
			routeLabel
		});
	}

	function closePanel(restoreFocus = true) {
		panel.close();
		if (restoreFocus) {
			void tick().then(() => onCloseRestoreFocus?.());
		}
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (!effectiveOpen || event.key !== 'Escape') return;
		if (document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"]')) return;
		event.preventDefault();
		closePanel(true);
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if effectiveOpen}
	<button
		type="button"
		aria-label="Close Tess panel"
		tabindex={-1}
		onclick={() => closePanel(true)}
		class="tess-scrim fixed inset-0 z-40 lg:hidden"
	></button>

	<aside
		aria-label="Tess Chief of Staff"
		data-testid="tess-panel"
		class="tess-panel fixed inset-y-0 right-0 z-50 flex w-full max-w-[390px] shrink-0 flex-col lg:static lg:z-20 lg:h-full lg:w-[380px]"
	>
		<header class="tess-head flex items-center justify-between gap-2 px-4 py-3">
			<div class="flex min-w-0 items-center gap-2.5">
				<span
					class="tess-mark inline-flex size-8 shrink-0 items-center justify-center rounded-lg"
					aria-hidden="true"
				>
					<Sparkles class="size-4" />
				</span>
				<div class="min-w-0 leading-tight">
					<div class="flex items-center gap-2">
						<h2 class="truncate text-sm font-semibold text-[var(--exec-ink)]">Tess</h2>
						<span
							class="tess-status-badge inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold"
							title="Tess live runtime (TASK-80.4) is not connected yet — answers use local workspace data only."
						>
							<WifiOff class="size-2.5 shrink-0" aria-hidden="true" />
							<span>Local summary</span>
						</span>
					</div>
					<p class="truncate text-xs text-[var(--exec-muted)]">DM thread · Chief of Staff</p>
				</div>
			</div>
			<div class="flex shrink-0 items-center gap-1">
				<button
					type="button"
					onclick={() => void openTessDmInChat()}
					aria-label="Open DM with Tess in Chat"
					title="Open DM with Tess in Chat"
					class="tess-open-chat inline-flex h-8 items-center gap-1.5 rounded-lg px-2.5 text-xs font-medium"
				>
					<MessageSquare class="size-3.5 shrink-0" aria-hidden="true" />
					<span>Chat</span>
				</button>
				{#if panel.turns.length > 0}
					<button
						type="button"
						onclick={() => panel.clear()}
						aria-label="Clear Tess conversation"
						title="Clear conversation"
						class="tess-icon-btn inline-flex size-9 items-center justify-center rounded-lg"
					>
						<Trash2 class="size-4" aria-hidden="true" />
					</button>
				{/if}
				<button
					type="button"
					onclick={() => closePanel(true)}
					aria-label="Close Tess panel"
					title="Close (Esc)"
					class="tess-icon-btn inline-flex size-9 items-center justify-center rounded-lg"
				>
					<X class="size-4" aria-hidden="true" />
				</button>
			</div>
		</header>

		{#if ambientLabel}
			<div class="tess-ctx-bar flex items-center justify-between gap-2 px-4 py-2">
				<div class="flex min-w-0 items-center gap-1.5 text-xs text-[var(--exec-muted)]">
					<Compass class="size-3.5 shrink-0 text-[var(--exec-accent-ink)]" aria-hidden="true" />
					<span class="shrink-0">Viewing:</span>
					<span class="truncate font-medium text-[var(--exec-ink)]">{ambientLabel}</span>
				</div>
				<button
					type="button"
					onclick={() => panel.dismissContext()}
					aria-label="Dismiss viewing context"
					class="tess-ctx-dismiss inline-flex size-7 shrink-0 items-center justify-center rounded-md"
				>
					<X class="size-3.5" aria-hidden="true" />
				</button>
			</div>
		{/if}

		<div
			{@attach scrollOnTurns(panel.turns.length)}
			class="tess-body flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto p-4"
		>
			{#if panel.turns.length === 0}
				<div class="tess-welcome flex flex-col gap-2 rounded-xl p-3.5">
					<p class="text-sm font-semibold text-[var(--exec-ink)]">
						Ask Tess anything about your organization.
					</p>
					<p class="text-xs leading-relaxed text-[var(--exec-muted)]">
						Right now Tess answers directly from your live workspace data in local quick-summary
						mode. Live tool runs and action proposals arrive once the Tess runtime connects.
					</p>
				</div>

				<div class="flex flex-col gap-2">
					<span class="text-[11px] font-semibold tracking-wider text-[var(--exec-dim)] uppercase">
						Suggested questions
					</span>
					<div class="flex flex-col gap-1.5">
						{#each TESS_STARTERS as starter (starter)}
							<button
								type="button"
								onclick={() => submitPrompt(starter)}
								class="tess-starter flex min-h-9 w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-xs font-medium"
							>
								<Sparkles
									class="size-3.5 shrink-0 text-[var(--exec-accent-ink)]"
									aria-hidden="true"
								/>
								<span class="min-w-0 flex-1 truncate">{starter}</span>
							</button>
						{/each}
					</div>
				</div>
			{:else}
				{#each panel.turns as turn (turn.id)}
					<div class="flex flex-col gap-2">
						<div class="tess-user-bubble self-end rounded-xl px-3 py-2 text-xs font-medium">
							{turn.query}
						</div>
						<article class="tess-answer-card relative overflow-hidden rounded-xl p-3.5 pl-4">
							<div class="mb-1.5 flex flex-wrap items-center justify-between gap-2">
								<span
									class="inline-flex items-center gap-1.5 text-[11px] font-semibold text-[var(--exec-accent-ink)]"
								>
									<Sparkles class="size-3" aria-hidden="true" />
									<span>Tess</span>
								</span>
								{#if turn.mode === 'offline_summary'}
									<span class="tess-offline-pill rounded px-1.5 py-0.5 text-[10px] font-semibold">
										Offline quick summary · Local data only
									</span>
								{/if}
							</div>
							<p class="text-[13px] font-semibold leading-snug text-[var(--exec-ink)]">
								{turn.answer.headline}
							</p>
							{#if turn.answer.bullets.length > 0}
								<ul
									class="mt-2 flex flex-col gap-1.5 text-[13px] leading-relaxed text-[var(--exec-body)]"
								>
									{#each turn.answer.bullets as bullet (bullet.text)}
										<li class="flex flex-wrap items-baseline gap-x-2">
											<span>• {bullet.text}</span>
											{#if bullet.href && bullet.linkLabel}
												<a
													href={resolve(bullet.href as '/')}
													class="text-xs font-semibold text-[var(--exec-accent-ink)] underline underline-offset-2"
												>
													{bullet.linkLabel}
												</a>
											{/if}
										</li>
									{/each}
								</ul>
							{/if}
						</article>
					</div>
				{/each}

				{#if remainingStarters.length > 0}
					<div class="mt-1 flex flex-col gap-1.5 pt-2">
						<span class="text-[11px] font-semibold tracking-wider text-[var(--exec-dim)] uppercase">
							Ask a follow-up
						</span>
						<div class="flex flex-wrap gap-1.5">
							{#each remainingStarters as starter (starter)}
								<button
									type="button"
									onclick={() => submitPrompt(starter)}
									class="tess-chip inline-flex min-h-9 items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium"
								>
									<Sparkles
										class="size-3 shrink-0 text-[var(--exec-accent-ink)]"
										aria-hidden="true"
									/>
									<span>{starter}</span>
								</button>
							{/each}
						</div>
					</div>
				{/if}
			{/if}
		</div>

		<form
			onsubmit={(event) => {
				event.preventDefault();
				submitPrompt(panel.draft);
			}}
			class="tess-foot flex flex-col gap-2 px-3.5 py-3"
		>
			<div class="tess-composer flex items-center gap-2 rounded-xl px-3 py-1.5">
				<input
					{@attach focusOnMount}
					bind:value={panel.draft}
					type="text"
					autocomplete="off"
					aria-label="Ask Tess"
					placeholder="Ask Tess about priorities, risks, or budget…"
					class="min-h-9 min-w-0 flex-1 bg-transparent text-sm text-[var(--exec-ink)] outline-none placeholder:text-[var(--exec-dim)]"
				/>
				<button
					type="submit"
					disabled={!panel.draft.trim()}
					aria-label="Send to Tess"
					class="tess-send inline-flex size-8 shrink-0 items-center justify-center rounded-lg disabled:cursor-not-allowed disabled:opacity-40"
				>
					<ArrowUp class="size-4" aria-hidden="true" />
				</button>
			</div>
			<div class="flex items-center justify-between px-1 text-[11px] text-[var(--exec-dim)]">
				<span>⌘/ Toggle Tess</span>
				<span>Esc Close</span>
			</div>
		</form>
	</aside>
{/if}

<style>
	.tess-scrim {
		background: var(--side-panel-scrim);
	}
	.tess-panel {
		background: var(--exec-cmdk-menu-bg);
		-webkit-backdrop-filter: var(--exec-cmdk-menu-blur);
		backdrop-filter: var(--exec-cmdk-menu-blur);
		border-left: 1px solid var(--exec-border-hi);
		box-shadow: var(--side-panel-shadow);
	}
	.tess-head {
		border-bottom: 1px solid var(--exec-border);
	}
	.tess-mark {
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.tess-status-badge {
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.tess-icon-btn,
	.tess-ctx-dismiss {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.tess-icon-btn:hover,
	.tess-ctx-dismiss:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.tess-open-chat {
		background: var(--exec-cmdk-tag-bg, var(--exec-well));
		color: var(--exec-body);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.tess-open-chat:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-cmdk-opt-ring, var(--exec-accent-ring));
	}
	.tess-ctx-bar {
		background: var(--exec-well);
		border-bottom: 1px solid var(--exec-border);
	}
	.tess-welcome {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.tess-starter,
	.tess-chip {
		background: var(--exec-well);
		color: var(--exec-body);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.tess-starter:hover,
	.tess-chip:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.tess-user-bubble {
		max-width: 88%;
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.tess-answer-card {
		background: var(--exec-well);
		box-shadow:
			inset 3px 0 0 var(--tone-accent),
			inset 0 0 0 1px var(--exec-border);
	}
	.tess-offline-pill {
		background: var(--exec-surface);
		color: var(--exec-dim);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.tess-foot {
		border-top: 1px solid var(--exec-border);
	}
	.tess-composer {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.tess-composer:focus-within {
		box-shadow: inset 0 0 0 1.5px var(--focus);
	}
	.tess-send {
		background: var(--exec-cta-bg);
		color: var(--exec-on-accent);
	}
	@media (max-width: 720px), (pointer: coarse) {
		.tess-icon-btn,
		.tess-send {
			min-height: 44px;
			min-width: 44px;
		}
	}
</style>
