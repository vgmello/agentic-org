<script lang="ts">
	import { resolve } from '$app/paths';
	import ActivityOpsGateList from './ActivityOpsGateList.svelte';
	import ActivityOpsThroughput from './ActivityOpsThroughput.svelte';
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
		<ActivityOpsGateList gates={model.releaseGates} />

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
	<ActivityOpsThroughput
		days={model.throughput7d}
		source={model.throughputSource}
		ariaLabel={model.throughputAriaLabel}
		unavailableMessage={model.throughputUnavailableMessage}
	/>
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

<style>
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
	.ops-approval-list,
	.ops-agents {
		margin: 0;
		padding: 0;
		list-style: none;
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
	.ops-approval-link:focus-visible,
	.ops-agent-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
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
