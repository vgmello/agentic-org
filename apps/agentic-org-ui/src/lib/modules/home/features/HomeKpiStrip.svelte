<script lang="ts">
	import { resolve } from '$app/paths';
	import Activity from '@lucide/svelte/icons/activity';
	import DollarSign from '@lucide/svelte/icons/dollar-sign';
	import Users from '@lucide/svelte/icons/users';
	import type { ExecutiveKpis, SignalTone } from '$lib/home/model';

	interface Props {
		kpis: ExecutiveKpis;
	}

	let { kpis }: Props = $props();

	function pillToneClass(tone: SignalTone): string {
		if (tone === 'bad') return 'is-bad';
		if (tone === 'warn') return 'is-warn';
		if (tone === 'muted') return 'is-muted';
		return 'is-ok';
	}
</script>

<section
	class="kpi-strip grid grid-cols-1 lg:grid-cols-3"
	aria-label="Executive overview: Financials, Workforce, and Work & Flow"
>
	<!-- Pillar 1: Financials / Monthly Spend & Budget -->
	<article class="kpi-pillar flex flex-col gap-3">
		<div class="flex items-center justify-between gap-2">
			<a href={resolve('/settings/costs')} class="pillar-name inline-flex items-center gap-1.5">
				<DollarSign class="size-3.5 text-[var(--exec-warn)]" aria-hidden="true" />
				<span>Financials</span>
			</a>
			<a
				href={resolve('/settings/costs')}
				class={['status-pill', pillToneClass(kpis.financials.pillTone)]}
			>
				{kpis.financials.pillLabel}
			</a>
		</div>
		<a
			href={resolve('/settings/costs')}
			class="pillar-link -m-1 flex flex-col gap-2 rounded-lg p-1"
		>
			<div class="flex items-baseline justify-between gap-2">
				<div class="flex items-baseline gap-1.5">
					<span class="pillar-num">{kpis.financials.spentLabel}</span>
					<span class="text-xs font-medium text-[var(--exec-muted)]">
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
				<span class="bar-accent" style:width="{kpis.financials.usagePct}%"></span>
			</div>
		</a>
		<div class="pillar-sub grid grid-cols-2 gap-3 pt-2.5">
			<a
				href={resolve('/settings/costs')}
				class="pillar-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
			>
				<span class="text-[11px] font-medium text-[var(--exec-dim)]">Cost / completed</span>
				<span class="text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{kpis.financials.costPerTaskLabel}
				</span>
			</a>
			<a
				href={resolve('/settings/costs')}
				class="pillar-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
			>
				<span class="text-[11px] font-medium text-[var(--exec-dim)]">Agent budgets</span>
				<span class="text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{#if kpis.financials.available}
						{kpis.financials.budgetedAgentsCount} active
					{:else}
						Configure
					{/if}
				</span>
			</a>
		</div>
	</article>

	<!-- Pillar 2: Workforce & Offices -->
	<article class="kpi-pillar flex flex-col gap-3">
		<div class="flex items-center justify-between gap-2">
			<a href={resolve('/teams/members')} class="pillar-name inline-flex items-center gap-1.5">
				<Users class="size-3.5 text-[var(--exec-accent-ink)]" aria-hidden="true" />
				<span>Workforce &amp; Offices</span>
			</a>
			<a
				href={resolve('/settings/offices')}
				class={['status-pill', pillToneClass(kpis.officePillTone)]}
				title={kpis.offlineOffices > 0
					? `Offline: ${kpis.offlineOfficeNames.join(', ')}`
					: 'All offices online'}
			>
				{kpis.officePillLabel}
			</a>
		</div>
		<a href={resolve('/teams/members')} class="pillar-link -m-1 flex flex-col gap-2 rounded-lg p-1">
			<div class="flex items-baseline justify-between gap-2">
				<div class="flex items-baseline gap-1.5">
					<span class="pillar-num">{kpis.totalAgents > 0 ? `${kpis.autonomyPct}%` : '—'}</span>
					<span class="text-xs font-medium text-[var(--exec-muted)]">autonomous</span>
				</div>
				<span class="text-xs font-semibold text-[var(--exec-dim)] tabular-nums">
					{kpis.peopleCount}
					{kpis.peopleCount === 1 ? 'person' : 'people'} · {kpis.totalAgents}
					agents
				</span>
			</div>
			<div class="bar-track bg-bar-track" aria-hidden="true">
				<span class="bar-accent" style:width="{kpis.autonomyPct}%"></span>
				{#if kpis.totalAgents > 0 && kpis.autonomyPct < 100}
					<span class="bar-warn" style:width="{100 - kpis.autonomyPct}%"></span>
				{/if}
			</div>
		</a>
		<div class="pillar-sub grid grid-cols-[1.22fr_1fr] gap-2.5 pt-2.5">
			<a
				href={resolve('/teams/members')}
				class="pillar-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
			>
				<span class="truncate text-[11px] font-medium text-[var(--exec-dim)]">Agents</span>
				<span class="truncate text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{kpis.activeAgents} active
					{#if kpis.pausedAgents > 0}
						<span class="ml-1 text-[11px] text-[var(--exec-warn)]">
							· {kpis.pausedAgents} idle
						</span>
					{/if}
					{#if kpis.unplacedAgents > 0}
						<span class="ml-1 text-[11px] text-[var(--exec-bad)]">
							· {kpis.unplacedAgents} off
						</span>
					{/if}
				</span>
			</a>
			<a
				href={resolve('/settings/offices')}
				class="pillar-link -m-1 flex flex-col gap-0.5 rounded-md p-1"
				title={kpis.offlineOffices > 0
					? `${kpis.onlineOffices} online · ${kpis.offlineOffices} offline (${kpis.offlineOfficeNames.join(', ')})`
					: `${kpis.onlineOffices} online`}
			>
				<span class="truncate text-[11px] font-medium text-[var(--exec-dim)]">Offices</span>
				<span class="flex items-baseline gap-1.5 truncate text-[13px] font-semibold tabular-nums">
					<span class="text-[var(--exec-ok)]">{kpis.onlineOffices} online</span>
					{#if kpis.offlineOffices > 0}
						<span class="text-[var(--exec-dim)]" aria-hidden="true">·</span>
						<span class="text-[var(--exec-bad)]">{kpis.offlineOffices} offline</span>
					{/if}
				</span>
			</a>
		</div>
	</article>

	<!-- Pillar 3: Work & Flow / Delivery -->
	<article class="kpi-pillar flex flex-col gap-3">
		<div class="flex items-center justify-between gap-2">
			<a href={resolve('/work')} class="pillar-name inline-flex items-center gap-1.5">
				<Activity class="size-3.5 text-[var(--exec-ok)]" aria-hidden="true" />
				<span>Work &amp; Flow</span>
			</a>
			<a
				href={resolve('/projects')}
				class={['status-pill', kpis.projectsAttention > 0 ? 'is-warn' : 'is-ok']}
			>
				{kpis.projectsAttention > 0
					? `${kpis.projectsAttention} need attention`
					: kpis.projectsTotal > 0
						? 'All on track'
						: 'No projects yet'}
			</a>
		</div>
		<a href={resolve('/work')} class="pillar-link -m-1 flex flex-col gap-2 rounded-lg p-1">
			<div class="flex items-baseline justify-between gap-2">
				<div class="flex items-baseline gap-1.5">
					<span class="pillar-num">
						{kpis.doneCount > 0 ? `${kpis.firstTryPassPct}%` : `${kpis.completionPct}%`}
					</span>
					<span class="text-xs font-medium text-[var(--exec-muted)]">
						{kpis.doneCount > 0 ? 'clean delivery' : 'complete'}
					</span>
				</div>
				<span class="text-xs font-semibold text-[var(--exec-dim)] tabular-nums">
					{kpis.openCount} open · {kpis.doneCount} done
				</span>
			</div>
			<div class="bar-track bg-bar-track" aria-hidden="true">
				<span
					class="bar-ok"
					style:width="{kpis.doneCount > 0 ? kpis.firstTryPassPct : kpis.completionPct}%"
				></span>
			</div>
		</a>
		<div class="pillar-sub grid grid-cols-2 gap-3 pt-2.5">
			<a href={resolve('/work')} class="pillar-link -m-1 flex flex-col gap-0.5 rounded-md p-1">
				<span class="text-[11px] font-medium text-[var(--exec-dim)]">Progress</span>
				<span class="text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{kpis.completionPct}%
					<span class="ml-1 text-[11px] font-semibold text-[var(--exec-ok)]">
						{kpis.doneCount} done
					</span>
				</span>
			</a>
			<a href={resolve('/inbox')} class="pillar-link -m-1 flex flex-col gap-0.5 rounded-md p-1">
				<span class="text-[11px] font-medium text-[var(--exec-dim)]">Blocked</span>
				<span class="text-sm font-semibold text-[var(--exec-ink)] tabular-nums">
					{kpis.blockedCount}
					<span
						class={[
							'ml-1 text-[11px] font-semibold',
							kpis.blockedCount > 0 ? 'text-[var(--exec-bad)]' : 'text-[var(--exec-ok)]'
						]}
					>
						{kpis.blockedCount > 0 ? 'blocked' : 'clear'}
					</span>
				</span>
			</a>
		</div>
	</article>
</section>

<style>
	.kpi-strip {
		gap: var(--exec-section-gap);
		padding: var(--exec-kpi-pad-y);
		border-bottom: var(--exec-section-border);
	}
	.kpi-pillar {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: var(--exec-pillar-pad-sm);
	}
	.kpi-pillar:hover {
		background: var(--exec-panel-bg-hover);
		box-shadow: var(--exec-panel-shadow-hover);
	}
	.kpi-pillar + .kpi-pillar {
		border-top: var(--exec-col-divider);
	}
	@media (min-width: 1024px) {
		.kpi-pillar {
			padding: var(--exec-pillar-pad);
		}
		.kpi-pillar:first-child {
			padding-left: var(--exec-pillar-edge-pad);
		}
		.kpi-pillar:last-child {
			padding-right: var(--exec-pillar-edge-pad);
		}
		.kpi-pillar + .kpi-pillar {
			border-top: 0;
			border-left: var(--exec-col-divider);
		}
	}
	.pillar-name {
		font-size: 0.7rem;
		line-height: 1rem;
		font-weight: 650;
		letter-spacing: 0.11em;
		text-transform: uppercase;
		color: var(--exec-pillar-name-ink);
		transition: color 0.14s ease;
	}
	.pillar-name:hover {
		color: var(--exec-ink);
	}
	.status-pill {
		padding: 2px 8px;
		border-radius: var(--exec-pill-radius);
		font-size: 0.7rem;
		font-weight: 600;
		white-space: nowrap;
	}
	.status-pill.is-ok {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: var(--exec-pill-ring-ok);
	}
	.status-pill.is-warn {
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: var(--exec-pill-ring-warn);
	}
	.status-pill.is-bad {
		background: var(--exec-bad-bg);
		color: var(--exec-bad);
		box-shadow: var(--exec-pill-ring-bad);
	}
	.status-pill.is-muted {
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: var(--exec-pill-ring-muted);
	}
	.pillar-link:hover {
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
	.bar-accent {
		background: var(--tone-accent-bar);
	}
	.bar-ok {
		background: var(--tone-ok-bar);
	}
	.bar-warn {
		background: var(--tone-warn-bar);
	}
	.pillar-sub {
		border-top: 1px solid var(--exec-border);
	}
</style>
