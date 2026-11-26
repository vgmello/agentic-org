<!--
  @component
  Full-Screen Agent Analysis Mode studio (TASK-119, Prototype Release 06fb02ef):
  - Expands smoothly (280ms) from the 340px right rail while keeping the left navigation sidebar visible.
  - Left pane: floating NeuralOrb + minimal monospace state pill + toggleable Tools & context drawer.
  - Right pane: clean framed live session transcript + discreet Follow trace disclosure under each turn
    + detached composer pill wired to live `membersApi.runNow`.
  - Top-right 3 quiet icon toggles: Show all traces, Tools & context, Exit (Esc).
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Eye from '@lucide/svelte/icons/eye';
	import Minimize2 from '@lucide/svelte/icons/minimize-2';
	import Wrench from '@lucide/svelte/icons/wrench';
	import { toAppError } from '$lib/proto';
	import { membersApi } from '../../api';
	import { reportedText } from '../../present';
	import type { MemberProfile, RunNowResult, Turn } from '../../types';
	import { NeuralOrbEngine, type OrbState } from './neural-orb';

	let {
		open = $bindable(false),
		organizationId,
		profile,
		onstarted,
		onclose
	}: {
		open: boolean;
		organizationId: string;
		profile: MemberProfile;
		onstarted?: (result: RunNowResult) => void;
		onclose?: () => void;
	} = $props();

	let expanded = $state(false);
	let showAllTraces = $state(false);
	let showToolsDrawer = $state(false);
	let openTraceIds = $state<Record<string, boolean>>({});
	let sentPrompts = $state<Record<string, string>>({});
	let turns = $state<Turn[]>([]);
	let loadingRuns = $state(false);
	let promptDraft = $state('');
	let sending = $state(false);
	let inputEl = $state<HTMLInputElement | null>(null);
	let messagesEl = $state<HTMLDivElement | null>(null);
	let orbEngine: NeuralOrbEngine | null = null;

	const isAvailable = $derived(profile.status === 'active' && profile.office !== null);
	const orbState = $derived.by<OrbState>(() => {
		if (sending) return 'thinking';
		if (profile.status === 'inactive') return 'offline';
		if (profile.status === 'paused') return 'paused';
		const pres = profile.presence?.id;
		if (pres === 'working' || pres === 'with_you' || profile.openTurn?.state === 'running') {
			return 'working';
		}
		if (profile.openTurn?.state === 'accepted') return 'thinking';
		if (pres === 'needs_you' || pres === 'waiting_answer') return 'attention';
		if (pres === 'offline') return 'offline';
		return 'idle';
	});

	const rateLabel = $derived(
		orbState === 'working' || orbState === 'thinking'
			? '1.4 Hz'
			: orbState === 'idle'
				? '0.4 Hz'
				: '—'
	);

	function isTraceExpanded(turnId: string): boolean {
		const override = openTraceIds[turnId];
		return override !== undefined ? override : showAllTraces;
	}

	function toggleTrace(turnId: string) {
		openTraceIds = { ...openTraceIds, [turnId]: !isTraceExpanded(turnId) };
	}

	function toggleAllTraces() {
		showAllTraces = !showAllTraces;
		openTraceIds = {};
	}

	async function loadTurns() {
		loadingRuns = true;
		try {
			const loaded = await membersApi.runs(organizationId, profile.id);
			turns = [...loaded].reverse();
		} catch {
			turns = [];
		} finally {
			loadingRuns = false;
		}
	}

	$effect(() => {
		if (!open) {
			expanded = false;
			return;
		}
		void loadTurns();
		const raf = requestAnimationFrame(() => {
			expanded = true;
		});
		void tick().then(() => {
			inputEl?.focus({ preventScroll: true });
		});
		return () => cancelAnimationFrame(raf);
	});

	$effect(() => {
		orbEngine?.setState(orbState);
	});

	function mountOrb(canvas: HTMLCanvasElement) {
		const engine = new NeuralOrbEngine(canvas, orbState);
		orbEngine = engine;
		engine.start();
		let ro: ResizeObserver | undefined;
		if (typeof ResizeObserver !== 'undefined' && canvas.parentElement) {
			ro = new ResizeObserver(() => engine.resize());
			ro.observe(canvas.parentElement);
		}
		return () => {
			ro?.disconnect();
			engine.stop();
			if (orbEngine === engine) orbEngine = null;
		};
	}

	function closeOverlay() {
		expanded = false;
		open = false;
		onclose?.();
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (!open || event.key !== 'Escape') return;
		event.preventDefault();
		event.stopPropagation();
		closeOverlay();
	}

	async function submitPrompt(event: SubmitEvent) {
		event.preventDefault();
		const text = promptDraft.trim();
		if (!text || !isAvailable || sending) return;
		sending = true;
		promptDraft = '';
		orbEngine?.pulse(1);
		try {
			const result = await membersApi.runNow(organizationId, profile.id, text);
			sentPrompts = { ...sentPrompts, [result.turn.id]: text };
			const nextTurn: Turn = {
				...result.turn,
				startedBy: { kind: 'run', byViewer: true }
			};
			turns = [...turns.filter((t) => t.id !== nextTurn.id), nextTurn];
			onstarted?.(result);
			await tick();
			if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
		} catch (e) {
			promptDraft = text;
			toast.error(`Couldn't send instruction to ${profile.name}`, {
				description: toAppError(e).message
			});
		} finally {
			sending = false;
		}
	}

	function turnPromptText(t: Turn): string {
		if (sentPrompts[t.id]) return sentPrompts[t.id];
		if (t.startedBy.ref) return `Started from ${t.startedBy.ref}`;
		if (t.startedBy.kind === 'message') {
			return t.startedBy.by ? `Message from ${t.startedBy.by.name}` : 'Direct message';
		}
		if (t.startedBy.kind === 'assignment') return 'Task assignment';
		if (t.startedBy.byViewer) return 'Instruction from you';
		return t.startedBy.by ? `Run by ${t.startedBy.by.name}` : 'Agent run';
	}
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if open}
	<div
		class="af-shell"
		data-expanded={expanded ? '' : undefined}
		data-testid="agent-analysis-mode"
		role="dialog"
		aria-modal="true"
		aria-label="{profile.name} Analysis Mode"
	>
		<div
			class="af-corner-actions"
			role="toolbar"
			aria-label="{profile.name} Analysis Mode controls"
		>
			<button
				type="button"
				class="af-icon-btn"
				aria-pressed={showAllTraces}
				aria-label="Show all traces"
				title="Show all traces"
				onclick={toggleAllTraces}
			>
				<Eye class="size-4" aria-hidden="true" />
			</button>
			<button
				type="button"
				class="af-icon-btn"
				aria-pressed={showToolsDrawer}
				aria-controls="af-tools-drawer"
				aria-label="Tools & context"
				title="Tools & context"
				onclick={() => (showToolsDrawer = !showToolsDrawer)}
			>
				<Wrench class="size-4" aria-hidden="true" />
			</button>
			<button
				type="button"
				class="af-icon-btn"
				aria-label="Exit Analysis Mode"
				title="Exit Analysis Mode (Esc)"
				onclick={closeOverlay}
			>
				<Minimize2 class="size-4" aria-hidden="true" />
			</button>
		</div>

		<div class="af-stage">
			<div class="af-body">
				<section
					class={['af-orb-col', showToolsDrawer && 'has-tools']}
					aria-label="{profile.name} neural orb"
				>
					<div class="af-orb-wrap">
						<div class="af-orb-stage">
							<canvas {@attach mountOrb} aria-hidden="true"></canvas>
							<span class="sr-only">
								Neural orb reflecting {profile.name} state: {orbState}
							</span>
						</div>
						<div class="af-telemetry" data-cstate={orbState}>
							<span class="af-dot" aria-hidden="true"></span>
							<span class="af-tel-lbl">STATE: <b>{orbState.toUpperCase()}</b></span>
							<span class="af-tel-sep" aria-hidden="true">|</span>
							<span class={['af-speed tabular-nums', orbState === 'working' && 'is-live']}>
								<b>{rateLabel}</b>
							</span>
						</div>
					</div>

					{#if showToolsDrawer}
						<div class="af-tools-drawer" id="af-tools-drawer">
							<div class="af-tools-sec">
								<h3 class="af-tools-lbl">Agent context</h3>
								<dl class="af-tools-dl">
									<div>
										<dt>Office</dt>
										<dd>{profile.office ? profile.office.name : 'Unplaced'}</dd>
									</div>
									{#if profile.office}
										<div>
											<dt>Heartbeat</dt>
											<dd>{reportedText(profile.office.observedAt)}</dd>
										</div>
									{/if}
									<div>
										<dt>Health</dt>
										<dd>{profile.supervision?.health ?? 'Not reported'}</dd>
									</div>
								</dl>
							</div>
							<div class="af-tools-sec">
								<h3 class="af-tools-lbl">Current work</h3>
								{#if profile.currentTask}
									<a
										href={resolve(`/work/${encodeURIComponent(profile.currentTask.ref)}` as '/')}
										onclick={closeOverlay}
										class="af-task-link"
									>
										<span class="font-mono">{profile.currentTask.ref}</span>
										<span>{profile.currentTask.title}</span>
									</a>
								{:else}
									<p class="text-xs text-[var(--exec-muted)]">No active task assigned.</p>
								{/if}
							</div>
							<div class="af-tools-sec">
								<h3 class="af-tools-lbl">Recent runs</h3>
								<p class="text-xs text-[var(--exec-muted)] tabular-nums">
									{turns.length}
									{turns.length === 1 ? 'session' : 'sessions'} recorded
								</p>
							</div>
						</div>
					{/if}
				</section>

				<section class="af-chat-col" aria-label="Session transcript with {profile.name}">
					<div class="af-chat-card">
						{#if !isAvailable}
							<div class="af-notice" role="status">
								<p class="font-semibold text-[var(--exec-ink)]">
									{profile.status === 'paused'
										? `${profile.name} is paused`
										: `${profile.name} is unavailable`}
								</p>
								<p class="text-xs text-[var(--exec-muted)]">
									{profile.office === null
										? 'Assign an office before starting new runs.'
										: 'Resume this agent to send new instructions.'}
								</p>
							</div>
						{/if}

						<div bind:this={messagesEl} class="af-messages">
							{#if loadingRuns}
								<p class="af-empty">Loading sessions…</p>
							{:else if turns.length === 0}
								<div class="af-empty">
									<p>Start an analysis turn with {profile.name}.</p>
								</div>
							{:else}
								<div class="af-thread">
									{#each turns as turn (turn.id)}
										{@const processing = turn.state === 'running' || turn.state === 'accepted'}
										{@const traceOpen = isTraceExpanded(turn.id)}
										<article class="af-turn">
											<div class="af-msg-user">
												<div class="af-user-bubble">{turnPromptText(turn)}</div>
											</div>
											{#if turn.summary || turn.failure}
												<div class="af-reply-text">
													{turn.summary ?? turn.failure}
												</div>
											{/if}
											<div class="af-turn-trace">
												<div class={['c-trace-wrap', traceOpen && 'is-open']}>
													<div class="c-trace-bar">
														{#if processing}
															<span class="c-thinking-cue" role="status">
																<span class="c-thinking-dots" aria-hidden="true">
																	<i></i><i></i><i></i>
																</span>
																<span>Thinking…</span>
															</span>
														{/if}
														<button
															type="button"
															class="c-trace-toggle"
															aria-expanded={traceOpen}
															aria-controls="trace-{turn.id}"
															onclick={() => toggleTrace(turn.id)}
														>
															<span class="c-trace-rule" aria-hidden="true"></span>
															<span>{traceOpen ? 'Hide trace' : 'Follow trace'}</span>
														</button>
													</div>
													{#if traceOpen}
														<div class="c-trace-panel" id="trace-{turn.id}">
															<div class="c-trace-summary">
																State: {turn.state} · Started {new Date(
																	turn.startedAt
																).toLocaleTimeString()}
																{#if turn.endedAt}
																	· Ended {new Date(turn.endedAt).toLocaleTimeString()}
																{/if}
															</div>
															{#if turn.attempts.length > 0}
																<div class="c-steps">
																	{#each turn.attempts as att (att.n)}
																		<div class="c-step">
																			<span class="c-step-dot"></span>
																			<span class="c-step-tool">Attempt #{att.n}</span>
																			<span class="c-step-arg">{att.reason ?? att.outcome}</span>
																		</div>
																	{/each}
																</div>
															{:else}
																<p class="c-trace-empty">
																	{processing
																		? 'Live run in progress at office…'
																		: 'No tool-call breakdown recorded for this session.'}
																</p>
															{/if}
														</div>
													{/if}
												</div>
											</div>
										</article>
									{/each}
								</div>
							{/if}
						</div>
					</div>

					<form class="af-composer-pill" onsubmit={submitPrompt}>
						<label class="sr-only" for="af-prompt-input">Send instruction to {profile.name}</label>
						<input
							bind:this={inputEl}
							bind:value={promptDraft}
							id="af-prompt-input"
							type="text"
							autocomplete="off"
							disabled={!isAvailable || sending}
							placeholder={isAvailable
								? `Ask ${profile.name} anything…`
								: `${profile.name} is unavailable`}
							class="af-input"
						/>
						<button
							type="submit"
							disabled={!isAvailable || sending || !promptDraft.trim()}
							class="af-send-btn"
						>
							{sending ? 'Sending…' : 'Send'}
						</button>
					</form>
				</section>
			</div>
		</div>
	</div>
{/if}

<style>
	.af-shell {
		--af-full-width: calc(100% - var(--shell-sidebar-offset, 0px));
		position: fixed;
		inset: 0 0 0 auto;
		width: min(var(--af-full-width), 340px);
		z-index: 45;
		display: flex;
		flex-direction: column;
		overflow: hidden;
		border-left: 1px solid var(--exec-border-hi, var(--line-strong));
		box-shadow: var(--exec-panel-shadow, none);
		background: var(
			--exec-analysis-bg,
			radial-gradient(circle at 26% 50%, var(--tone-accent-bg, transparent), transparent 46%),
			var(--exec-bg, var(--surface))
		);
		color: var(--exec-ink, var(--fg));
		transition:
			width 280ms cubic-bezier(0.2, 0.7, 0.25, 1),
			opacity 220ms ease;
	}
	@media (min-width: 640px) {
		.af-shell {
			--af-full-width: calc(100% - 68px);
		}
	}
	.af-shell[data-expanded] {
		width: var(--af-full-width);
		border-left-color: var(--exec-border, var(--line));
		box-shadow: none;
	}

	.af-corner-actions {
		position: absolute;
		top: 16px;
		right: 20px;
		z-index: 4;
		display: flex;
		align-items: center;
		gap: 6px;
		opacity: 0.6;
		transition: opacity 0.22s ease;
	}
	.af-corner-actions:is(:hover, :focus-within) {
		opacity: 1;
	}
	.af-icon-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 34px;
		height: 34px;
		border-radius: 8px;
		color: var(--exec-muted, var(--fg-muted));
		cursor: pointer;
	}
	.af-icon-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	.af-icon-btn[aria-pressed='true'] {
		background: var(--tone-accent-bg);
		color: var(--exec-ink, var(--fg));
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}

	.af-stage {
		flex: 1 1 auto;
		min-height: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 36px clamp(24px, 4vw, 60px);
	}
	.af-body {
		width: min(1260px, 100%);
		height: min(680px, 100%);
		min-height: 0;
		display: grid;
		grid-template-columns: minmax(280px, 40%) minmax(0, 1fr);
		gap: clamp(24px, 4vw, 56px);
		align-items: center;
	}
	.af-orb-col {
		display: flex;
		flex-direction: column;
		justify-content: center;
		align-items: center;
		gap: 18px;
		max-height: 100%;
		min-height: 0;
		overflow-y: auto;
		padding: 8px 4px;
		transform: scale(0.76);
		opacity: 0.72;
		transition:
			transform 300ms cubic-bezier(0.2, 0.7, 0.25, 1),
			opacity 220ms ease;
	}
	.af-shell[data-expanded] .af-orb-col {
		transform: scale(1);
		opacity: 1;
	}
	.af-orb-col.has-tools {
		justify-content: flex-start;
		padding-top: 4px;
	}
	.af-orb-wrap {
		width: 100%;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 14px;
		flex: none;
	}
	.af-orb-stage {
		position: relative;
		width: min(400px, 100%, 54vh);
		aspect-ratio: 1 / 1;
		flex-shrink: 0;
		contain: layout paint;
		transform: translateZ(0);
	}
	.af-orb-stage canvas {
		width: 100%;
		height: 100%;
		display: block;
	}
	.af-orb-col.has-tools .af-orb-stage {
		width: min(240px, 100%, 32vh);
	}
	.af-telemetry {
		display: inline-flex;
		align-items: center;
		gap: 9px;
		padding: 6px 16px;
		border-radius: 999px;
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border-hi, var(--line-strong));
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.75rem;
		letter-spacing: 0.04em;
		color: var(--exec-muted, var(--fg-muted));
	}
	.af-dot {
		width: 7px;
		height: 7px;
		border-radius: 999px;
		background: var(--tone-accent, var(--brand));
	}
	.af-telemetry[data-cstate='working'] .af-dot,
	.af-telemetry[data-cstate='thinking'] .af-dot {
		background: var(--tone-ok);
	}
	.af-telemetry[data-cstate='paused'] .af-dot,
	.af-telemetry[data-cstate='offline'] .af-dot {
		background: var(--fg-muted);
	}
	.af-tel-lbl b {
		font-weight: 600;
		color: var(--exec-ink, var(--fg));
	}
	.af-tel-sep {
		opacity: 0.4;
	}
	.af-speed.is-live,
	.af-speed.is-live b {
		color: var(--tone-ok-ink, var(--tone-ok));
	}

	.af-tools-drawer {
		width: min(380px, 100%);
		border-radius: 14px;
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border-hi, var(--line-strong));
		padding: 14px 16px;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}
	.af-tools-sec {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding-top: 10px;
		border-top: 1px solid var(--exec-border, var(--line));
	}
	.af-tools-sec:first-child {
		padding-top: 0;
		border-top: 0;
	}
	.af-tools-lbl {
		margin: 0;
		font-size: 0.75rem;
		font-weight: 650;
		letter-spacing: 0.06em;
		text-transform: uppercase;
		color: var(--exec-muted, var(--fg-muted));
	}
	.af-tools-dl {
		margin: 0;
		display: grid;
		gap: 4px;
		font-size: 0.75rem;
	}
	.af-tools-dl > div {
		display: flex;
		justify-content: space-between;
		gap: 8px;
	}
	.af-tools-dl dt {
		color: var(--exec-muted, var(--fg-muted));
	}
	.af-tools-dl dd {
		margin: 0;
		color: var(--exec-body, var(--fg-secondary));
	}
	.af-task-link {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		font-size: 0.8125rem;
		color: var(--tone-accent-ink, var(--link));
		text-decoration: none;
	}
	.af-task-link:hover {
		text-decoration: underline;
	}

	.af-chat-col {
		height: 100%;
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 14px;
		opacity: 0;
		transform: translateX(18px);
		transition:
			opacity 240ms ease 60ms,
			transform 280ms cubic-bezier(0.2, 0.7, 0.25, 1) 40ms;
	}
	.af-shell[data-expanded] .af-chat-col {
		opacity: 1;
		transform: translateX(0);
	}
	.af-chat-card {
		flex: 1 1 auto;
		min-height: 0;
		display: flex;
		flex-direction: column;
		border-radius: 16px;
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border-hi, var(--line-strong));
		overflow: hidden;
	}
	.af-notice {
		margin: 16px 24px 0;
		padding: 10px 14px;
		border-radius: 10px;
		background: var(--tone-warn-bg);
		border: 1px solid var(--tone-warn-ring);
	}
	.af-messages {
		flex: 1 1 auto;
		min-height: 0;
		overflow-y: auto;
		padding: 28px 30px;
	}
	.af-thread {
		display: flex;
		flex-direction: column;
		gap: 24px;
	}
	.af-empty {
		padding: 56px 16px;
		text-align: center;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.875rem;
	}
	.af-turn {
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.af-msg-user {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
	}
	.af-user-bubble {
		max-width: min(82%, 540px);
		padding: 9px 14px;
		border-radius: 13px;
		border-bottom-right-radius: 4px;
		background: var(--tone-accent-bg);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
		color: var(--exec-ink, var(--fg));
		font-size: 0.875rem;
		line-height: 1.5;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.af-reply-text {
		color: var(--exec-body, var(--fg-secondary));
		font-size: 0.9375rem;
		line-height: 1.62;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		max-width: 64ch;
	}
	.af-turn-trace {
		margin-top: -6px;
	}

	/* Discreet Follow trace disclosure (TASK-119) */
	.c-trace-wrap {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.c-trace-bar {
		display: flex;
		align-items: center;
		gap: 10px;
		flex-wrap: wrap;
	}
	.c-thinking-cue {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		font-size: 0.8125rem;
		font-weight: 500;
		color: var(--exec-body, var(--fg-secondary));
	}
	.c-thinking-dots {
		display: inline-flex;
		align-items: center;
		gap: 3px;
	}
	.c-thinking-dots i {
		width: 5px;
		height: 5px;
		border-radius: 99px;
		background: var(--tone-accent, var(--brand));
		opacity: 0.35;
		animation: trace-dot 1.2s ease-in-out infinite;
	}
	.c-thinking-dots i:nth-child(2) {
		animation-delay: 0.18s;
	}
	.c-thinking-dots i:nth-child(3) {
		animation-delay: 0.36s;
	}
	@keyframes trace-dot {
		0%,
		80%,
		100% {
			opacity: 0.28;
			transform: scale(0.85);
		}
		40% {
			opacity: 1;
			transform: scale(1.15);
		}
	}
	.c-trace-toggle {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 0 4px 2px 0;
		border-radius: 4px;
		border: 0;
		background: transparent;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 500;
		opacity: 0.55;
		cursor: pointer;
		transition:
			opacity 0.16s ease,
			color 0.16s ease;
	}
	.c-trace-rule {
		display: inline-block;
		width: 10px;
		height: 1px;
		background: currentColor;
		opacity: 0.7;
		transition:
			width 0.16s ease,
			background-color 0.16s ease;
	}
	.af-turn:is(:hover, :focus-within) .c-trace-toggle {
		opacity: 0.85;
	}
	.c-trace-toggle:is(:hover, :focus-visible),
	.c-trace-wrap.is-open .c-trace-toggle {
		opacity: 1;
		color: var(--exec-ink, var(--fg));
	}
	.c-trace-wrap.is-open .c-trace-rule {
		width: 14px;
		background: var(--tone-accent-ink, var(--link));
		opacity: 1;
	}
	.c-trace-panel {
		border-radius: 8px;
		background: var(--exec-well, var(--surface-sunken));
		box-shadow: inset 0 0 0 1px var(--exec-border, var(--line));
		padding: 10px 12px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.75rem;
	}
	.c-trace-summary {
		color: var(--exec-muted, var(--fg-muted));
		padding-bottom: 6px;
		border-bottom: 1px solid var(--exec-border, var(--line));
	}
	.c-steps {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.c-step {
		display: grid;
		grid-template-columns: auto auto minmax(0, 1fr);
		align-items: center;
		gap: 8px;
	}
	.c-step-dot {
		width: 5px;
		height: 5px;
		border-radius: 99px;
		background: var(--tone-accent, var(--brand));
	}
	.c-step-tool {
		color: var(--tone-accent-ink, var(--link));
		font-weight: 600;
	}
	.c-step-arg {
		color: var(--exec-muted, var(--fg-muted));
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.c-trace-empty {
		margin: 0;
		color: var(--exec-muted, var(--fg-muted));
	}

	.af-composer-pill {
		flex: none;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 8px 8px 8px 18px;
		border-radius: 14px;
		background: var(--exec-well, var(--surface-raised));
		box-shadow: inset 0 0 0 1px var(--exec-border-hi, var(--line-strong));
		transition: box-shadow 0.16s ease;
	}
	.af-composer-pill:focus-within {
		box-shadow:
			inset 0 0 0 1px var(--tone-accent-ring),
			0 0 0 3px var(--tone-accent-bg);
	}
	.af-input {
		flex: 1;
		min-width: 0;
		border: 0;
		background: transparent;
		color: var(--exec-ink, var(--fg));
		font-size: 0.9375rem;
		line-height: 1.4;
		padding: 6px 0;
		outline: none;
	}
	.af-input::placeholder {
		color: var(--exec-muted, var(--fg-muted));
	}
	.af-send-btn {
		border-radius: 10px;
		padding: 8px 20px;
		background: var(--exec-cta-bg, var(--primary));
		color: var(--exec-on-accent, var(--on-emphasis));
		font-size: 0.875rem;
		font-weight: 600;
		cursor: pointer;
	}
	.af-send-btn:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}

	@media (max-width: 959px) {
		.af-stage {
			padding: 48px 14px 16px;
		}
		.af-body {
			height: 100%;
			grid-template-columns: minmax(0, 1fr);
			grid-template-rows: auto minmax(0, 1fr);
			gap: 14px;
		}
		.af-orb-stage,
		.af-orb-col.has-tools .af-orb-stage {
			width: 156px;
		}
		.af-messages {
			padding: 18px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.af-shell,
		.af-orb-col,
		.af-chat-col,
		.af-corner-actions {
			transition: none;
		}
		.c-thinking-dots i {
			animation: none;
		}
	}
</style>
