<!--
  @component
  Agent Profile right-hand Neural Orb status rail (v6/pages/agent-profile.js parity, TASK-152):
  renders a live 3D particle Neural Orb reflecting the agent's truthful state, a telemetry status strip,
  a "Now" active work card, and a concise runtime/office summary without fabricating unserved metrics.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { agentSetupHref } from '$lib/modules/agent-setup';
	import { reportedText } from '../../present';
	import type { MemberProfile } from '../../types';
	import { NeuralOrbEngine, type OrbState } from './neural-orb';
	import { tabHref } from './tabs';

	let { profile }: { profile: MemberProfile } = $props();

	const telemetry = $derived.by<{ state: OrbState; label: string; detail: string }>(() => {
		if (profile.status === 'inactive') {
			return { state: 'offline', label: 'Offboarded', detail: 'Record read-only' };
		}
		if (profile.status === 'paused') {
			return { state: 'paused', label: 'Paused', detail: 'Admissions paused' };
		}
		const pres = profile.presence?.id;
		if (pres === 'needs_you' || pres === 'waiting_answer') {
			return {
				state: 'attention',
				label: pres === 'needs_you' ? 'Needs you' : 'Waiting',
				detail: profile.presence?.detail ?? 'Waiting for input'
			};
		}
		if (pres === 'working' || pres === 'with_you' || profile.openTurn?.state === 'running') {
			return {
				state: 'working',
				label: 'Working',
				detail: profile.currentTask ? profile.currentTask.ref : 'Active turn'
			};
		}
		if (profile.openTurn?.state === 'accepted') {
			return { state: 'thinking', label: 'Requested', detail: 'Waiting for office start' };
		}
		if (pres === 'offline') {
			return {
				state: 'offline',
				label: 'Offline',
				detail: profile.office?.name ?? 'Office offline'
			};
		}
		if (profile.office === null) {
			return { state: 'attention', label: 'Unplaced', detail: 'No office assigned' };
		}
		if (profile.currentTask) {
			return { state: 'idle', label: 'Assigned', detail: profile.currentTask.ref };
		}
		return {
			state: 'idle',
			label: pres === 'available' ? 'Available' : 'Standing by',
			detail: profile.office ? profile.office.name : 'Status unknown'
		};
	});

	function mountOrb(state: OrbState) {
		return (canvas: HTMLCanvasElement) => {
			const engine = new NeuralOrbEngine(canvas, state);
			engine.start();
			let ro: ResizeObserver | undefined;
			if (typeof ResizeObserver !== 'undefined' && canvas.parentElement) {
				ro = new ResizeObserver(() => engine.resize());
				ro.observe(canvas.parentElement);
			}
			return () => {
				ro?.disconnect();
				engine.stop();
			};
		};
	}
</script>

<aside class="agent-rail" aria-label="Agent status and activity" data-testid="agent-orb-rail">
	<div class="orb-section">
		<div
			class="orb-stage"
			role="img"
			aria-label="Neural orb reflecting {profile.name} status: {telemetry.label}"
		>
			<canvas aria-hidden="true" {@attach mountOrb(telemetry.state)}></canvas>
		</div>
		<div class="telemetry-strip" data-orb-state={telemetry.state}>
			<span class="orb-dot" aria-hidden="true"></span>
			<span class="orb-state-txt">{telemetry.label}</span>
			<span class="text-fg-muted" aria-hidden="true">·</span>
			<span class="orb-detail-txt">{telemetry.detail}</span>
		</div>
	</div>

	<div class="rail-sec">
		<div class="rail-sec-hd">
			<h3 class="rail-lbl">Now</h3>
			{#if profile.currentTask}
				<a
					href={resolve(`/work/${encodeURIComponent(profile.currentTask.ref)}` as '/')}
					class="rail-ref-pill">{profile.currentTask.ref}</a
				>
			{/if}
		</div>
		{#if profile.currentTask}
			<a
				href={resolve(`/work/${encodeURIComponent(profile.currentTask.ref)}` as '/')}
				class="rail-now-title"
			>
				{profile.currentTask.title}
			</a>
			{#if profile.currentTask.project}
				<p class="rail-now-sub">{profile.currentTask.project}</p>
			{/if}
		{:else}
			<p class="rail-now-title is-idle">Nothing in progress</p>
			<p class="rail-now-sub">
				{profile.status === 'paused'
					? 'Resume this agent to admit new work.'
					: profile.office === null
						? 'Place in an office before assigning active runs.'
						: 'Assign a task or start a run to wake this agent.'}
			</p>
		{/if}
	</div>

	<div class="rail-sec">
		<div class="rail-sec-hd">
			<h3 class="rail-lbl">Runtime &amp; office</h3>
			<a href={resolve(agentSetupHref(profile.id) as '/')} class="rail-link">Setup</a>
		</div>
		<dl class="rail-dl">
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
		<div class="rail-links">
			<a href={resolve(tabHref(profile.id, 'work') as '/')} class="rail-link">View work</a>
			<a href={resolve(tabHref(profile.id, 'runs') as '/')} class="rail-link">Runs</a>
		</div>
	</div>
</aside>

<style>
	.agent-rail {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		padding: 1rem;
		border-inline-start: 1px solid var(--line-subtle);
		background: var(--exec-rail-bg, transparent);
	}
	@media (max-width: 1023px) {
		.agent-rail {
			border-inline-start: 0;
			border-block-start: 1px solid var(--line-subtle);
		}
	}
	.orb-section {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.5rem;
		padding: 0.75rem 0.5rem 1rem;
		border-radius: var(--exec-panel-radius, 12px);
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
	}
	.orb-stage {
		width: 11rem;
		height: 11rem;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.orb-stage canvas {
		width: 100%;
		height: 100%;
		display: block;
	}
	.telemetry-strip {
		display: inline-flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: center;
		gap: 0.375rem;
		font-size: 0.75rem;
		line-height: 1.25rem;
		padding: 0.125rem 0.625rem;
		border-radius: 999px;
		background: var(--surface-raised);
		box-shadow: inset 0 0 0 1px var(--line);
	}
	.orb-dot {
		width: 0.4375rem;
		height: 0.4375rem;
		border-radius: 999px;
		background: var(--fg-muted);
		flex: none;
	}
	.telemetry-strip[data-orb-state='working'] .orb-dot,
	.telemetry-strip[data-orb-state='thinking'] .orb-dot {
		background: var(--tone-ok);
	}
	.telemetry-strip[data-orb-state='attention'] .orb-dot {
		background: var(--tone-warn);
	}
	.telemetry-strip[data-orb-state='idle'] .orb-dot {
		background: var(--tone-accent);
	}
	.orb-state-txt {
		font-weight: 600;
		color: var(--fg);
	}
	.orb-detail-txt {
		color: var(--fg-secondary);
		max-width: 11rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.rail-sec {
		display: flex;
		flex-direction: column;
		gap: 0.375rem;
		padding: 0.875rem 1rem;
		border-radius: var(--exec-panel-radius, 12px);
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
	}
	.rail-sec-hd {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 0.5rem;
	}
	.rail-lbl {
		margin: 0;
		font-size: 0.6875rem;
		font-weight: 650;
		letter-spacing: 0.08em;
		text-transform: uppercase;
		color: var(--exec-muted, var(--fg-muted));
	}
	.rail-ref-pill {
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.6875rem;
		padding: 0.0625rem 0.375rem;
		border-radius: 0.375rem;
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
		text-decoration: none;
	}
	.rail-now-title {
		font-size: 0.875rem;
		line-height: 1.35;
		font-weight: 600;
		color: var(--fg);
		text-decoration: none;
		overflow-wrap: anywhere;
	}
	.rail-now-title:hover:not(.is-idle) {
		color: var(--link);
		text-decoration: underline;
	}
	.rail-now-title.is-idle {
		font-weight: 500;
		color: var(--fg-secondary);
	}
	.rail-now-sub {
		margin: 0;
		font-size: 0.75rem;
		line-height: 1.4;
		color: var(--fg-muted);
	}
	.rail-dl {
		margin: 0.125rem 0 0;
		display: grid;
		gap: 0.375rem;
		font-size: 0.75rem;
	}
	.rail-dl > div {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 0.5rem;
	}
	.rail-dl dt {
		color: var(--fg-muted);
	}
	.rail-dl dd {
		margin: 0;
		color: var(--fg-secondary);
		text-align: end;
		overflow-wrap: anywhere;
	}
	.rail-links {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-top: 0.375rem;
		padding-top: 0.5rem;
		border-top: 1px solid var(--line-subtle);
	}
	.rail-link {
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--link);
		text-decoration: none;
		border-radius: 0.25rem;
	}
	.rail-link:hover {
		color: var(--brand-text);
		text-decoration: underline;
	}
	.rail-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	:global(html[data-ui-theme^='report']) .orb-section,
	:global(html[data-ui-theme^='report']) .rail-sec {
		border: 1px solid var(--report-rule);
		border-radius: 4px;
		box-shadow: none;
	}
</style>
