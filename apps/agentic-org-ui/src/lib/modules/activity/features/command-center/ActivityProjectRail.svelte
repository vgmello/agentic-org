<script lang="ts">
	import { resolve } from '$app/paths';
	import type { ReleaseMilestone } from '$lib/modules/releases/types';
	import {
		computeReleaseMilestonesAggregatePct,
		computeReleaseMilestoneStats
	} from '$lib/modules/releases/progress';
	import ActivityOpsGateList from './ActivityOpsGateList.svelte';
	import ActivityOpsThroughput from './ActivityOpsThroughput.svelte';
	import ActivityProjectLineEvents from './ActivityProjectLineEvents.svelte';
	import {
		formatTelemetryHours,
		type ActivityCommandCenterModel,
		type FeedPhase,
		type LineEventEntry
	} from './assembly-model';

	let {
		model,
		loading = false,
		unavailableReason = undefined,
		inboxPhase = 'ready',
		zone = 'UTC',
		nowMs = Date.now(),
		extraLineEvents = [],
		releaseMilestones = []
	}: {
		model: ActivityCommandCenterModel;
		loading?: boolean;
		unavailableReason?: string;
		inboxPhase?: FeedPhase;
		zone?: string;
		nowMs?: number;
		extraLineEvents?: readonly LineEventEntry[];
		releaseMilestones?: readonly ReleaseMilestone[];
	} = $props();

	const dataUnavailable = $derived(loading || Boolean(unavailableReason));
	const releaseMilestonesPct = $derived(computeReleaseMilestonesAggregatePct(releaseMilestones));
</script>

<!-- 1. Stations with agent slot dots -->
<section class="ops-section" aria-labelledby="proj-ops-stations-heading">
	<div class="ops-section-head">
		<h3 id="proj-ops-stations-heading">Stations</h3>
		<span class="ops-section-meta">live counts</span>
	</div>
	{#if loading}
		<p class="ops-note">Loading station counts…</p>
	{:else if unavailableReason}
		<p class="ops-note is-warn">Station counts unavailable.</p>
	{:else}
		<dl class="ops-stations project-stations">
			<div>
				<dt>Queue</dt>
				<dd class="tabular-nums"><span>{model.stageCounts.queue} waiting</span></dd>
			</div>
			<div>
				<dt>Agents</dt>
				<dd>
					{#if model.stationAgents.length > 0}
						<span class="project-station-slots" aria-hidden="true">
							{#each model.stationAgents as slot (slot.id)}
								<i data-busy={slot.busy ? '' : undefined} title={slot.name}></i>
							{/each}
						</span>
					{/if}
					<span class="tabular-nums">
						{model.activeAgentsCount > 0
							? `${model.busyAgentsCount} / ${model.activeAgentsCount} busy`
							: `${model.stageCounts.agents} active`}
					</span>
				</dd>
			</div>
			<div>
				<dt>CI</dt>
				<dd class="tabular-nums"><span>{model.ciStationLabel}</span></dd>
			</div>
			<div>
				<dt>Human review</dt>
				<dd class={['tabular-nums', model.stageCounts.review > 0 && 'is-warn']}>
					<span>{model.stageCounts.review} waiting</span>
				</dd>
			</div>
			<div>
				<dt>Merged</dt>
				<dd class="tabular-nums">
					<span>{model.stageCounts.merged} of {model.totalItems}</span>
				</dd>
			</div>
		</dl>
	{/if}
</section>

<!-- 2. Milestones (real release milestones when served, else honest Project delivery progress) -->
{#if !dataUnavailable && (releaseMilestones.length > 0 || model.milestones.length > 0)}
	<section class="ops-section" aria-labelledby="proj-ops-milestones-heading">
		<div class="ops-section-head">
			<h3 id="proj-ops-milestones-heading">
				{releaseMilestones.length > 0 ? 'Milestones' : 'Project progress'}
			</h3>
			<span class="ops-section-meta tabular-nums">
				{releaseMilestones.length > 0 ? releaseMilestonesPct : model.readinessPct}%
			</span>
		</div>
		{#if releaseMilestones.length > 0}
			{#each releaseMilestones as ms (ms.id)}
				{@const msStats = computeReleaseMilestoneStats(ms)}
				<div class="project-milestone-block">
					<div class="project-milestone-name">M-{ms.number} · {ms.title}</div>
					<div class="project-milestone-progress">
						<progress
							class={[
								'project-progress-bar',
								msStats.total > 0 && msStats.finished >= msStats.total && 'is-complete'
							]}
							value={msStats.finished}
							max={Math.max(1, msStats.total)}
							aria-label="{ms.title} progress: {msStats.pct}%"
						></progress>
						<span class="ops-note tabular-nums">
							{msStats.pct}% · {msStats.finished}/{msStats.total} tasks
						</span>
					</div>
				</div>
			{/each}
		{:else}
			{#each model.milestones as milestone (milestone.id)}
				<div class="project-milestone-block">
					<div class="project-milestone-name">{milestone.name}</div>
					<div class="project-milestone-progress">
						<progress
							class={['project-progress-bar', milestone.isComplete && 'is-complete']}
							value={milestone.doneCount}
							max={Math.max(1, milestone.totalCount)}
							aria-label="{milestone.name} progress: {milestone.pct}%"
						></progress>
						<span class="ops-note tabular-nums">{milestone.summary}</span>
					</div>
				</div>
			{/each}
		{/if}
	</section>
{/if}

<!-- 3. Needs a human -->
<section class="ops-section" aria-labelledby="proj-ops-human-heading">
	<div class="ops-section-head">
		<h3 id="proj-ops-human-heading">
			Needs a human
			<span class="project-head-count">
				{dataUnavailable || inboxPhase !== 'ready' ? '—' : model.humanWaitingItems.length}
			</span>
		</h3>
	</div>
	{#if loading || inboxPhase === 'loading'}
		<p class="ops-note">Checking waiting items…</p>
	{:else if unavailableReason}
		<p class="ops-note is-warn">Work data unavailable.</p>
	{:else if inboxPhase === 'error'}
		<p class="ops-note is-warn">Inbox unavailable — waiting reviews cannot be verified.</p>
	{:else if model.humanWaitingItems.length === 0}
		<p class="ops-note">Nothing waiting on a human right now.</p>
	{:else}
		<ul class="ops-approval-list">
			{#each model.humanWaitingItems as item (item.id)}
				<li>
					<a
						href={resolve(item.href as '/inbox')}
						class="ops-approval-link min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
					>
						{item.title}
					</a>
				</li>
			{/each}
		</ul>
	{/if}
</section>

<!-- 4. Gate status -->
<section class="ops-section" aria-labelledby="proj-ops-gates-heading">
	<div class="ops-section-head">
		<h3 id="proj-ops-gates-heading">Gate status</h3>
	</div>
	{#if loading}
		<p class="ops-note">Loading release gates…</p>
	{:else if unavailableReason}
		<p class="ops-note is-warn">Work data unavailable; gates cannot be verified.</p>
	{:else}
		<ActivityOpsGateList gates={model.releaseGates} />
	{/if}
</section>

<!-- 5. Telemetry Card: Work activity (7d) + Avg. cycle time + Waiting on people now -->
<section class="project-telemetry" aria-labelledby="proj-ops-throughput-heading">
	<div class="project-telemetry-header">
		<h3 id="proj-ops-throughput-heading">Work activity (7d)</h3>
		<span class="ops-section-meta tabular-nums">{model.throughputMeta}</span>
	</div>
	<ActivityOpsThroughput
		days={model.throughput7d}
		source={model.throughputSource}
		ariaLabel={model.throughputAriaLabel}
		unavailableMessage={model.throughputUnavailableMessage}
	/>
	<dl class="project-throughput-metrics">
		<div>
			<dt>Avg. cycle time</dt>
			<dd>{dataUnavailable ? '—' : formatTelemetryHours(model.avgCycleHours)}</dd>
		</div>
		<div>
			<dt>Waiting on people now</dt>
			<dd>
				{dataUnavailable || inboxPhase !== 'ready'
					? '—'
					: formatTelemetryHours(model.waitingOnPeopleHours)}
			</dd>
		</div>
	</dl>
</section>

<!-- 6. Telemetry Card: Line events (chronological log) -->
<ActivityProjectLineEvents events={model.lineEvents} extraEvents={extraLineEvents} {zone} {nowMs} />

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
	.ops-approval-list {
		margin: 0;
		padding: 0;
		list-style: none;
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
	.ops-approval-link:focus-visible {
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
	.project-stations > div {
		padding-block: 5px;
	}
	.project-stations > div + div {
		border-block-start: 1px solid var(--line);
	}
	.project-stations dd {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: flex-end;
		gap: 4px 10px;
	}
	.project-station-slots {
		display: flex;
		flex-wrap: wrap;
		justify-content: flex-end;
		gap: 4px;
	}
	.project-station-slots i {
		display: inline-block;
		width: 8px;
		height: 8px;
		border: 1px solid var(--fg-secondary);
		border-radius: 50%;
	}
	.project-station-slots i[data-busy] {
		border-color: var(--tone-accent-ink);
		background: var(--tone-accent-ink);
	}
	.project-milestone-block + .project-milestone-block {
		margin-top: 10px;
	}
	.project-milestone-name {
		margin-bottom: 5px;
		color: var(--tone-accent-ink);
		font-size: 0.8125rem;
		font-weight: 600;
	}
	.project-milestone-progress {
		display: grid;
		gap: 4px;
	}
	.project-progress-bar {
		width: 100%;
		height: 6px;
		appearance: none;
		border: 0;
		border-radius: 999px;
		overflow: hidden;
		background: var(--bar-track);
	}
	.project-progress-bar::-webkit-progress-bar {
		background: var(--bar-track);
		border-radius: 999px;
	}
	.project-progress-bar::-webkit-progress-value,
	.project-progress-bar::-moz-progress-bar {
		background: var(--tone-accent-bar);
		border-radius: 999px;
	}
	.project-progress-bar.is-complete::-webkit-progress-value,
	.project-progress-bar.is-complete::-moz-progress-bar {
		background: var(--tone-ok-bar);
	}
	.project-head-count {
		margin-inline-start: 4px;
		color: var(--fg-secondary);
		font-variant-numeric: tabular-nums;
		font-weight: 500;
	}
	.project-telemetry {
		margin-block: 12px;
		padding: 12px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--detail-panel-surface, var(--surface-raised));
	}
	.project-telemetry-header {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 4px 8px;
		margin-bottom: 8px;
	}
	.project-telemetry-header h3 {
		margin: 0;
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 650;
	}
	.project-throughput-metrics {
		display: flex;
		flex-wrap: wrap;
		gap: 8px 16px;
		margin: 12px 0 0;
	}
	.project-throughput-metrics > div {
		display: flex;
		flex-direction: column-reverse;
		gap: 2px;
	}
	.project-throughput-metrics dt {
		color: var(--fg-secondary);
		font-size: 0.75rem;
	}
	.project-throughput-metrics dd {
		margin: 0;
		color: var(--exec-ink);
		font-size: 0.9375rem;
		font-weight: 650;
		font-variant-numeric: tabular-nums;
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
</style>
