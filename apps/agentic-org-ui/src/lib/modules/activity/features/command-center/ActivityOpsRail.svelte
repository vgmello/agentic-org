<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import type { ActivityCommandCenterModel } from './assembly-model';

	let {
		model,
		loading = false,
		unavailableReason = undefined,
		embedded = false
	}: {
		model: ActivityCommandCenterModel;
		loading?: boolean;
		unavailableReason?: string;
		embedded?: boolean;
	} = $props();

	const agentsCapacity = $derived(
		model.activeAgentsCount > 0
			? `${model.busyAgentsCount} / ${model.activeAgentsCount}`
			: `${model.stageCounts.agents} active`
	);
	const dataUnavailable = $derived(loading || Boolean(unavailableReason));
</script>

{#snippet railSections()}
	<!-- 1. Release Gates -->
	<section
		class="ops-section"
		aria-labelledby={embedded ? undefined : 'ops-gates-heading'}
		aria-label={embedded ? 'Release gates' : undefined}
	>
		{#if !embedded}
			<div class="ops-section-head">
				<h2 id="ops-gates-heading">Release gates</h2>
				<span class="ops-section-meta">{model.scopeTitle}</span>
			</div>
		{:else}
			<div class="ops-section-head">
				<span class="ops-sub-label">Scope</span>
				<span class="ops-section-meta">{model.scopeTitle}</span>
			</div>
		{/if}

		{#if loading}
			<p class="ops-note">Loading release gates…</p>
		{:else if unavailableReason}
			<p class="ops-note is-warn">Work data unavailable; gates cannot be verified.</p>
		{:else}
			<ul class="ops-list">
				{#each model.releaseGates as gate (gate.id)}
					<li>
						<a
							href={resolve(gate.href as '/inbox')}
							class="ops-gate-link min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
						>
							<span class="ops-gate-label">
								{#if gate.tone === 'ok'}
									<CheckCircle2
										class="size-3.5 shrink-0"
										style="color: var(--exec-ok);"
										aria-hidden="true"
									/>
								{:else if gate.tone === 'bad'}
									<AlertTriangle
										class="size-3.5 shrink-0"
										style="color: var(--exec-bad);"
										aria-hidden="true"
									/>
								{:else if gate.tone === 'warn'}
									<AlertTriangle
										class="size-3.5 shrink-0"
										style="color: var(--exec-warn);"
										aria-hidden="true"
									/>
								{:else}
									<span class="ops-gate-dot" aria-hidden="true"></span>
								{/if}
								<span class="ops-gate-text">{gate.label}</span>
							</span>
							<span class="ops-gate-status tabular-nums" data-tone={gate.tone}>
								{gate.statusLabel}
							</span>
						</a>
					</li>
				{/each}
			</ul>

			{#if model.pendingApprovals.length > 0}
				<div class="ops-approvals">
					<div class="ops-sub-label">Pending sign-offs</div>
					<ul class="ops-approval-list">
						{#each model.pendingApprovals as approval (approval.id)}
							<li>
								<a
									href={resolve(approval.href as '/inbox')}
									class="ops-approval-link min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
								>
									{approval.title}
								</a>
							</li>
						{/each}
					</ul>
				</div>
			{/if}
		{/if}
	</section>

	<!-- 2. Stations -->
	<section class="ops-section" aria-labelledby="ops-stations-heading">
		<div class="ops-section-head">
			{#if embedded}
				<h3 id="ops-stations-heading">Stations</h3>
			{:else}
				<h2 id="ops-stations-heading">Stations</h2>
			{/if}
			<span class="ops-section-meta">live counts</span>
		</div>
		{#if loading}
			<p class="ops-note">Loading station counts…</p>
		{:else if unavailableReason}
			<p class="ops-note is-warn">Station counts unavailable.</p>
		{:else}
			<dl class="ops-stations">
				<div>
					<dt>Queue</dt>
					<dd class="tabular-nums">{model.stageCounts.queue} waiting</dd>
				</div>
				<div>
					<dt>Agents</dt>
					<dd class="tabular-nums">{agentsCapacity}</dd>
				</div>
				<div>
					<dt>CI</dt>
					<dd class="tabular-nums">{model.ciStationLabel}</dd>
				</div>
				<div>
					<dt>Human review</dt>
					<dd class={['tabular-nums', model.stageCounts.review > 0 && 'is-warn']}>
						{model.stageCounts.review} waiting
					</dd>
				</div>
			</dl>
			<p class="ops-note">{model.stationsNote}</p>
		{/if}
	</section>

	<!-- 3. Throughput (7d) -->
	<section class="ops-section" aria-labelledby="ops-throughput-heading">
		<div class="ops-section-head">
			{#if embedded}
				<h3 id="ops-throughput-heading">Throughput (7d)</h3>
			{:else}
				<h2 id="ops-throughput-heading">Throughput (7d)</h2>
			{/if}
			<span class="ops-section-meta tabular-nums">{model.throughputMeta}</span>
		</div>
		{#if model.throughputSource === 'unavailable'}
			<p class="ops-note is-warn">
				{model.throughputUnavailableMessage ?? 'Throughput data is unavailable.'}
			</p>
		{:else if model.throughputSource === 'loading'}
			<p class="ops-note">Loading 7-day activity…</p>
		{:else}
			<ol class="ops-throughput" aria-label={model.throughputAriaLabel}>
				{#each model.throughput7d as day (day.key)}
					<li class={['ops-day', day.isToday && 'is-today']}>
						<span class="ops-day-count tabular-nums">{day.count}</span>
						<div class="ops-day-track" aria-hidden="true">
							<span
								class="ops-day-bar"
								style:height={`${day.count > 0 ? Math.max(16, day.heightPct) : 6}%`}
								data-empty={day.count === 0 ? 'true' : undefined}
							></span>
						</div>
						<span class="ops-day-label">{day.label}</span>
					</li>
				{/each}
			</ol>
		{/if}
	</section>

	<!-- 4. Active Agents -->
	<section class="ops-section" aria-labelledby="ops-agents-heading">
		<div class="ops-section-head">
			{#if embedded}
				<h3 id="ops-agents-heading">Active agents</h3>
			{:else}
				<h2 id="ops-agents-heading">Active agents</h2>
			{/if}
			<span class="ops-section-meta tabular-nums">{dataUnavailable ? '—' : agentsCapacity}</span>
		</div>

		{#if model.railAgents.length === 0}
			<p class="ops-note">No agents hired yet.</p>
		{:else}
			<ul class="ops-agents">
				{#each model.railAgents as agent (agent.id)}
					<li class="ops-agent-item">
						<a
							href={resolve('/(shell)/members/[memberId]/(profile)', { memberId: agent.id })}
							class="ops-agent-link"
						>
							<div class="ops-agent-top">
								<span class="ops-agent-identity">
									<span class="ops-agent-avatar" aria-hidden="true">{agent.initials}</span>
									<span class="ops-agent-name">{agent.name}</span>
								</span>
								<span class="ops-agent-status" data-status={agent.status}>
									{agent.statusLabel}
								</span>
							</div>
							{#if agent.currentWork}
								<div class="ops-agent-work">
									<span class="ops-agent-ref">{agent.currentWork.shortId}</span>
									<span class="truncate">{agent.currentWork.title}</span>
								</div>
							{:else}
								<div class="ops-agent-title">{agent.title}</div>
							{/if}
						</a>
					</li>
				{/each}
			</ul>
			{#if model.extraAgentsCount > 0}
				<p class="ops-note">
					+{model.extraAgentsCount} more {model.extraAgentsCount === 1 ? 'agent' : 'agents'}
				</p>
			{/if}
		{/if}
	</section>
{/snippet}

{#if embedded}
	<div class="ops-rail activity-rail is-embedded">
		{@render railSections()}
	</div>
{:else}
	<aside class="ops-rail activity-rail" aria-label="Operational context">
		{@render railSections()}
	</aside>
{/if}

<style>
	.ops-rail {
		min-width: 0;
		max-width: 100%;
		border: 1px solid var(--line);
		border-radius: var(--exec-inset-radius, 12px);
		background-color: var(--exec-surface);
		background-image: var(--exec-panel-sheen);
		-webkit-backdrop-filter: var(--exec-blur);
		backdrop-filter: var(--exec-blur);
		padding: 4px 16px;
	}

	.ops-rail.is-embedded {
		border: 0;
		border-radius: 0;
		background-color: transparent;
		background-image: none;
		-webkit-backdrop-filter: none;
		backdrop-filter: none;
		padding: 0 var(--activity-panel-padding, 16px) 24px;
	}

	.ops-section {
		padding: 14px 0;
		min-width: 0;
	}

	.ops-section + .ops-section {
		border-top: 1px solid var(--line);
	}

	.ops-section-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 8px;
		margin-bottom: 8px;
		min-width: 0;
	}

	.ops-section h2,
	.ops-section h3 {
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 650;
		flex-shrink: 0;
	}

	.ops-section-meta {
		color: var(--fg-secondary);
		font-size: 0.75rem;
		text-align: end;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.ops-list,
	.ops-approval-list,
	.ops-agents {
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.ops-gate-link {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 4px 0;
		color: var(--exec-body);
		font-size: 0.8125rem;
		text-decoration: none;
		min-width: 0;
	}

	.ops-gate-link:hover {
		color: var(--tone-accent-ink);
	}

	.ops-gate-link:focus-visible,
	.ops-approval-link:focus-visible,
	.ops-agent-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}

	.ops-gate-label {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.ops-gate-text {
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.ops-gate-dot {
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: var(--fg-secondary);
		flex-shrink: 0;
	}

	.ops-gate-status {
		font-size: 0.75rem;
		font-weight: 600;
		white-space: nowrap;
		flex-shrink: 0;
	}

	.ops-gate-status[data-tone='ok'] {
		color: var(--exec-ok);
	}

	.ops-gate-status[data-tone='warn'] {
		color: var(--exec-warn);
	}

	.ops-gate-status[data-tone='bad'] {
		color: var(--exec-bad);
	}

	.ops-gate-status[data-tone='neutral'] {
		color: var(--fg-secondary);
	}

	.ops-approvals {
		margin-top: 10px;
		padding-top: 8px;
		border-top: 1px solid var(--line-subtle);
		min-width: 0;
	}

	.ops-sub-label {
		color: var(--fg-secondary);
		font-size: 0.6875rem;
		font-weight: 600;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		margin-bottom: 4px;
	}

	.ops-approval-link {
		display: flex;
		align-items: center;
		padding: 4px 0;
		color: var(--exec-body);
		font-size: 0.75rem;
		line-height: 1.4;
		text-decoration: none;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.ops-approval-link:hover {
		color: var(--tone-accent-ink);
		text-decoration: underline;
	}

	.ops-stations {
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.ops-stations > div {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		font-size: 0.8125rem;
	}

	.ops-stations dt {
		color: var(--fg-secondary);
	}

	.ops-stations dd {
		margin: 0;
		color: var(--exec-ink);
		font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.75rem;
		font-weight: 600;
	}

	.ops-stations dd.is-warn {
		color: var(--exec-warn);
	}

	.ops-note {
		margin: 8px 0 0;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		line-height: 1.4;
	}

	.ops-note.is-warn {
		color: var(--exec-warn);
	}

	.ops-throughput {
		display: grid;
		grid-template-columns: repeat(7, minmax(0, 1fr));
		gap: 6px;
		margin: 8px 0 0;
		padding: 0;
		list-style: none;
	}

	.ops-day {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
	}

	.ops-day-count {
		color: var(--fg-secondary);
		font-size: 0.6875rem;
	}

	.ops-day.is-today .ops-day-count,
	.ops-day.is-today .ops-day-label {
		color: var(--tone-accent-ink);
		font-weight: 650;
	}

	.ops-day-track {
		display: flex;
		align-items: flex-end;
		width: 100%;
		height: 48px;
		padding-bottom: 2px;
		border-bottom: 1px solid var(--line);
	}

	.ops-day-bar {
		display: block;
		width: 100%;
		border-radius: 3px 3px 0 0;
		background: var(--tone-accent);
	}

	.ops-day-bar[data-empty='true'] {
		background: var(--bar-track);
	}

	.ops-day-label {
		color: var(--fg-secondary);
		font-size: 0.625rem;
		white-space: nowrap;
	}

	.ops-agents {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.ops-agent-link {
		display: block;
		min-height: 44px;
		padding: 8px 10px;
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		text-decoration: none;
		transition: box-shadow 0.15s ease;
	}

	.ops-agent-link:hover {
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}

	.ops-agent-top {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 6px;
	}

	.ops-agent-identity {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		min-width: 0;
	}

	.ops-agent-avatar {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 22px;
		height: 22px;
		border-radius: 999px;
		background: var(--exec-avatar-bg);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--exec-ink);
		font-size: 0.625rem;
		font-weight: 650;
		flex-shrink: 0;
	}

	.ops-agent-name {
		color: var(--exec-ink);
		font-size: 0.8125rem;
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.ops-agent-status {
		font-size: 0.6875rem;
		font-weight: 600;
		white-space: nowrap;
		color: var(--fg-secondary);
	}

	.ops-agent-status[data-status='building'] {
		color: var(--exec-sky);
	}

	.ops-agent-status[data-status='waiting'] {
		color: var(--exec-warn);
	}

	.ops-agent-status[data-status='paused'] {
		color: var(--exec-bad);
	}

	.ops-agent-work {
		display: flex;
		align-items: center;
		gap: 5px;
		margin-top: 4px;
		color: var(--exec-body);
		font-size: 0.75rem;
	}

	.ops-agent-ref {
		color: var(--tone-accent-ink);
		font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.6875rem;
		font-weight: 600;
		flex-shrink: 0;
	}

	.ops-agent-title {
		margin-top: 3px;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	@media (min-width: 640px) and (max-width: 1249px) {
		.ops-rail:not(.is-embedded) {
			display: grid;
			grid-template-columns: repeat(2, minmax(0, 1fr));
			gap: 16px;
			padding: 16px;
		}
		.ops-rail:not(.is-embedded) .ops-section {
			padding: 0;
		}
		.ops-rail:not(.is-embedded) .ops-section + .ops-section {
			border-top: 0;
		}
	}

	@media (min-width: 960px) and (max-width: 1249px) {
		.ops-rail:not(.is-embedded) {
			grid-template-columns: repeat(4, minmax(0, 1fr));
			gap: 14px;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.ops-agent-link {
			transition: none;
		}
	}

	@media (forced-colors: active) {
		.ops-agent-link {
			border: 1px solid CanvasText;
		}
	}
</style>
