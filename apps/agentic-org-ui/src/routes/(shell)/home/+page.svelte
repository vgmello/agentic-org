<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import CapacityBreakdown from '$lib/modules/home/features/CapacityBreakdown.svelte';
	import DecisionSpotlight from '$lib/modules/home/features/DecisionSpotlight.svelte';
	import ExecutiveBriefCard from '$lib/modules/home/features/ExecutiveBriefCard.svelte';
	import { HomeLiveState } from '$lib/modules/home/features/home-live.svelte';
	import { fitBelowFold } from '$lib/modules/home/features/home-fold';
	import HomeKpiStrip from '$lib/modules/home/features/HomeKpiStrip.svelte';
	import HomeMilestones from '$lib/modules/home/features/HomeMilestones.svelte';
	import HomeSkeleton from '$lib/modules/home/features/HomeSkeleton.svelte';
	import ProjectScorecard from '$lib/modules/home/features/ProjectScorecard.svelte';
	import { officesApi } from '$lib/modules/offices/api';
	import { layerOffices } from '$lib/modules/offices/layer';
	import { useTessPanel } from '$lib/modules/tess/tess-panel.svelte';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import {
		deriveCatchUpDigest,
		deriveDecisionSpotlight,
		deriveExecutiveBrief,
		deriveKpiMetrics,
		deriveProjectCards,
		deriveTaskTimeBreakdown,
		deriveUpcomingMilestones,
		greetingForHour,
		type HomeDomainData,
		type HomeInboxState
	} from '$lib/home/model';
	import { pageTitle } from '$lib/nav';
	import { owner } from '$lib/modules/organization/model';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const ownerMember = $derived(owner(snapshot));
	const ownerId = $derived(ownerMember?.id ?? 'owner');
	const shellData = useShellData();
	const tessPanel = useTessPanel();

	const live = new HomeLiveState({
		overview: (id) => officesApi.overview(id),
		layer: layerOffices
	});

	let spotlightRef = $state<{ jumpToDecision: (id: string) => boolean } | null>(null);

	$effect(() => {
		const id = organizationId;
		const baseOffices = data.connection.organization.offices;
		return untrack(() => {
			const seed =
				shellData.loaded &&
				shellData.loadedOrganizationId === id &&
				shellData.workError === undefined
					? {
							items: shellData.workItems,
							inboxItems:
								shellData.inboxLoadedOrganizationId === id ? shellData.inboxItems : undefined,
							offices: shellData.offices
						}
					: undefined;
			return live.connect(id, baseOffices, seed);
		});
	});

	onDestroy(() => {
		live.destroy();
	});

	const liveForOrg = $derived(live.loadedOrganizationId === organizationId);
	const liveItems = $derived(liveForOrg ? live.items : undefined);
	const liveWorkFromSeed = $derived(liveForOrg ? live.workFromSeed : false);
	const liveRetryingWork = $derived(liveForOrg ? live.retryingWork : false);
	const liveAllowShellFallback = $derived(liveForOrg ? live.allowShellFallback : true);
	const liveInboxLoaded = $derived(liveForOrg ? live.inboxLoaded : false);
	const liveInboxSettled = $derived(liveForOrg ? live.inboxSettled : false);
	const liveInboxItems = $derived(liveForOrg ? live.inboxItems : []);
	const liveOffices = $derived(liveForOrg ? live.offices : []);
	const liveLoadError = $derived(liveForOrg ? live.loadError : undefined);

	const shellSeedReady = $derived(
		shellData.loaded && shellData.loadedOrganizationId === organizationId
	);
	const shellWorkFailed = $derived(shellSeedReady && shellData.workError !== undefined);
	const shellWorkHydrationAllowed = $derived(
		shellSeedReady && !shellWorkFailed && liveAllowShellFallback
	);
	const hydratedItems = $derived(
		liveItems ?? (shellWorkHydrationAllowed ? shellData.workItems : undefined)
	);
	const usableWorkItems = $derived(
		liveRetryingWork || (liveWorkFromSeed && shellWorkFailed) ? undefined : hydratedItems
	);
	const shellInboxReady = $derived(shellData.inboxLoadedOrganizationId === organizationId);
	const hydratedInboxLoaded = $derived(liveInboxLoaded || shellInboxReady);
	const inboxState = $derived<HomeInboxState>(
		hydratedInboxLoaded ? 'ready' : liveInboxSettled ? 'unavailable' : 'loading'
	);
	const hydratedInboxItems = $derived(
		liveInboxLoaded ? liveInboxItems : shellInboxReady ? shellData.inboxItems : []
	);
	const activeOffices = $derived(
		liveOffices.length > 0
			? liveOffices
			: shellSeedReady && shellData.offices.length > 0
				? shellData.offices
				: data.connection.organization.offices
	);
	const hasOpenLiveInbox = $derived(liveInboxItems.some((item) => item.state === 'open'));
	const coreReady = $derived(
		usableWorkItems !== undefined ||
			(liveInboxLoaded &&
				hasOpenLiveInbox &&
				liveLoadError === undefined &&
				!shellWorkFailed &&
				!liveRetryingWork)
	);
	const loading = $derived(!coreReady && liveLoadError === undefined);
	const workItems = $derived(usableWorkItems ?? []);
	const domain = $derived<HomeDomainData>({
		orgCost: liveForOrg ? live.orgCost : undefined,
		agentBudgets: liveForOrg ? live.agentBudgets : undefined,
		inboxItems: hydratedInboxItems,
		inboxState,
		memberRows: liveForOrg ? live.memberRows : undefined,
		projectRows: liveForOrg ? live.projectRows : undefined,
		homeProjects: liveForOrg ? live.homeProjects : undefined,
		activityRows: liveForOrg ? live.activityRows : undefined
	});

	$effect(() => {
		const nextDomain = domain;
		tessPanel?.setDomainContext(nextDomain);
		return () => {
			tessPanel?.setDomainContext(undefined);
		};
	});

	const kpis = $derived(deriveKpiMetrics(snapshot, workItems, activeOffices, ownerId, domain));
	const spotlight = $derived(
		deriveDecisionSpotlight(workItems, snapshot, ownerId, live.nowMs, hydratedInboxItems)
	);
	const spotlightIds = $derived(new Set(spotlight.map((d) => d.id)));
	const urgentSpotlightCount = $derived(spotlight.filter((d) => d.urgent).length);
	const projectCards = $derived(deriveProjectCards(snapshot, workItems, domain, live.nowMs));
	const milestones = $derived(deriveUpcomingMilestones(snapshot, workItems, live.nowMs, domain));
	const catchUp = $derived(
		deriveCatchUpDigest(workItems, snapshot, live.nowMs, liveForOrg ? live.activityRows : undefined)
	);
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
		usableWorkItems !== undefined &&
			hydratedInboxLoaded &&
			!liveLoadError &&
			workItems.length === 0 &&
			hydratedInboxItems.length === 0 &&
			projectCards.length === 0 &&
			kpis.totalAgents === 0
	);

	function refreshLive(): Promise<void> {
		return live.load(organizationId, data.connection.organization.offices);
	}

	function handleSpotJump(decisionId: string) {
		spotlightRef?.jumpToDecision(decisionId);
	}
</script>

<svelte:head><title>{pageTitle('Home')}</title></svelte:head>

<div
	{@attach fitBelowFold(uiPrefs.theme)}
	class="home-page mx-auto flex w-full flex-col px-6 pt-3 pb-12 sm:px-8"
>
	<!-- Zone 1: Executive Greeting Header -->
	<section class="home-hero flex flex-wrap justify-between gap-4">
		<div class="home-brief flex min-w-0 flex-col gap-1">
			<h1 id="shell-page-title" tabindex="-1" class="home-h1 focus:outline-none">
				{#if liveLoadError}
					{greeting} <em class="tone-bad">Office unreachable.</em>
				{:else if loading}
					{greeting} Preparing your briefing…
				{:else if isDayOne}
					{greeting} <em class="tone-calm">Welcome</em> to {data.connection.organization.name}.
				{:else if urgentSpotlightCount > 0}
					{greeting}
					<em class="tone-warn">
						{urgentSpotlightCount}
						{urgentSpotlightCount === 1 ? 'time-sensitive decision' : 'time-sensitive decisions'}
					</em>
					{urgentSpotlightCount === 1 ? 'needs' : 'need'} you.
				{:else if spotlight.length > 0}
					{greeting}
					<em class="tone-calm">
						{spotlight.length}
						{spotlight.length === 1 ? 'item' : 'items'}
					</em>
					waiting when you're ready.
				{:else if usableWorkItems !== undefined && hydratedInboxLoaded}
					{greeting} <em class="tone-ok">All clear</em> across your organization.
				{:else}
					{greeting}
				{/if}
			</h1>
		</div>
	</section>

	{#if liveLoadError}
		<section class="exec-card flex flex-col items-center gap-3 px-6 py-12 text-center" role="alert">
			<span class="err-ring flex size-12 items-center justify-center rounded-full">
				<AlertTriangle class="size-6" aria-hidden="true" />
			</span>
			<h2 class="text-base font-semibold text-[var(--exec-ink)]">Couldn't reach live work data</h2>
			<p class="max-w-md text-sm text-[var(--exec-muted)]">{liveLoadError}</p>
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
				bind:this={spotlightRef}
				{spotlight}
				{organizationId}
				{snapshot}
				{isDayOne}
				{inboxState}
				onRefresh={refreshLive}
			/>
			<ExecutiveBriefCard {brief} {catchUp} />
		</section>

		{#if projectCards.length > 0 || milestones.length > 0 || taskTime.totalOpen > 0}
			<div class="home-below-fold flex flex-col">
				{#if projectCards.length > 0}
					<ProjectScorecard
						projects={projectCards}
						attentionCount={kpis.projectsAttention}
						{spotlightIds}
						onSpotJump={handleSpotJump}
					/>
				{/if}

				{#if milestones.length > 0 || projectCards.length > 0}
					<HomeMilestones {milestones} {spotlightIds} onSpotJump={handleSpotJump} />
				{/if}

				{#if taskTime.totalOpen > 0}
					<CapacityBreakdown {taskTime} offlineOffices={kpis.offlineOffices} />
				{/if}
			</div>
		{/if}
	{/if}
</div>

<style>
	.home-page {
		max-width: var(--exec-page-max-w);
		gap: var(--exec-page-gap);
	}
	.home-below-fold {
		gap: var(--exec-page-gap);
	}
	.home-hero {
		align-items: var(--exec-hero-align);
		padding-bottom: var(--exec-hero-pad-b);
		border-bottom: var(--exec-hero-border);
	}
	.home-h1 {
		font-size: var(--exec-h1-size);
		line-height: 1.2;
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
