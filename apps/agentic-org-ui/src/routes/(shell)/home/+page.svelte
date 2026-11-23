<script lang="ts">
	import { onDestroy } from 'svelte';
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import CapacityBreakdown from '$lib/modules/home/features/CapacityBreakdown.svelte';
	import DecisionSpotlight from '$lib/modules/home/features/DecisionSpotlight.svelte';
	import ExecutiveBriefCard from '$lib/modules/home/features/ExecutiveBriefCard.svelte';
	import { HomeLiveState } from '$lib/modules/home/features/home-live.svelte';
	import HomeKpiStrip from '$lib/modules/home/features/HomeKpiStrip.svelte';
	import HomeSkeleton from '$lib/modules/home/features/HomeSkeleton.svelte';
	import ProjectScorecard from '$lib/modules/home/features/ProjectScorecard.svelte';
	import { layerOffices, officesApi } from '$lib/modules/offices';
	import {
		deriveCatchUpDigest,
		deriveDecisionSpotlight,
		deriveExecutiveBrief,
		deriveKpiMetrics,
		deriveProjectCards,
		deriveTaskTimeBreakdown,
		greetingForHour,
		type HomeDomainData
	} from '$lib/home/model';
	import { pageTitle } from '$lib/nav';
	import { owner } from '$lib/org/model';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const ownerMember = $derived(owner(snapshot));
	const ownerId = $derived(ownerMember?.id ?? 'owner');

	const live = new HomeLiveState({
		overview: (id) => officesApi.overview(id),
		layer: layerOffices
	});

	$effect(() => live.connect(organizationId, data.connection.organization.offices));

	onDestroy(() => {
		live.destroy();
	});

	const activeOffices = $derived(
		live.offices.length > 0 ? live.offices : data.connection.organization.offices
	);
	const coreReady = $derived(
		live.items !== undefined || (live.inboxLoaded && live.inboxItems.length > 0)
	);
	const loading = $derived(!coreReady && live.loadError === undefined);
	const workItems = $derived(live.items ?? []);
	const domain = $derived<HomeDomainData>({
		orgCost: live.orgCost,
		agentBudgets: live.agentBudgets,
		inboxItems: live.inboxItems,
		memberRows: live.memberRows,
		projectRows: live.projectRows,
		homeProjects: live.homeProjects,
		activityRows: live.activityRows
	});

	const kpis = $derived(deriveKpiMetrics(snapshot, workItems, activeOffices, ownerId, domain));
	const spotlight = $derived(
		deriveDecisionSpotlight(workItems, snapshot, ownerId, live.nowMs, live.inboxItems)
	);
	const projectCards = $derived(deriveProjectCards(snapshot, workItems, domain));
	const catchUp = $derived(deriveCatchUpDigest(workItems, snapshot, live.nowMs, live.activityRows));
	const brief = $derived(
		deriveExecutiveBrief(
			snapshot,
			workItems,
			activeOffices,
			ownerId,
			data.connection.organization.name,
			live.nowMs,
			domain
		)
	);
	const taskTime = $derived(
		deriveTaskTimeBreakdown(workItems, ownerId, kpis.offlineOffices, domain)
	);
	const greeting = $derived(greetingForHour(new Date(live.nowMs).getHours()));
	const isDayOne = $derived(
		live.items !== undefined &&
			live.inboxLoaded &&
			!live.loadError &&
			workItems.length === 0 &&
			live.inboxItems.length === 0 &&
			projectCards.length === 0 &&
			kpis.totalAgents === 0
	);

	function refreshLive(): Promise<void> {
		return live.load(organizationId, data.connection.organization.offices);
	}
</script>

<svelte:head><title>{pageTitle('Home')}</title></svelte:head>

<div class="home-page mx-auto flex w-full flex-col px-6 pt-3 pb-12 sm:px-8">
	<!-- Zone 1: Executive Greeting Header -->
	<section class="home-hero flex flex-wrap justify-between gap-4">
		<h1 id="shell-page-title" tabindex="-1" class="home-h1 focus:outline-none">
			{#if live.loadError}
				{greeting} <em class="tone-bad">Office unreachable.</em>
			{:else if loading}
				{greeting}
			{:else if isDayOne}
				Welcome to {data.connection.organization.name}. <em class="tone-calm">Ready to launch.</em>
			{:else if spotlight.length > 0}
				{greeting}
				<em class={spotlight.some((s) => s.urgent) ? 'tone-warn' : 'tone-calm'}>
					{spotlight.length}
					{spotlight.length === 1 ? 'decision' : 'decisions'}
				</em>
				{spotlight.length === 1 ? 'needs' : 'need'} you.
			{:else}
				{greeting} <em class="tone-ok">All clear</em> across your organization.
			{/if}
		</h1>
		<a href={resolve('/work')} class="exec-cta exec-cta-glass">
			<span class="exec-cta-fill" aria-hidden="true"></span>
			<span class="exec-cta-sheen" aria-hidden="true"></span>
			<span class="exec-cta-inner">
				<Plus class="exec-cta-ico size-4" aria-hidden="true" />
				<span>New task</span>
			</span>
		</a>
	</section>

	{#if live.loadError}
		<section class="exec-card flex flex-col items-center gap-3 px-6 py-12 text-center" role="alert">
			<span class="err-ring flex size-12 items-center justify-center rounded-full">
				<AlertTriangle class="size-6" aria-hidden="true" />
			</span>
			<h2 class="text-base font-semibold text-[var(--exec-ink)]">Couldn't reach live work data</h2>
			<p class="max-w-md text-sm text-[var(--exec-muted)]">{live.loadError}</p>
			<button type="button" onclick={() => void refreshLive()} class="exec-cta mt-2">
				<span class="exec-cta-fill" aria-hidden="true"></span>
				<span class="exec-cta-inner">
					<RefreshCw class="size-4" aria-hidden="true" />
					<span>Try again</span>
				</span>
			</button>
		</section>
	{:else if loading}
		<HomeSkeleton />
	{:else}
		<HomeKpiStrip {kpis} />

		<section class="home-stage grid grid-cols-1 xl:grid-cols-[minmax(0,1.42fr)_minmax(340px,1fr)]">
			<DecisionSpotlight
				{spotlight}
				{organizationId}
				{snapshot}
				{isDayOne}
				onRefresh={refreshLive}
			/>
			<ExecutiveBriefCard {brief} {catchUp} />
		</section>

		{#if projectCards.length > 0}
			<ProjectScorecard projects={projectCards} attentionCount={kpis.projectsAttention} />
		{/if}

		{#if taskTime.totalOpen > 0}
			<CapacityBreakdown {taskTime} offlineOffices={kpis.offlineOffices} />
		{/if}
	{/if}
</div>

<style>
	.home-page {
		max-width: var(--exec-page-max-w);
		gap: var(--exec-page-gap);
	}
	.home-hero {
		align-items: var(--exec-hero-align);
		padding-bottom: var(--exec-hero-pad-b);
		border-bottom: var(--exec-hero-border);
	}
	.home-h1 {
		font-size: clamp(1.375rem, 2.2vw, 1.875rem);
		line-height: 1.22;
		font-weight: var(--exec-h1-weight);
		letter-spacing: var(--exec-h1-tracking);
		color: var(--exec-ink);
	}
	.home-h1 em {
		font-style: normal;
		text-decoration: var(--exec-h1-em-decoration);
		text-decoration-thickness: 2.5px;
		text-underline-offset: 5px;
	}
	.home-h1 em.tone-warn {
		color: var(--exec-h1-em-warn);
		text-decoration-color: var(--exec-h1-em-ul-warn);
	}
	.home-h1 em.tone-calm {
		color: var(--exec-h1-em-calm);
		text-decoration-color: var(--exec-h1-em-ul-calm);
	}
	.home-h1 em.tone-ok {
		color: var(--exec-h1-em-ok);
		text-decoration-color: var(--exec-h1-em-ul-ok);
	}
	.home-h1 em.tone-bad {
		color: var(--exec-h1-em-bad);
		text-decoration-color: var(--exec-h1-em-ul-bad);
	}
	.home-stage {
		gap: var(--exec-stage-gap);
		padding: var(--exec-section-pad-y);
		border-bottom: var(--exec-section-border);
	}
	.err-ring {
		background: var(--exec-bad-bg);
		box-shadow: inset 0 0 0 1px var(--exec-bad-ring);
		color: var(--exec-bad);
	}
</style>
