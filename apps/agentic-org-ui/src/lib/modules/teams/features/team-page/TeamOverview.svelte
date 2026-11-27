<!--
  @component
  Team › Overview (Prototype Release 1149b4f1):
  Team purpose and lead link, 28-day window KPIs (Completed, Median cycle time, Median approval wait, Blocked now),
  weekly completions (Tasks vs Bugs) and open work by status (Blocked first), active project progress and health,
  alphabetical contributor counts, and top 3 recent team activity entries.
  Strictly honest: never fabricates completion transitions, cycle times, or approval wait durations when unserved;
  reconciles every chart and total from shared team scope rules.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { Resource } from '$lib/components/resource.svelte';
	import { accountZone, eventTime } from '$lib/modules/preferences';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { teamsApi } from '../../api';
	import { teamHref } from '../../team-page';
	import { teamPageContext } from './context';
	import { reloadOnChange } from './reload.svelte';
	import { deriveTeamOverview } from './team-overview-model';
	import TeamOverviewProjectsAndContribs from './TeamOverviewProjectsAndContribs.svelte';

	const team = teamPageContext();

	let shellData: ReturnType<typeof useShellData> | undefined;
	try {
		shellData = useShellData();
	} catch {
		shellData = undefined;
	}

	const work = team.work;
	const projects = new Resource(() => teamsApi.projects(team.organizationId, team.teamId));
	const activity = new Resource(() => teamsApi.activity(team.organizationId, team.teamId));

	reloadOnChange(projects, () => team.loads);
	reloadOnChange(activity, () => team.loads);

	$effect(() => {
		void accountZone.ensure(team.organizationId);
	});

	const memberNameById = $derived<Record<string, string>>(
		Object.fromEntries((shellData?.snapshot?.members ?? []).map((m) => [m.id, m.name]))
	);

	const model = $derived(
		deriveTeamOverview({
			detail: team.detail,
			workRows: work.state === 'ready' ? work.data : undefined,
			workState: work.state,
			projects: projects.state === 'ready' ? projects.data : undefined,
			activity: activity.state === 'ready' ? activity.data : undefined,
			// Authorized completion transition history & approval wait records are not yet exposed on TeamsApi;
			// pass null so timing medians and 28-day completion counts state unavailability honestly.
			completions: null,
			memberNameById
		})
	);

	const nowDate = $derived(new Date());

	function activityStamp(at: string): { text: string; full: string } {
		const ms = Date.parse(at);
		if (Number.isNaN(ms)) return { text: at, full: at };
		const formatted = eventTime(new Date(ms), nowDate, accountZone.zone, true);
		return { text: formatted.text, full: formatted.full };
	}
</script>

<section class="team-overview" aria-label="Team overview" data-testid="team-overview">
	<div class="team-overview-intro">
		<p>
			{#if team.detail.purpose}
				{team.detail.purpose}
			{:else}
				Overview of {team.detail.team.name}.
			{/if}
			{#if team.detail.team.lead}
				<span>
					Lead
					<a
						href={resolve(`/members/${encodeURIComponent(team.detail.team.lead.id)}` as '/')}
						class="insight-link">{team.detail.team.lead.name}</a
					>
				</span>
			{:else}
				<span>No team lead assigned</span>
			{/if}
		</p>
		<small>Last 28 days · {model.windowLabel}</small>
	</div>

	<dl class="insight-kpis" data-testid="team-overview-kpis">
		{#each model.kpis as kpi (kpi.id)}
			<div data-kpi={kpi.id}>
				<dt>{kpi.label}</dt>
				<dd class="tabular-nums">{kpi.value}</dd>
				<dd class="insight-kpi-note">{kpi.note}</dd>
			</div>
		{/each}
	</dl>

	<div class="insights-grid">
		<section class="insight-card" aria-labelledby="team-throughput-title">
			<header class="insight-card-head">
				<div>
					<h3 id="team-throughput-title">Weekly completions</h3>
					<p>Tasks and bugs completed each week</p>
				</div>
				{#if model.completionsAvailable}
					<div class="insight-legend" aria-label="Series">
						<span><i data-series="0" aria-hidden="true"></i>Task</span>
						<span><i data-series="1" aria-hidden="true"></i>Bug</span>
					</div>
				{/if}
			</header>
			{#if !model.completionsAvailable}
				<p class="insight-empty" data-testid="team-completions-unavailable">
					Completion transition history isn't served by this server yet.
				</p>
			{:else if model.completedTotal === 0}
				<p class="insight-empty">No completed tasks or bugs in the last 28 days.</p>
			{:else}
				<div class="insight-columns" role="list" aria-label="Weekly completions">
					{#each model.weekly as bucket (bucket.label)}
						{@const taskPct =
							model.weeklyMax > 0 ? Math.round((bucket.tasks / model.weeklyMax) * 100) : 0}
						{@const bugPct =
							model.weeklyMax > 0 ? Math.round((bucket.bugs / model.weeklyMax) * 100) : 0}
						<div class="insight-col" role="listitem">
							<span class="insight-col-total tabular-nums">{bucket.total}</span>
							<div class="insight-col-track" aria-hidden="true">
								{#if bucket.tasks > 0}
									<span class="insight-col-bar" data-series="0" style:height="{taskPct}%"></span>
								{/if}
								{#if bucket.bugs > 0}
									<span class="insight-col-bar" data-series="1" style:height="{bugPct}%"></span>
								{/if}
							</div>
							<span class="insight-col-label">{bucket.label}</span>
						</div>
					{/each}
				</div>
			{/if}
		</section>

		<section class="insight-card" aria-labelledby="team-workload-title">
			<header class="insight-card-head">
				<div>
					<h3 id="team-workload-title">Open work by status</h3>
					<p>Current assignments for this team</p>
				</div>
				<a class="insight-link" href={resolve(teamHref(team.teamId, 'work') as '/')}>View work</a>
			</header>
			{#if work.state === 'loading'}
				<p class="insight-empty" role="status">Loading open work…</p>
			{:else if work.state === 'unavailable' || work.state === 'unsupported'}
				<p class="insight-empty" data-testid="team-work-unavailable">
					Open work isn't served by this server yet.
				</p>
			{:else if work.state === 'error'}
				<p class="insight-empty">Couldn't load open work right now.</p>
			{:else if model.openByStatus.length === 0}
				<p class="insight-empty">No open work assigned.</p>
			{:else}
				<ul class="insight-bars" role="list" aria-labelledby="team-workload-title">
					{#each model.openByStatus as row (row.state)}
						{@const pct =
							model.openStatusMax > 0
								? Math.max(6, Math.round((row.count / model.openStatusMax) * 100))
								: 0}
						<li class="insight-point">
							<div class="insight-point-main">
								<span class="insight-point-label">{row.label}</span>
								<div class="insight-point-track" aria-hidden="true">
									<span
										class="insight-point-fill"
										data-status-tone={row.state === 'blocked' ? 'bad' : 'accent'}
										style:width="{pct}%"
									></span>
								</div>
							</div>
							<span class="insight-point-val tabular-nums">{row.count}</span>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	</div>

	<TeamOverviewProjectsAndContribs
		teamId={team.teamId}
		{model}
		projectsState={projects.state}
		workState={work.state}
	/>

	<section class="insight-card" aria-labelledby="team-recent-title">
		<header class="insight-card-head">
			<h3 id="team-recent-title">Recent activity</h3>
			<a class="insight-link" href={resolve(teamHref(team.teamId, 'activity') as '/')}
				>View activity</a
			>
		</header>
		{#if activity.state === 'loading'}
			<p class="insight-empty" role="status">Loading recent activity…</p>
		{:else if activity.state === 'unavailable' || activity.state === 'unsupported'}
			<p class="insight-empty" data-testid="team-activity-unavailable">
				Recent activity isn't served by this server yet.
			</p>
		{:else if activity.state === 'error'}
			<p class="insight-empty">Couldn't load recent activity right now.</p>
		{:else if model.recentActivity.length === 0}
			<p class="insight-empty">No recent team activity.</p>
		{:else}
			<ul class="team-overview-events" role="list">
				{#each model.recentActivity as event (event.id)}
					{@const stamp = activityStamp(event.at)}
					<li>
						<span>
							<strong class="font-medium text-fg">{event.actor}</strong>
							{event.text}
						</span>
						<time datetime={event.at} title={stamp.full}>{stamp.text}</time>
					</li>
				{/each}
			</ul>
		{/if}
	</section>
</section>

<style>
	.team-overview {
		container: team-overview / inline-size;
		display: grid;
		gap: 14px;
		padding: 18px 1rem 32px;
	}
	@media (min-width: 640px) {
		.team-overview {
			padding-inline: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.team-overview {
			padding-inline: 2rem;
		}
	}
	.team-overview-intro,
	.team-overview-events li {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 4px 12px;
	}
	.team-overview-intro p {
		color: var(--fg-secondary);
		font-size: 0.875rem;
		line-height: 1.5;
	}
	.team-overview-intro p span {
		margin-inline-start: 0.35em;
		white-space: nowrap;
	}
	.team-overview-intro small,
	.team-overview-events time {
		color: var(--fg-muted);
		font-size: 0.8125rem;
	}
	.insight-kpis {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 12px;
		margin: 0;
	}
	.insight-kpis > div {
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-width: 0;
		padding: 14px 16px;
		border-radius: var(--exec-panel-radius, 12px);
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
	}
	.insight-kpis dt {
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 600;
	}
	.insight-kpis dd {
		margin: 0;
		color: var(--exec-ink, var(--fg));
		font-size: 1.5rem;
		font-weight: 700;
		letter-spacing: -0.025em;
		line-height: 1.15;
	}
	.insight-kpis dd.insight-kpi-note {
		margin-top: 2px;
		color: var(--fg-muted);
		font-size: 0.75rem;
		font-weight: 400;
		letter-spacing: 0;
		line-height: 1.4;
	}
	.insights-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		align-items: stretch;
		gap: 14px;
	}
	.insight-card {
		display: flex;
		flex-direction: column;
		gap: 12px;
		min-width: 0;
		padding: 16px;
		border-radius: var(--exec-panel-radius, 12px);
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
	}
	.insight-card-head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 6px 12px;
	}
	.insight-card-head h3 {
		margin: 0;
		color: var(--exec-ink, var(--fg));
		font-size: 0.9375rem;
		font-weight: 650;
	}
	.insight-card-head p {
		margin: 2px 0 0;
		color: var(--fg-muted);
		font-size: 0.8125rem;
	}
	.insight-link {
		color: var(--link);
		font-size: 0.8125rem;
		font-weight: 600;
		text-decoration: none;
		border-radius: 4px;
	}
	.insight-link:hover {
		color: var(--brand-text);
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.insight-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.insight-empty {
		margin: 4px 0;
		color: var(--fg-muted);
		font-size: 0.875rem;
	}
	.insight-legend {
		display: flex;
		flex-wrap: wrap;
		gap: 10px;
		font-size: 0.75rem;
		color: var(--fg-muted);
	}
	.insight-legend span {
		display: inline-flex;
		align-items: center;
		gap: 5px;
	}
	.insight-legend i {
		display: inline-block;
		width: 8px;
		height: 8px;
		border-radius: 2px;
		background: var(--tone-accent-bar);
	}
	.insight-legend i[data-series='1'] {
		background: var(--tone-ok-bar);
	}
	.insight-columns {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 10px;
		align-items: end;
		min-height: 9rem;
		padding-top: 8px;
	}
	.insight-col {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 6px;
		min-width: 0;
	}
	.insight-col-total {
		font-size: 0.75rem;
		font-weight: 600;
		color: var(--fg-secondary);
	}
	.insight-col-track {
		display: flex;
		flex-direction: column-reverse;
		gap: 2px;
		width: min(36px, 70%);
		height: 6rem;
		padding: 2px;
		border-radius: 6px;
		background: var(--bar-track);
	}
	.insight-col-bar {
		display: block;
		width: 100%;
		border-radius: 4px;
		background: var(--tone-accent-bar);
	}
	.insight-col-bar[data-series='1'] {
		background: var(--tone-ok-bar);
	}
	.insight-col-label {
		color: var(--fg-muted);
		font-size: 0.7rem;
		text-align: center;
		overflow-wrap: anywhere;
	}
	.insight-bars {
		display: grid;
		gap: 10px;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.insight-point {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(1.5rem, max-content);
		align-items: center;
		gap: 10px;
	}
	.insight-point-main {
		display: grid;
		gap: 4px;
		min-width: 0;
	}
	.insight-point-label {
		color: var(--fg-secondary);
		font-size: 0.8125rem;
		font-weight: 500;
	}
	.insight-point-track {
		height: 8px;
		border-radius: 999px;
		overflow: hidden;
		background: var(--bar-track);
	}
	.insight-point-fill {
		display: block;
		height: 100%;
		border-radius: 999px;
		background: var(--tone-accent-bar);
	}
	.insight-point-fill[data-status-tone='bad'] {
		background: var(--tone-bad-bar);
	}
	.insight-point-val {
		text-align: end;
		color: var(--fg);
		font-size: 0.875rem;
		font-weight: 600;
	}
	.team-overview-events {
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.team-overview-events li {
		padding-block: 9px;
		font-size: 0.875rem;
	}
	.team-overview-events li + li {
		border-block-start: 1px solid var(--line-subtle);
	}
	.team-overview-events li > span {
		flex: 1;
		min-width: min(100%, 15rem);
		color: var(--fg-secondary);
		overflow-wrap: anywhere;
	}
	@container team-overview (max-width: 56rem) {
		.insight-kpis {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
		.insights-grid {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	@container team-overview (max-width: 28rem) {
		.insight-kpis {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	:global(html[data-ui-theme^='report']) .insight-kpis > div,
	:global(html[data-ui-theme^='report']) .insight-card {
		border: 1px solid var(--report-rule);
		border-radius: 4px;
		box-shadow: none;
	}
</style>
