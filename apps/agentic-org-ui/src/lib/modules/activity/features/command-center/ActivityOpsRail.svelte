<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import { eventTime } from '$lib/modules/preferences';
	import type { ReleaseMilestone } from '$lib/modules/releases/types';
	import {
		computeCompletionPct,
		formatTelemetryHours,
		type ActivityCommandCenterModel,
		type FeedPhase,
		type LineEventEntry
	} from './assembly-model';

	let {
		model,
		loading = false,
		unavailableReason = undefined,
		embedded = false,
		projectScope = false,
		inboxPhase = 'ready',
		zone = 'UTC',
		nowMs = Date.now(),
		extraLineEvents = [],
		releaseMilestones = []
	}: {
		model: ActivityCommandCenterModel;
		loading?: boolean;
		unavailableReason?: string;
		embedded?: boolean;
		projectScope?: boolean;
		inboxPhase?: FeedPhase;
		zone?: string;
		nowMs?: number;
		extraLineEvents?: readonly LineEventEntry[];
		releaseMilestones?: readonly ReleaseMilestone[];
	} = $props();

	const agentsCapacity = $derived(
		model.activeAgentsCount > 0
			? `${model.busyAgentsCount} / ${model.activeAgentsCount}`
			: `${model.stageCounts.agents} active`
	);
	const dataUnavailable = $derived(loading || Boolean(unavailableReason));
	const nowDate = $derived(new Date(nowMs));

	const mergedLineEvents = $derived.by<LineEventEntry[]>(() => {
		const seen: Record<string, true> = {};
		const combined: LineEventEntry[] = [];
		for (const item of [...extraLineEvents, ...model.lineEvents]) {
			const dedupKey = `${item.id}|${item.at}|${item.text}`;
			if (Object.hasOwn(seen, dedupKey)) continue;
			seen[dedupKey] = true;
			combined.push(item);
		}
		combined.sort((a, b) => (Date.parse(b.at) || 0) - (Date.parse(a.at) || 0));
		return combined.slice(0, 30);
	});

	function formatLineTime(iso: string): { text: string; full: string; iso?: string } {
		const ms = Date.parse(iso);
		if (Number.isNaN(ms)) return { text: '—', full: iso, iso: undefined };
		return eventTime(new Date(ms), nowDate, zone, true);
	}
</script>

{#snippet projectRailSections()}
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
				<span class="ops-section-meta tabular-nums">{model.readinessPct}%</span>
			</div>
			{#if releaseMilestones.length > 0}
				{#each releaseMilestones as ms (ms.id)}
					{@const msTotal = ms.tasksDone + ms.tasksOpen}
					{@const msPct = computeCompletionPct(ms.tasksDone, msTotal)}
					<div class="project-milestone-block">
						<div class="project-milestone-name">M-{ms.number} · {ms.title}</div>
						<div class="project-milestone-progress">
							<progress
								class={[
									'project-progress-bar',
									msTotal > 0 && ms.tasksDone >= msTotal && 'is-complete'
								]}
								value={ms.tasksDone}
								max={Math.max(1, msTotal)}
								aria-label="{ms.title} progress: {msPct}%"
							></progress>
							<span class="ops-note tabular-nums">
								{msPct}% · {ms.tasksDone}/{msTotal} tasks
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

	<!-- 3. Needs a human (honest loading/unavailable states during Inbox outage) -->
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
		{/if}
	</section>

	<!-- 5. Telemetry Card: Throughput (7d) + Avg. cycle time + Waiting on people now -->
	<section class="project-telemetry" aria-labelledby="proj-ops-throughput-heading">
		<div class="project-telemetry-header">
			<h3 id="proj-ops-throughput-heading">Throughput (7d)</h3>
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
	<section
		class="project-telemetry"
		aria-labelledby="proj-ops-line-events-heading"
		data-testid="project-line-events"
	>
		<div class="project-telemetry-header">
			<h3 id="proj-ops-line-events-heading">Line events</h3>
			<span class="ops-section-meta tabular-nums">{mergedLineEvents.length}</span>
		</div>
		{#if mergedLineEvents.length === 0}
			<p class="ops-note">No line events recorded yet.</p>
		{:else}
			<ol class="project-line-log" role="list">
				{#each mergedLineEvents as ev (ev.id)}
					{@const when = formatLineTime(ev.at)}
					<li data-tone={ev.tone || undefined}>
						<time datetime={when.iso} title={when.full} class="project-line-time">
							{when.text}
						</time>
						<span>{ev.text}</span>
					</li>
				{/each}
			</ol>
		{/if}
	</section>
{/snippet}

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
	<div class={['ops-rail activity-rail is-embedded', projectScope && 'project-release-rail']}>
		{#if projectScope}
			{@render projectRailSections()}
		{:else}
			{@render railSections()}
		{/if}
	</div>
{:else}
	<aside
		class={['ops-rail activity-rail', projectScope && 'project-release-rail']}
		aria-label="Operational context"
	>
		{#if projectScope}
			{@render projectRailSections()}
		{:else}
			{@render railSections()}
		{/if}
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

	.project-progress-bar::-webkit-progress-value {
		background: var(--tone-accent-bar);
		border-radius: 999px;
	}

	.project-progress-bar.is-complete::-webkit-progress-value {
		background: var(--tone-ok-bar);
	}

	.project-progress-bar::-moz-progress-bar {
		background: var(--tone-accent-bar);
		border-radius: 999px;
	}

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

	.project-line-log {
		display: flex;
		flex-direction: column;
		gap: 8px;
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.project-line-log li {
		display: grid;
		grid-template-columns: minmax(4.5rem, max-content) minmax(0, 1fr);
		align-items: baseline;
		gap: 8px;
		font-size: 0.75rem;
		line-height: 1.4;
		color: var(--exec-body);
	}

	.project-line-log li span {
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.project-line-time {
		color: var(--fg-secondary);
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
	}

	.project-line-log li[data-tone='ok'] {
		color: var(--exec-ok);
	}

	.project-line-log li[data-tone='warn'] {
		color: var(--exec-warn);
	}

	.project-line-log li[data-tone='bad'] {
		color: var(--exec-bad);
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
