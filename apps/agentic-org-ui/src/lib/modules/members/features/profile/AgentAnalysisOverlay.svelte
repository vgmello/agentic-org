<!-- @component Full-Screen Agent Analysis Mode studio (TASK-119). -->
<script lang="ts">
	import { tick } from 'svelte';
	import { toast } from 'svelte-sonner';
	import Eye from '@lucide/svelte/icons/eye';
	import Minimize2 from '@lucide/svelte/icons/minimize-2';
	import Wrench from '@lucide/svelte/icons/wrench';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import { toAppError } from '$lib/proto';
	import { membersApi } from '../../api';
	import type { MemberProfile, RunNowResult, Turn } from '../../types';
	import AgentAnalysisToolsDrawer from './AgentAnalysisToolsDrawer.svelte';
	import AgentAnalysisTurnCard from './AgentAnalysisTurnCard.svelte';
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
			<ModalTooltip label="Show all traces">
				<button
					type="button"
					class="af-icon-btn"
					aria-pressed={showAllTraces}
					aria-label="Show all traces"
					onclick={toggleAllTraces}
				>
					<Eye class="size-4" aria-hidden="true" />
				</button>
			</ModalTooltip>
			<ModalTooltip label="Tools & context">
				<button
					type="button"
					class="af-icon-btn"
					aria-pressed={showToolsDrawer}
					aria-controls="af-tools-drawer"
					aria-label="Tools & context"
					onclick={() => (showToolsDrawer = !showToolsDrawer)}
				>
					<Wrench class="size-4" aria-hidden="true" />
				</button>
			</ModalTooltip>
			<ModalTooltip label="Exit Analysis Mode (Esc)">
				<button
					type="button"
					class="af-icon-btn"
					aria-label="Exit Analysis Mode"
					onclick={closeOverlay}
				>
					<Minimize2 class="size-4" aria-hidden="true" />
				</button>
			</ModalTooltip>
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
						<AgentAnalysisToolsDrawer {profile} turnsCount={turns.length} onClose={closeOverlay} />
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
										<AgentAnalysisTurnCard
											{turn}
											promptText={turnPromptText(turn)}
											traceOpen={isTraceExpanded(turn.id)}
											onToggleTrace={() => toggleTrace(turn.id)}
										/>
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
	}
</style>
