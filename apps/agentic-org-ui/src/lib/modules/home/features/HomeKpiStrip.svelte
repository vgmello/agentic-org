<script lang="ts">
	import { resolve } from '$app/paths';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import DollarSign from '@lucide/svelte/icons/dollar-sign';
	import Folder from '@lucide/svelte/icons/folder';
	import Users from '@lucide/svelte/icons/users';
	import {
		formatCycleDuration,
		type ExecutiveKpis,
		type SignalTone
	} from '$lib/modules/home/model';
	import HomeKpiCard from './HomeKpiCard.svelte';
	import HomeThroughputSparkline from './HomeThroughputSparkline.svelte';

	interface Props {
		kpis: ExecutiveKpis;
	}

	let { kpis }: Props = $props();

	const onTrackProjects = $derived(
		Math.max(0, kpis.projectsTotal - kpis.atRiskProjects - kpis.offTrackProjects)
	);
	const projectsBarOk = $derived(
		kpis.projectsTotal > 0 ? Math.round((onTrackProjects / kpis.projectsTotal) * 100) : 0
	);
	const projectsBarWarn = $derived(
		kpis.projectsTotal > 0 ? Math.round((kpis.atRiskProjects / kpis.projectsTotal) * 100) : 0
	);
	const projectsBarBad = $derived(
		kpis.projectsTotal > 0 ? Math.max(0, 100 - projectsBarOk - projectsBarWarn) : 0
	);
	const projectsPillTone = $derived<SignalTone>(
		kpis.projectsTotal === 0
			? 'muted'
			: kpis.projectsAttention === 0
				? 'ok'
				: kpis.offTrackProjects > 0 && kpis.atRiskProjects === 0
					? 'bad'
					: 'warn'
	);
	const projectsPillLabel = $derived(
		kpis.projectsTotal === 0
			? 'No projects yet'
			: kpis.projectsAttention === 0
				? 'All on track'
				: kpis.offTrackProjects > 0 && kpis.atRiskProjects === 0
					? `${kpis.offTrackProjects} ${kpis.offTrackProjects === 1 ? 'project' : 'projects'} off track`
					: `${kpis.projectsAttention} need attention`
	);

	const hasDeliveries = $derived(kpis.doneCount > 0 || kpis.openCount > 0);
	const deliveryHeadlinePct = $derived(
		kpis.doneCount > 0 ? kpis.firstTryPassPct : kpis.completionPct
	);
	const deliveryPillTone = $derived<SignalTone>(
		!hasDeliveries ? 'muted' : kpis.firstTryPassPct >= 80 ? 'ok' : 'warn'
	);
	const deliveryPillLabel = $derived(
		!hasDeliveries
			? 'No deliveries yet'
			: kpis.doneCount > 0
				? `${kpis.firstTryPassPct}% clean`
				: `${kpis.openCount} active`
	);
	const medianCycleLabel = $derived(formatCycleDuration(kpis.medianCycleHours));
	const closedDelta = $derived(
		kpis.closedPrior7d !== null ? kpis.closedLast7d - kpis.closedPrior7d : null
	);

	const teamSubLabel = $derived(
		kpis.totalAgents === 0
			? 'Team capacity'
			: `${kpis.totalAgents} ${kpis.totalAgents === 1 ? 'agent' : 'agents'}`
	);

	const finPillHref = $derived(
		kpis.financials.cappedAgent
			? `/members/${encodeURIComponent(kpis.financials.cappedAgent.memberId)}`
			: '/settings/costs'
	);
</script>

<section
	class="kpi-strip grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4"
	aria-label="Executive overview: Projects, Delivery, Team, and Financials"
>
	<!-- Pillar 1: Projects -->
	<HomeKpiCard
		id="hb-pil-projects"
		title="Projects"
		titleHref="/projects"
		pillLabel={projectsPillLabel}
		pillHref="/projects"
		pillTone={projectsPillTone}
		heroHref="/projects"
		heroTitle="{kpis.projectsTotal} active projects: {onTrackProjects} on track, {kpis.atRiskProjects} at risk, {kpis.offTrackProjects} off track"
		subGridClass="grid-cols-[1.35fr_0.9fr] gap-2.5"
		isFirst
	>
		{#snippet icon()}
			<Folder class="size-3.5 text-[var(--exec-accent-ink)]" aria-hidden="true" />
		{/snippet}
		{#snippet hero()}
			<div class="flex items-baseline justify-between gap-2">
				<div class="flex items-baseline gap-1.5">
					<span class="pillar-num">
						{kpis.projectsTotal > 0 ? `${onTrackProjects}/${kpis.projectsTotal}` : '0'}
					</span>
					<span class="text-[0.8125rem] font-medium text-[var(--exec-muted)]">
						{kpis.projectsTotal > 0 ? 'on track' : 'projects'}
					</span>
				</div>
				<span
					class={[
						'text-xs font-semibold tabular-nums',
						kpis.projectsTotal > 0 ? 'text-[var(--exec-dim)]' : 'text-[var(--exec-accent-ink)]'
					]}
				>
					{kpis.projectsTotal > 0 ? `${kpis.projectsTotal} active` : 'Create first'}
				</span>
			</div>
			<div class="bar-track bg-bar-track" aria-hidden="true">
				{#if projectsBarOk > 0}
					<span class="bar-ok" style:width="{projectsBarOk}%"></span>
				{/if}
				{#if projectsBarWarn > 0}
					<span class="bar-warn" style:width="{projectsBarWarn}%"></span>
				{/if}
				{#if projectsBarBad > 0}
					<span class="bar-bad" style:width="{projectsBarBad}%"></span>
				{/if}
			</div>
		{/snippet}
		{#snippet sub()}
			<a
				href={resolve('/projects')}
				class="sub-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title="Portfolio health: {onTrackProjects} on track, {kpis.atRiskProjects} at risk, {kpis.offTrackProjects} off track"
			>
				<span class="sub-lbl truncate">Health</span>
				<span class="truncate text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{#if kpis.projectsTotal === 0}
						—
					{:else}
						<span class="text-[var(--exec-ok)]">{onTrackProjects} ok</span>
						<span class="ml-1 text-xs font-medium text-[var(--exec-warn)]">
							· {kpis.atRiskProjects} risk
						</span>
						<span class="ml-1 text-xs font-medium text-[var(--exec-bad)]">
							· {kpis.offTrackProjects} off
						</span>
					{/if}
				</span>
			</a>
			<a
				href={resolve('/work')}
				class="sub-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title="Tasks currently blocked across active projects"
			>
				<span class="sub-lbl truncate">Blocked</span>
				<span class="truncate text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{#if kpis.projectsTotal === 0 && kpis.blockedCount === 0}
						—
					{:else if kpis.blockedCount > 0}
						{kpis.blockedCount}
						<span class="ml-1 text-xs font-semibold text-[var(--exec-warn)]">blocked</span>
					{:else}
						0
						<span class="ml-1 text-xs font-semibold text-[var(--exec-ok)]">clear</span>
					{/if}
				</span>
			</a>
		{/snippet}
	</HomeKpiCard>

	<!-- Pillar 2: Delivery -->
	<HomeKpiCard
		id="hb-pil-proj"
		title="Delivery"
		titleHref="/insights"
		pillLabel={deliveryPillLabel}
		pillHref="/insights"
		pillTone={deliveryPillTone}
		heroHref="/insights"
		heroTitle="Share of completed items approved on the first try"
	>
		{#snippet icon()}
			<CheckCircle2 class="size-3.5 text-[var(--exec-ok)]" aria-hidden="true" />
		{/snippet}
		{#snippet hero()}
			<div class="flex items-end justify-between gap-2">
				<div class="flex items-baseline gap-1.5">
					<span class="pillar-num">{hasDeliveries ? `${deliveryHeadlinePct}%` : '—'}</span>
					<span class="text-[0.8125rem] font-medium text-[var(--exec-muted)]">
						{kpis.doneCount > 0 ? 'first try' : 'complete'}
					</span>
				</div>
				<HomeThroughputSparkline points={kpis.throughputPoints} />
			</div>
			<div class="bar-track bg-bar-track" aria-hidden="true">
				<span class="bar-ok" style:width="{hasDeliveries ? deliveryHeadlinePct : 0}%"></span>
			</div>
		{/snippet}
		{#snippet sub()}
			<a
				href={resolve('/insights')}
				class="sub-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title="Median time from start to completion"
			>
				<span class="sub-lbl truncate">Median cycle</span>
				<span class="truncate text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{medianCycleLabel}
				</span>
			</a>
			<a
				href={resolve('/insights')}
				class="sub-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title="Items completed in the last 7 days"
			>
				<span class="sub-lbl truncate">Closed · 7d</span>
				<span class="truncate text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{#if !hasDeliveries}
						—
					{:else if kpis.closedLast7d > 0}
						{kpis.closedLast7d}
						{#if closedDelta !== null}
							<span
								class={[
									'ml-1 text-xs font-semibold',
									closedDelta >= 0 ? 'text-[var(--exec-ok)]' : 'text-[var(--exec-warn)]'
								]}
							>
								{closedDelta > 0
									? `+${closedDelta}`
									: closedDelta < 0
										? `−${Math.abs(closedDelta)}`
										: '±0'}
							</span>
						{/if}
					{:else}
						{kpis.doneCount}
						<span class="ml-1 text-xs font-medium text-[var(--exec-muted)]">total</span>
					{/if}
				</span>
			</a>
		{/snippet}
	</HomeKpiCard>

	<!-- Pillar 3: Team -->
	<HomeKpiCard
		id="hb-pil-team"
		title="Team"
		titleHref="/teams/members"
		pillLabel={kpis.officePillLabel}
		pillHref="/settings/offices"
		pillTone={kpis.officePillTone}
		pillTitle={kpis.offlineOffices > 0
			? `Offline: ${kpis.offlineOfficeNames.join(', ')}`
			: 'All connected offices are online'}
		heroHref="/teams/members"
		heroTitle="Share of work running autonomously without needing your input"
		subGridClass="grid-cols-[1.35fr_0.9fr] gap-2.5"
	>
		{#snippet icon()}
			<Users class="size-3.5 text-[var(--exec-accent-ink)]" aria-hidden="true" />
		{/snippet}
		{#snippet hero()}
			<div class="flex items-baseline justify-between gap-2">
				<div class="flex items-baseline gap-1.5">
					<span class="pillar-num">{kpis.totalAgents > 0 ? `${kpis.autonomyPct}%` : '—'}</span>
					<span class="text-[0.8125rem] font-medium text-[var(--exec-muted)]">autonomous</span>
				</div>
				<span class="text-xs font-semibold text-[var(--exec-dim)] tabular-nums">
					{#if kpis.totalAgents > 0}
						{100 - kpis.autonomyPct}% you
					{:else}
						No activity
					{/if}
				</span>
			</div>
			<div class="bar-track bg-bar-track" aria-hidden="true">
				<span class="bar-accent" style:width="{kpis.totalAgents > 0 ? kpis.autonomyPct : 0}%"
				></span>
				{#if kpis.totalAgents > 0 && kpis.autonomyPct < 100}
					<span class="bar-warn" style:width="{100 - kpis.autonomyPct}%"></span>
				{/if}
			</div>
		{/snippet}
		{#snippet sub()}
			<a
				href={resolve('/teams/members')}
				class="sub-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title={kpis.totalAgents > 0
					? `${kpis.totalAgents} agents (${kpis.leadsCount} leads, ${kpis.specialistsCount} specialists): ${kpis.activeAgents} active, ${kpis.pausedAgents} idle, ${kpis.unplacedAgents} off`
					: 'Team capacity'}
			>
				<span class="sub-lbl truncate">{teamSubLabel}</span>
				<span class="truncate text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{#if kpis.totalAgents === 0}
						0 agents
						<span class="ml-1 text-xs font-medium text-[var(--exec-muted)]">
							· {kpis.peopleCount}
							{kpis.peopleCount === 1 ? 'person' : 'people'}
						</span>
					{:else}
						{kpis.activeAgents} active
						{#if kpis.pausedAgents > 0}
							<span class="ml-1 text-xs text-[var(--exec-warn)]">· {kpis.pausedAgents} idle</span>
						{/if}
						{#if kpis.unplacedAgents > 0}
							<span class="ml-1 text-xs text-[var(--exec-bad)]">· {kpis.unplacedAgents} off</span>
						{/if}
					{/if}
				</span>
			</a>
			<a
				href={resolve('/settings/offices')}
				class="sub-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title={kpis.offlineOffices > 0
					? `${kpis.onlineOffices} online · ${kpis.offlineOffices} offline (${kpis.offlineOfficeNames.join(', ')})`
					: `${kpis.onlineOffices} online`}
			>
				<span class="sub-lbl truncate">Offices</span>
				<span class="flex items-baseline gap-1.5 truncate text-sm font-semibold tabular-nums">
					<span class="text-[var(--exec-ok)]">{kpis.onlineOffices} on</span>
					{#if kpis.offlineOffices > 0}
						<span class="text-[var(--exec-dim)]" aria-hidden="true">·</span>
						<span class="text-[var(--exec-bad)]">{kpis.offlineOffices} off</span>
					{/if}
				</span>
			</a>
		{/snippet}
	</HomeKpiCard>

	<!-- Pillar 4: Financials (100% real costsApi + agentBudgets data) -->
	<HomeKpiCard
		id="hb-pil-fin"
		title="Financials"
		titleHref="/settings/costs"
		pillLabel={kpis.financials.pillLabel}
		pillHref={finPillHref}
		pillTone={kpis.financials.pillTone}
		heroHref="/settings/costs"
		isLast
	>
		{#snippet icon()}
			<DollarSign class="size-3.5 text-[var(--exec-warn)]" aria-hidden="true" />
		{/snippet}
		{#snippet hero()}
			<div class="flex items-baseline justify-between gap-2">
				<div class="flex items-baseline gap-1.5">
					<span class="pillar-num">{kpis.financials.spentLabel}</span>
					<span class="text-[0.8125rem] font-medium text-[var(--exec-muted)]">
						{kpis.financials.budgetLabel}
					</span>
				</div>
				{#if kpis.financials.available && kpis.financials.budgetMicroUsd !== null}
					<span class="text-xs font-semibold text-[var(--exec-dim)] tabular-nums">
						{kpis.financials.usagePct}% used
					</span>
				{/if}
			</div>
			<div class="bar-track bg-bar-track" aria-hidden="true">
				<span class="bar-spend" style:width="{kpis.financials.usagePct}%"></span>
			</div>
		{/snippet}
		{#snippet sub()}
			<a
				href={resolve('/insights')}
				class="sub-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title="Average AI spend per completed task"
			>
				<span class="sub-lbl truncate">Cost / task</span>
				<span class="truncate text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{kpis.financials.costPerTaskLabel}
				</span>
			</a>
			<a
				href={resolve('/settings/costs')}
				class="sub-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title="Remaining share of monthly organization budget"
			>
				<span class="sub-lbl truncate">Budget remaining</span>
				<span class="truncate text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{#if kpis.budgetRemainingPct !== null}
						{kpis.budgetRemainingPct}%
						<span class="ml-1 text-xs font-semibold text-[var(--exec-ok)]">left</span>
					{:else if kpis.financials.available}
						No cap set
					{:else}
						Configure
					{/if}
				</span>
			</a>
		{/snippet}
	</HomeKpiCard>
</section>

<style>
	.kpi-strip {
		gap: var(--exec-section-gap);
		padding: var(--exec-kpi-pad-y);
		border-bottom: var(--exec-section-border);
	}
	.sub-link:hover {
		background: var(--exec-accent-soft);
	}
	.pillar-num {
		font-size: var(--exec-kpi-num-size);
		line-height: var(--exec-kpi-num-lh);
		font-weight: var(--exec-kpi-num-weight);
		letter-spacing: var(--exec-kpi-num-tracking);
		color: var(--exec-ink);
		font-variant-numeric: tabular-nums;
	}
	.sub-lbl {
		font-size: var(--exec-kpi-sub-lbl-size);
		font-weight: var(--exec-kpi-sub-lbl-weight);
		text-transform: var(--exec-kpi-sub-lbl-transform);
		letter-spacing: var(--exec-kpi-sub-lbl-tracking);
		color: var(--exec-dim);
	}
	.bar-track {
		display: flex;
		gap: 2px;
		height: var(--exec-bar-height);
		border-radius: var(--exec-bar-radius);
		overflow: hidden;
	}
	.bar-track > span {
		display: block;
		height: 100%;
		border-radius: var(--exec-bar-radius);
	}
	.bar-spend {
		background: var(--exec-bar-budget);
	}
	.bar-accent {
		background: var(--tone-accent-bar);
	}
	.bar-ok {
		background: var(--tone-ok-bar);
	}
	.bar-warn {
		background: var(--tone-warn-bar);
	}
	.bar-bad {
		background: var(--tone-bad-bar);
	}
</style>
