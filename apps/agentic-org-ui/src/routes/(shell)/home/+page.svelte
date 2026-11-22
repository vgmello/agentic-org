<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Activity from '@lucide/svelte/icons/activity';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Ban from '@lucide/svelte/icons/ban';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import Info from '@lucide/svelte/icons/info';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Scale from '@lucide/svelte/icons/scale';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Users from '@lucide/svelte/icons/users';
	import XCircle from '@lucide/svelte/icons/x-circle';
	import Zap from '@lucide/svelte/icons/zap';
	import type { OfficeInfo, WorkItemSummary } from '$lib/proto';
	import { executeDecisionAction } from '$lib/home/actions';
	import {
		deriveCatchUpDigest,
		deriveDecisionSpotlight,
		deriveKpiMetrics,
		deriveProjectCards,
		deriveTaskTimeBreakdown,
		greetingForHour
	} from '$lib/home/model';
	import HomeView from '$lib/modules/home/features/view/HomeView.svelte';
	import { pageTitle } from '$lib/nav';
	import { officeApi, toAppError } from '$lib/office/api';
	import { owner } from '$lib/org/model';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';
	import { workApi } from '$lib/work';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const ownerMember = $derived(owner(snapshot));
	const ownerId = $derived(ownerMember?.id ?? 'owner');

	let items = $state.raw<WorkItemSummary[] | undefined>(undefined);
	let offices = $state.raw<OfficeInfo[]>([]);
	let loadError = $state<string | undefined>(undefined);
	let busyId = $state<string | undefined>(undefined);
	let spotIdx = $state(0);
	let decliningId = $state<string | undefined>(undefined);
	let declineReason = $state('');
	let nowMs = $state(Date.now());

	const activeOffices = $derived(
		offices.length > 0 ? offices : data.connection.organization.offices
	);
	const loading = $derived(items === undefined && loadError === undefined);
	const workItems = $derived(items ?? []);

	const kpis = $derived(deriveKpiMetrics(snapshot, workItems, activeOffices, ownerId));
	const spotlight = $derived(deriveDecisionSpotlight(workItems, snapshot, ownerId, nowMs));
	const safeIdx = $derived(spotlight.length > 0 ? Math.min(spotIdx, spotlight.length - 1) : 0);
	const currentDecision = $derived(spotlight[safeIdx]);
	const nextDecision = $derived(
		spotlight.length > 1 ? spotlight[(safeIdx + 1) % spotlight.length] : undefined
	);
	const projectCards = $derived(deriveProjectCards(snapshot, workItems));
	const catchUp = $derived(deriveCatchUpDigest(workItems, snapshot, nowMs));
	const taskTime = $derived(deriveTaskTimeBreakdown(workItems, ownerId, kpis.offlineOffices));
	const greeting = $derived(greetingForHour(new Date(nowMs).getHours()));
	const isDayOne = $derived(
		!loading && !loadError && workItems.length === 0 && projectCards.length === 0 && kpis.totalAgents === 0
	);

	async function loadLive() {
		loadError = undefined;
		try {
			const [workRes, orgsRes] = await Promise.allSettled([
				workApi.snapshot(organizationId),
				officeApi.peekOrganizations()
			]);
			if (workRes.status === 'fulfilled') {
				items = workRes.value.items;
			} else {
				const err = toAppError(workRes.reason);
				// Graceful empty when work endpoint is unavailable on a fresh office
				if (err.source === 'api' && (err.code === 'unsupported' || err.code === 'unavailable')) {
					items = [];
				} else {
					loadError = err.message;
				}
			}
			if (orgsRes.status === 'fulfilled') {
				const found = orgsRes.value.find((o) => o.id === organizationId);
				if (found && found.offices.length > 0) offices = found.offices;
			}
			nowMs = Date.now();
		} catch (e) {
			loadError = toAppError(e).message;
		}
	}

	onMount(() => {
		if (uiPrefs.executiveUi) void loadLive();
	});

	async function runAction(kind: 'primary' | 'secondary') {
		if (!currentDecision || busyId) return;
		if (kind === 'secondary' && currentDecision.secondaryAction?.kind === 'decline_request' && decliningId !== currentDecision.id) {
			decliningId = currentDecision.id;
			declineReason = '';
			return;
		}
		busyId = currentDecision.id;
		try {
			const res = await executeDecisionAction(
				organizationId,
				currentDecision,
				kind,
				snapshot,
				declineReason
			);
			decliningId = undefined;
			declineReason = '';
			if (res.navigateTo) {
				await goto(resolve(res.navigateTo as '/work'));
				return;
			}
			if (res.message) toast.success(res.message);
			await loadLive();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			busyId = undefined;
		}
	}
</script>

<svelte:head><title>{pageTitle('Home')}</title></svelte:head>

{#if uiPrefs.executiveUi}
<div class="hb-page mx-auto flex w-full max-w-[1260px] flex-col gap-6 px-6 pt-3 pb-12 sm:px-8">
	<!-- Zone 1: Executive Greeting Header -->
	<section class="flex flex-wrap items-center justify-between gap-4">
		<h1 id="shell-page-title" tabindex="-1" class="hb-h1 focus:outline-none">
			{#if loadError}
				{greeting} <em class="tone-bad">Office unreachable.</em>
			{:else if loading}
				{greeting}
			{:else if isDayOne}
				Welcome to {data.connection.organization.name}. <em class="tone-calm">Ready to launch.</em>
			{:else if spotlight.length > 0}
				{greeting}
				<em class={spotlight.some((s) => s.urgent) ? 'tone-warn' : 'tone-calm'}>
					{spotlight.length} {spotlight.length === 1 ? 'decision' : 'decisions'}
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
				<Plus class="size-4" aria-hidden="true" />
				<span>New task</span>
			</span>
		</a>
	</section>

	{#if loadError}
		<!-- Error state -->
		<section class="exec-card flex flex-col items-center gap-3 px-6 py-12 text-center" role="alert">
			<span class="hb-ring is-bad flex size-12 items-center justify-center rounded-full">
				<AlertTriangle class="size-6" aria-hidden="true" />
			</span>
			<h2 class="text-base font-semibold" style="color: var(--exec-ink);">Couldn't reach live work data</h2>
			<p class="max-w-md text-sm" style="color: var(--exec-muted);">{loadError}</p>
			<button type="button" onclick={() => void loadLive()} class="exec-cta mt-2">
				<span class="exec-cta-fill" aria-hidden="true"></span>
				<span class="exec-cta-inner">
					<RefreshCw class="size-4" aria-hidden="true" />
					<span>Try again</span>
				</span>
			</button>
		</section>
	{:else if loading}
		<!-- 3-Column Skeletons -->
		<div class="grid grid-cols-1 gap-4 md:grid-cols-3" aria-busy="true" aria-label="Loading executive overview">
			{#each [1, 2, 3] as n (n)}
				<div class="exec-card flex flex-col gap-3 p-5">
					<div class="hb-skel h-3 w-24 rounded"></div>
					<div class="hb-skel h-7 w-36 rounded"></div>
					<div class="hb-skel h-1.5 w-full rounded-full"></div>
					<div class="hb-skel h-3 w-44 rounded"></div>
				</div>
			{/each}
		</div>
	{:else}
		<!-- Zone 1b: 3 Executive KPI Pillars -->
		<section class="grid grid-cols-1 gap-4 lg:grid-cols-3" aria-label="Executive overview: Work, Team, and Delivery">
			<!-- Pillar 1: Work & Flow -->
			<article class="exec-card flex flex-col gap-3 p-4.5">
				<div class="flex items-center justify-between gap-2">
					<a href={resolve('/work')} class="hb-pil-name inline-flex items-center gap-1.5">
						<Activity class="size-3.5" style="color: var(--exec-sky);" aria-hidden="true" />
						<span>Work &amp; Flow</span>
					</a>
					<a href={resolve('/inbox')} class={['hb-pil-pill', kpis.urgentCount > 0 ? 'is-warn' : 'is-ok']}>
						{kpis.urgentCount > 0 ? `${kpis.urgentCount} need you` : 'Queue clear'}
					</a>
				</div>
				<a href={resolve('/work')} class="hb-pil-hero flex flex-col gap-2 rounded-lg p-1 -m-1">
					<div class="flex items-baseline justify-between gap-2">
						<div class="flex items-baseline gap-1.5">
							<span class="hb-pil-num">{kpis.openCount}</span>
							<span class="text-xs font-medium" style="color: var(--exec-muted);">open items</span>
						</div>
						<span class="text-xs font-semibold tabular-nums" style="color: var(--exec-dim);">
							{kpis.completionPct}% complete
						</span>
					</div>
					<div class="hb-bar-track" aria-hidden="true">
						<span class="hb-bar-budget" style="width: {kpis.completionPct}%"></span>
					</div>
				</a>
				<div class="hb-pil-sub grid grid-cols-2 gap-3 pt-2.5">
					<a href={resolve('/work')} class="hb-sub-cell flex flex-col gap-0.5 rounded-md p-1 -m-1">
						<span class="text-[11px] font-medium" style="color: var(--exec-dim);">Completed</span>
						<span class="text-sm font-semibold tabular-nums" style="color: var(--exec-ink);">
							{kpis.doneCount}
							<span class="ml-1 text-[11px] font-semibold" style="color: var(--exec-ok);">done</span>
						</span>
					</a>
					<a href={resolve('/inbox')} class="hb-sub-cell flex flex-col gap-0.5 rounded-md p-1 -m-1">
						<span class="text-[11px] font-medium" style="color: var(--exec-dim);">Blocked</span>
						<span class="text-sm font-semibold tabular-nums" style="color: var(--exec-ink);">
							{kpis.blockedCount}
							<span
								class="ml-1 text-[11px] font-semibold"
								style="color: {kpis.blockedCount > 0 ? 'var(--exec-bad)' : 'var(--exec-ok)'};"
							>
								{kpis.blockedCount > 0 ? 'blocked' : 'clear'}
							</span>
						</span>
					</a>
				</div>
			</article>

			<!-- Pillar 2: Team & Offices (dynamic online/offline counts, green online + rose offline) -->
			<article class="exec-card flex flex-col gap-3 p-4.5">
				<div class="flex items-center justify-between gap-2">
					<a href={resolve('/agents')} class="hb-pil-name inline-flex items-center gap-1.5">
						<Users class="size-3.5" style="color: var(--exec-accent-ink);" aria-hidden="true" />
						<span>Team</span>
					</a>
					<a
						href={resolve('/offices')}
						class={['hb-pil-pill', kpis.officePillTone === 'bad' ? 'is-bad' : 'is-ok']}
						title={kpis.offlineOffices > 0 ? `Offline: ${kpis.offlineOfficeNames.join(', ')}` : 'All offices online'}
					>
						{kpis.officePillLabel}
					</a>
				</div>
				<a href={resolve('/agents')} class="hb-pil-hero flex flex-col gap-2 rounded-lg p-1 -m-1">
					<div class="flex items-baseline justify-between gap-2">
						<div class="flex items-baseline gap-1.5">
							<span class="hb-pil-num">{kpis.totalAgents > 0 ? `${kpis.autonomyPct}%` : '—'}</span>
							<span class="text-xs font-medium" style="color: var(--exec-muted);">autonomous</span>
						</div>
						<span class="text-xs font-semibold tabular-nums" style="color: var(--exec-dim);">
							{kpis.peopleCount} {kpis.peopleCount === 1 ? 'person' : 'people'} · {kpis.totalAgents} agents
						</span>
					</div>
					<div class="hb-bar-track" aria-hidden="true">
						<span class="hb-bar-sky" style="width: {kpis.autonomyPct}%"></span>
						{#if kpis.totalAgents > 0 && kpis.autonomyPct < 100}
							<span class="hb-bar-warn" style="width: {100 - kpis.autonomyPct}%"></span>
						{/if}
					</div>
				</a>
				<div class="hb-pil-sub grid grid-cols-[1.22fr_1fr] gap-2.5 pt-2.5">
					<a href={resolve('/agents')} class="hb-sub-cell flex flex-col gap-0.5 rounded-md p-1 -m-1">
						<span class="truncate text-[11px] font-medium" style="color: var(--exec-dim);">Agents</span>
						<span class="truncate text-sm font-semibold tabular-nums" style="color: var(--exec-ink);">
							{kpis.activeAgents} active
							{#if kpis.pausedAgents > 0}
								<span class="ml-1 text-[11px]" style="color: var(--exec-warn);">· {kpis.pausedAgents} idle</span>
							{/if}
							{#if kpis.unplacedAgents > 0}
								<span class="ml-1 text-[11px]" style="color: var(--exec-bad);">· {kpis.unplacedAgents} off</span>
							{/if}
						</span>
					</a>
					<a
						href={resolve('/offices')}
						class="hb-sub-cell flex flex-col gap-0.5 rounded-md p-1 -m-1"
						title={kpis.offlineOffices > 0
							? `${kpis.onlineOffices} online · ${kpis.offlineOffices} offline (${kpis.offlineOfficeNames.join(', ')})`
							: `${kpis.onlineOffices} online`}
					>
						<span class="truncate text-[11px] font-medium" style="color: var(--exec-dim);">Offices</span>
						<span class="flex items-baseline gap-1.5 truncate text-[13px] font-semibold tabular-nums">
							<span style="color: var(--exec-ok);">{kpis.onlineOffices} online</span>
							{#if kpis.offlineOffices > 0}
								<span style="color: var(--exec-dim);" aria-hidden="true">·</span>
								<span style="color: var(--exec-bad);">{kpis.offlineOffices} offline</span>
							{/if}
						</span>
					</a>
				</div>
			</article>

			<!-- Pillar 3: Delivery -->
			<article class="exec-card flex flex-col gap-3 p-4.5">
				<div class="flex items-center justify-between gap-2">
					<a href={resolve('/projects')} class="hb-pil-name inline-flex items-center gap-1.5">
						<CheckCircle2 class="size-3.5" style="color: var(--exec-ok);" aria-hidden="true" />
						<span>Delivery</span>
					</a>
					<a
						href={resolve('/projects')}
						class={['hb-pil-pill', kpis.projectsAttention > 0 ? 'is-warn' : 'is-ok']}
					>
						{kpis.projectsAttention > 0
							? `${kpis.projectsAttention} need attention`
							: kpis.projectsTotal > 0
								? 'All on track'
								: 'No projects yet'}
					</a>
				</div>
				<a href={resolve('/projects')} class="hb-pil-hero flex flex-col gap-2 rounded-lg p-1 -m-1">
					<div class="flex items-baseline justify-between gap-2">
						<div class="flex items-baseline gap-1.5">
							<span class="hb-pil-num">{kpis.doneCount > 0 ? `${kpis.firstTryPassPct}%` : '—'}</span>
							<span class="text-xs font-medium" style="color: var(--exec-muted);">clean delivery</span>
						</div>
						<span class="text-xs font-semibold tabular-nums" style="color: var(--exec-ok);">
							{kpis.projectsTotal} {kpis.projectsTotal === 1 ? 'project' : 'projects'}
						</span>
					</div>
					<div class="hb-bar-track" aria-hidden="true">
						<span class="hb-bar-ok" style="width: {kpis.firstTryPassPct}%"></span>
					</div>
				</a>
				<div class="hb-pil-sub grid grid-cols-2 gap-3 pt-2.5">
					<a href={resolve('/work')} class="hb-sub-cell flex flex-col gap-0.5 rounded-md p-1 -m-1">
						<span class="text-[11px] font-medium" style="color: var(--exec-dim);">Open bugs</span>
						<span class="text-sm font-semibold tabular-nums" style="color: var(--exec-ink);">
							{kpis.openBugs}
							{#if kpis.criticalBugs > 0}
								<span class="ml-1 text-[11px]" style="color: var(--exec-bad);">{kpis.criticalBugs} serious</span>
							{/if}
						</span>
					</a>
					<a href={resolve('/work')} class="hb-sub-cell flex flex-col gap-0.5 rounded-md p-1 -m-1">
						<span class="text-[11px] font-medium" style="color: var(--exec-dim);">Delivered</span>
						<span class="text-sm font-semibold tabular-nums" style="color: var(--exec-ink);">
							{kpis.doneCount} {kpis.doneCount === 1 ? 'item' : 'items'}
						</span>
					</a>
				</div>
			</article>
		</section>

		<!-- Zone 2: Decision Spotlight + Catch-up -->
		<section class="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1.42fr)_minmax(340px,1fr)]">
			<!-- Left: Decision Spotlight -->
			<article
				class={['exec-card hb-spot relative flex min-h-[280px] flex-col gap-4 overflow-hidden p-5 sm:p-6', currentDecision?.urgent && 'is-urgent']}
				aria-label="Decision Spotlight"
			>
				{#if currentDecision}
					<div class="flex flex-wrap items-center justify-between gap-2">
						<span class={['hb-spot-badge inline-flex items-center gap-1.5', currentDecision.urgent && 'is-urgent']}>
							<Clock class="size-3.5" aria-hidden="true" />
							<span>{currentDecision.badgeText}</span>
						</span>
						<div class="flex items-center gap-2.5">
							<div class="flex items-center gap-1" role="tablist" aria-label="Priority queue">
								{#each spotlight.slice(0, 6) as d, i (d.id)}
									<button
										type="button"
										role="tab"
										aria-selected={i === safeIdx}
										aria-label="Decision {i + 1}: {d.title}"
										onclick={() => {
											spotIdx = i;
											decliningId = undefined;
										}}
										class={['hb-dot', i === safeIdx && 'is-cur', d.urgent && 'is-urgent']}
									></button>
								{/each}
							</div>
							<span class="text-xs font-semibold tabular-nums" style="color: var(--exec-muted);">
								Priority {safeIdx + 1} of {spotlight.length}
							</span>
							<div class="flex items-center gap-1">
								<button
									type="button"
									disabled={spotlight.length <= 1}
									onclick={() => (spotIdx = (safeIdx - 1 + spotlight.length) % spotlight.length)}
									aria-label="Previous decision"
									class="hb-arrow flex size-7 items-center justify-center rounded-full"
								>
									<ChevronLeft class="size-4" aria-hidden="true" />
								</button>
								<button
									type="button"
									disabled={spotlight.length <= 1}
									onclick={() => (spotIdx = (safeIdx + 1) % spotlight.length)}
									aria-label="Next decision"
									class="hb-arrow flex size-7 items-center justify-center rounded-full"
								>
									<ChevronRight class="size-4" aria-hidden="true" />
								</button>
							</div>
						</div>
					</div>

					<div class="flex flex-1 flex-col gap-3.5">
						<div class="flex flex-wrap items-center gap-2 text-xs" style="color: var(--exec-muted);">
							<span class="hb-av inline-flex size-6 items-center justify-center rounded-full text-[10px] font-bold" style="color: var(--exec-ink);">
								{currentDecision.actorInitials}
							</span>
							<span class="font-semibold" style="color: var(--exec-ink);">{currentDecision.actorName}</span>
							<span aria-hidden="true">·</span>
							<span>{currentDecision.kindLabel}</span>
							<span aria-hidden="true">·</span>
							<span>{currentDecision.ageLabel}</span>
							<span aria-hidden="true">·</span>
							<span>{currentDecision.projectName}</span>
						</div>

						<h2 class="text-lg leading-snug font-semibold tracking-tight sm:text-xl" style="color: var(--exec-ink);">
							{currentDecision.title}
						</h2>

						<div class="hb-rec flex items-start gap-2.5 rounded-xl px-3.5 py-2.5 text-sm">
							<Info class="mt-0.5 size-4 shrink-0" style="color: var(--exec-accent-ink);" aria-hidden="true" />
							<span>{currentDecision.recommendation}</span>
						</div>

						<!-- 3-Column Risk, Impact & Proof Strip -->
						<div class="exec-well grid grid-cols-1 gap-2.5 p-3 sm:grid-cols-3 sm:gap-0 sm:px-1">
							{#each [currentDecision.risk, currentDecision.impact, currentDecision.proof] as cell, ci (cell.label + ci)}
								<div class={['flex min-w-0 items-center gap-2.5 sm:px-3', ci > 0 && 'hb-cell-sep']}>
									<span class={['hb-well-ico flex size-8 shrink-0 items-center justify-center rounded-full', `tone-${cell.tone}`]}>
										{#if cell.icon === 'shield-check'}
											<ShieldCheck class="size-4" aria-hidden="true" />
										{:else if cell.icon === 'shield-alert'}
											<ShieldAlert class="size-4" aria-hidden="true" />
										{:else if cell.icon === 'bolt'}
											<Zap class="size-4" aria-hidden="true" />
										{:else if cell.icon === 'scale'}
											<Scale class="size-4" aria-hidden="true" />
										{:else if cell.icon === 'check-circle'}
											<CheckCircle2 class="size-4" aria-hidden="true" />
										{:else}
											<Clock class="size-4" aria-hidden="true" />
										{/if}
									</span>
									<div class="min-w-0">
										<div class="truncate text-xs font-semibold" style="color: var(--exec-ink);">{cell.label}</div>
										<div class="truncate text-[11px]" style="color: var(--exec-muted);">{cell.sub}</div>
									</div>
								</div>
							{/each}
						</div>
					</div>

					<!-- Action Bar directly above UP NEXT -->
					<div class="hb-spot-foot flex flex-wrap items-center justify-between gap-3 pt-3.5">
						<div class="flex flex-wrap items-center gap-2">
							<button
								type="button"
								disabled={busyId === currentDecision.id}
								onclick={() => void runAction('primary')}
								class="exec-cta"
							>
								<span class="exec-cta-fill" aria-hidden="true"></span>
								<span class="exec-cta-sheen" aria-hidden="true"></span>
								<span class="exec-cta-inner">
									<Check class="size-4" aria-hidden="true" />
									<span>{currentDecision.primaryAction.label}</span>
								</span>
							</button>
							{#if currentDecision.secondaryAction}
								<button
									type="button"
									disabled={busyId === currentDecision.id}
									onclick={() => void runAction('secondary')}
									class="hb-outline-btn inline-flex h-9 items-center gap-1.5 rounded-lg px-3 text-xs font-semibold"
								>
									<Ban class="size-3.5" aria-hidden="true" />
									<span>{currentDecision.secondaryAction.label}</span>
								</button>
							{/if}
							<a href={resolve(currentDecision.detailHref as '/work')} class="inline-flex items-center gap-0.5 px-2 text-xs font-medium" style="color: var(--exec-muted);">
								<span>Details</span>
								<ChevronRight class="size-3.5" aria-hidden="true" />
							</a>
						</div>
						<a href={resolve('/inbox')} class="inline-flex items-center gap-0.5 text-xs font-medium" style="color: var(--exec-muted);">
							<span>All {spotlight.length} in Inbox</span>
							<ChevronRight class="size-3.5" aria-hidden="true" />
						</a>

						{#if decliningId === currentDecision.id}
							<div class="mt-1 flex w-full flex-wrap items-center gap-2">
								<input
									bind:value={declineReason}
									type="text"
									placeholder="Optional reason for declining…"
									class="hb-input min-w-[200px] flex-1 rounded-lg px-3 py-1.5 text-xs"
								/>
								<button type="button" onclick={() => void runAction('secondary')} class="hb-danger-btn rounded-lg px-3 py-1.5 text-xs font-semibold">
									Confirm decline
								</button>
								<button type="button" onclick={() => (decliningId = undefined)} class="px-2 py-1.5 text-xs" style="color: var(--exec-muted);">
									Cancel
								</button>
							</div>
						{/if}
					</div>

					<!-- UP NEXT tucked below action bar -->
					{#if nextDecision}
						<button
							type="button"
							onclick={() => {
								spotIdx = (safeIdx + 1) % spotlight.length;
								decliningId = undefined;
							}}
							class="exec-well hb-next-bar flex items-center justify-between gap-2 px-3 py-2 text-left text-xs"
						>
							<span class="flex min-w-0 items-center gap-2">
								<span class="shrink-0 text-[10px] font-bold tracking-wider uppercase" style="color: var(--exec-dim);">
									Up next
								</span>
								<span class="truncate font-medium" style="color: var(--exec-body);">{nextDecision.title}</span>
							</span>
							<ChevronRight class="size-3.5 shrink-0" style="color: var(--exec-dim);" aria-hidden="true" />
						</button>
					{/if}
				{:else}
					<!-- All clear / Day-1 Launch -->
					<div class="flex flex-1 flex-col items-center justify-center gap-3 py-8 text-center">
						<span class="hb-ring is-ok flex size-12 items-center justify-center rounded-full">
							<CheckCircle2 class="size-6" aria-hidden="true" />
						</span>
						<h2 class="text-base font-semibold" style="color: var(--exec-ink);">
							{isDayOne ? 'Launch your organization' : 'Nothing waiting on you'}
						</h2>
						<p class="max-w-md text-sm" style="color: var(--exec-muted);">
							{isDayOne
								? 'Your HQ office is online. Add your first agents, create a project, or open Work to start autonomous delivery.'
								: 'Every decision is cleared. Your agents and teams are continuing work autonomously.'}
						</p>
						<div class="mt-2 flex flex-wrap justify-center gap-2.5">
							<a href={resolve('/work')} class="exec-cta">
								<span class="exec-cta-fill" aria-hidden="true"></span>
								<span class="exec-cta-inner">
									<span>Open Work</span>
									<ChevronRight class="size-4" aria-hidden="true" />
								</span>
							</a>
							<a href={resolve('/agents')} class="hb-outline-btn inline-flex h-9 items-center gap-1.5 rounded-lg px-3.5 text-xs font-semibold">
								<span>Manage Agents</span>
							</a>
						</div>
					</div>
				{/if}
			</article>

			<!-- Right: Catch-up Digest -->
			<aside class="exec-card flex flex-col gap-3.5 p-5" aria-label="Catch-up">
				<div class="flex items-center justify-between gap-2">
					<h2 class="inline-flex items-center gap-2 text-sm font-semibold" style="color: var(--exec-ink);">
						<Activity class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
						<span>Catch-up</span>
					</h2>
					<span class="hb-cu-pill inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11px] font-medium">
						<span class="size-1.5 rounded-full" style="background: var(--exec-ok);"></span>
						<span>Live org</span>
					</span>
				</div>

				<div class="flex flex-1 flex-col justify-between gap-2.5">
					{#each catchUp as row (row.id)}
						<a href={resolve(row.href as '/work')} class="exec-well hb-cu-row flex items-start gap-2.5 p-3 text-xs">
							<span class="mt-0.5 flex size-4 shrink-0 items-center justify-center">
								{#if row.tone === 'ok'}
									<CheckCircle2 class="size-3.5" style="color: var(--exec-ok);" aria-hidden="true" />
								{:else if row.tone === 'warn'}
									<AlertTriangle class="size-3.5" style="color: var(--exec-warn);" aria-hidden="true" />
								{:else if row.tone === 'live'}
									<span class="size-2 rounded-full" style="background: var(--exec-ok);"></span>
								{:else}
									<Clock class="size-3.5" style="color: var(--exec-accent-ink);" aria-hidden="true" />
								{/if}
							</span>
							<div class="min-w-0 flex-1 leading-relaxed" style="color: var(--exec-body);">
								<strong style="color: var(--exec-ink);">{row.title}</strong> — {row.detail}
							</div>
							<span class="shrink-0 text-[11px] tabular-nums" style="color: var(--exec-dim);">{row.timeLabel}</span>
						</a>
					{/each}
				</div>

				<div class="hb-spot-foot flex justify-end pt-3">
					<a href={resolve('/activity')} class="inline-flex items-center gap-1 text-xs font-medium" style="color: var(--exec-muted);">
						<span>Full activity log</span>
						<ChevronRight class="size-3.5" aria-hidden="true" />
					</a>
				</div>
			</aside>
		</section>

		<!-- Zone 3: Projects Bento Row -->
		{#if projectCards.length > 0}
			<section class="flex flex-col gap-3.5" aria-labelledby="hb-projects-heading">
				<div class="flex items-baseline justify-between gap-3">
					<div class="flex items-baseline gap-2.5">
						<h2 id="hb-projects-heading" class="text-[15px] font-semibold" style="color: var(--exec-ink);">Projects</h2>
						<span class="text-xs" style="color: var(--exec-muted);">
							{projectCards.length} active{kpis.projectsAttention > 0 ? ` · ${kpis.projectsAttention} need attention` : ''}
						</span>
					</div>
					<a href={resolve('/projects')} class="inline-flex items-center gap-0.5 text-xs font-semibold" style="color: var(--exec-accent-ink);">
						<span>All projects</span>
						<ChevronRight class="size-3.5" aria-hidden="true" />
					</a>
				</div>
				<div class="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
					{#each projectCards.slice(0, 6) as proj (proj.id)}
						<article class="exec-card flex flex-col gap-3.5 p-5">
							<div class="flex items-center justify-between gap-2">
								<div class="flex min-w-0 items-baseline gap-2">
									<a href={resolve(`/projects/${proj.id}` as '/projects')} class="truncate text-[15px] font-semibold hover:underline" style="color: var(--exec-ink);">
										{proj.name}
									</a>
									{#if proj.key}
										<span class="hb-key-tag font-mono text-[11px]">{proj.key}</span>
									{/if}
								</div>
								<span class={['hb-pil-pill inline-flex items-center gap-1', `is-${proj.healthTone}`]}>
									{#if proj.healthTone === 'bad'}
										<XCircle class="size-3" aria-hidden="true" />
									{:else if proj.healthTone === 'warn'}
										<AlertTriangle class="size-3" aria-hidden="true" />
									{:else}
										<CheckCircle2 class="size-3" aria-hidden="true" />
									{/if}
									<span>{proj.health}</span>
								</span>
							</div>

							<div class="flex flex-col gap-1.5">
								<div class="flex items-baseline justify-between text-xs tabular-nums" style="color: var(--exec-muted);">
									<span>{proj.doneCount} of {proj.totalCount} done</span>
									<strong class="text-sm" style="color: var(--exec-ink);">{proj.pct}%</strong>
								</div>
								<div class="hb-bar-track" aria-hidden="true">
									<span class={`hb-bar-${proj.healthTone}`} style="width: {proj.pct}%"></span>
								</div>
							</div>

							<div class={['hb-blocker flex min-h-[56px] items-center justify-between gap-2.5 rounded-xl px-3 py-2 text-xs', `tone-${proj.blockerTone}`]}>
								<span class="min-w-0 flex-1 leading-snug">{proj.blockerText}</span>
								<a href={resolve(proj.actionHref as '/work')} class="hb-unblock inline-flex shrink-0 items-center gap-0.5 rounded-md px-2 py-1 text-[11px] font-semibold">
									<span>{proj.actionLabel}</span>
									<ChevronRight class="size-3" aria-hidden="true" />
								</a>
							</div>

							<div class="hb-spot-foot mt-auto flex items-center justify-between gap-2 pt-3 text-xs" style="color: var(--exec-muted);">
								<div class="flex items-center">
									{#each proj.memberInitials as init, idx (init + idx)}
										<span class="hb-av hb-av-stack inline-flex size-6 items-center justify-center rounded-full text-[9px] font-bold" style="color: var(--exec-ink);">
											{init}
										</span>
									{/each}
									{#if proj.extraMembers > 0}
										<span class="ml-1.5 text-[11px]">+{proj.extraMembers}</span>
									{/if}
								</div>
								<a href={resolve(`/projects/${proj.id}` as '/projects')} class="inline-flex items-center gap-0.5 hover:underline">
									<span>Lead {proj.leadName}</span>
									<ChevronRight class="size-3" aria-hidden="true" />
								</a>
							</div>
						</article>
					{/each}
				</div>
			</section>
		{/if}

		<!-- Zone 4: Where Task Time Goes (>30% waiting for an agent = coral-rose, <=30% = purple) -->
		{#if taskTime.totalOpen > 0}
			<section class="exec-card flex flex-col gap-3 p-5" aria-labelledby="hb-time-heading">
				<div class="flex flex-wrap items-baseline justify-between gap-2">
					<h2 id="hb-time-heading" class="text-[15px] font-semibold" style="color: var(--exec-ink);">Where task time goes</h2>
					<span class="text-xs tabular-nums" style="color: var(--exec-dim);">
						{taskTime.totalOpen} open {taskTime.totalOpen === 1 ? 'item' : 'items'}
					</span>
				</div>

				<div class="flex h-2.5 gap-1 overflow-hidden rounded-full" role="group" aria-label="Open task distribution">
					{#if taskTime.workingPct > 0}
						<a href={resolve('/agents')} class="hb-seg-agents rounded-full" style="width: {taskTime.workingPct}%" aria-label="Agents working: {taskTime.workingPct}%"></a>
					{/if}
					{#if taskTime.reviewPct > 0}
						<a href={resolve('/work')} class="hb-seg-ci rounded-full" style="width: {taskTime.reviewPct}%" aria-label="In review: {taskTime.reviewPct}%"></a>
					{/if}
					{#if taskTime.waitingYouPct > 0}
						<a href={resolve('/inbox')} class="hb-seg-human rounded-full" style="width: {taskTime.waitingYouPct}%" aria-label="Waiting on you: {taskTime.waitingYouPct}%"></a>
					{/if}
					{#if taskTime.queuedPct > 0}
						<a
							href={resolve('/work')}
							class={['rounded-full', taskTime.queuedIsHigh ? 'hb-seg-queued-high' : 'hb-seg-queued']}
							style="width: {taskTime.queuedPct}%"
							aria-label="Waiting for an agent: {taskTime.queuedPct}%"
						></a>
					{/if}
				</div>

				<div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs">
					<a href={resolve('/agents')} class="inline-flex items-center gap-1.5">
						<span class="size-2 rounded-full bg-info"></span>
						<strong style="color: var(--exec-sky);">{taskTime.workingPct}%</strong>
						<span style="color: var(--exec-muted);">Agents working ({taskTime.workingCount})</span>
					</a>
					<a href={resolve('/work')} class="inline-flex items-center gap-1.5">
						<span class="size-2 rounded-full bg-positive"></span>
						<strong style="color: var(--exec-ok);">{taskTime.reviewPct}%</strong>
						<span style="color: var(--exec-muted);">In review ({taskTime.reviewCount})</span>
					</a>
					<a href={resolve('/inbox')} class="inline-flex items-center gap-1.5">
						<span class="size-2 rounded-full bg-warning"></span>
						<strong style="color: var(--exec-warn);">{taskTime.waitingYouPct}%</strong>
						<span style="color: var(--exec-muted);">Waiting on you ({taskTime.waitingYouCount})</span>
					</a>
					<a href={resolve('/work')} class="inline-flex items-center gap-1.5">
						<span
							class="size-2 rounded-full"
							style="background: {taskTime.queuedIsHigh ? 'var(--exec-bad)' : 'var(--exec-purple)'};"
						></span>
						<strong style="color: {taskTime.queuedIsHigh ? 'var(--exec-bad)' : 'var(--exec-purple)'};">
							{taskTime.queuedPct}%
						</strong>
						<span style="color: var(--exec-muted);">Waiting for an agent ({taskTime.queuedCount})</span>
					</a>
				</div>

				<div class="flex flex-wrap items-center justify-between gap-2 text-xs">
					<p style="color: var(--exec-muted);">{taskTime.summary}</p>
					<div class="flex items-center gap-3.5">
						<a href={resolve(taskTime.primaryLink.href as '/work')} class="inline-flex items-center gap-0.5 font-semibold" style="color: var(--exec-accent-ink);">
							<span>{taskTime.primaryLink.label}</span>
							<ChevronRight class="size-3.5" aria-hidden="true" />
						</a>
						{#if taskTime.secondaryLink}
							<a href={resolve(taskTime.secondaryLink.href as '/work')} class="inline-flex items-center gap-0.5 font-semibold" style="color: var(--exec-accent-ink);">
								<span>{taskTime.secondaryLink.label}</span>
								<ChevronRight class="size-3.5" aria-hidden="true" />
							</a>
						{/if}
					</div>
				</div>
			</section>
		{/if}
	{/if}
</div>
{:else}
	<HomeView
		organizationId={data.connection.organization.id}
		organizationName={data.connection.organization.name}
	/>
{/if}

<style>
	.hb-h1 {
		font-size: clamp(1.375rem, 2.2vw, 1.875rem);
		line-height: 1.22;
		font-weight: 600;
		letter-spacing: -0.03em;
		color: var(--exec-ink);
	}
	.hb-h1 em {
		font-style: normal;
	}
	.hb-h1 em.tone-warn {
		color: var(--exec-warn);
	}
	.hb-h1 em.tone-calm {
		color: var(--exec-accent-ink);
	}
	.hb-h1 em.tone-ok {
		color: var(--exec-ok);
	}
	.hb-h1 em.tone-bad {
		color: var(--exec-bad);
	}
	.hb-pil-name {
		font-size: 0.7rem;
		font-weight: 650;
		letter-spacing: 0.11em;
		text-transform: uppercase;
		color: var(--exec-muted);
	}
	.hb-pil-pill {
		padding: 2px 8px;
		border-radius: 999px;
		font-size: 0.7rem;
		font-weight: 600;
		white-space: nowrap;
	}
	.hb-pil-pill.is-ok {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	.hb-pil-pill.is-warn {
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
	}
	.hb-pil-pill.is-bad {
		background: var(--exec-bad-bg);
		color: var(--exec-bad);
		box-shadow: inset 0 0 0 1px var(--exec-bad-ring);
	}
	.hb-pil-hero:hover,
	.hb-sub-cell:hover {
		background: var(--exec-accent-soft);
	}
	.hb-pil-num {
		font-size: 1.625rem;
		line-height: 1.1;
		font-weight: 650;
		letter-spacing: -0.03em;
		color: var(--exec-ink);
		font-variant-numeric: tabular-nums;
	}
	.hb-bar-track {
		display: flex;
		gap: 2px;
		height: 5px;
		border-radius: 999px;
		overflow: hidden;
		background: var(--exec-bar-track);
	}
	.hb-bar-track > span {
		display: block;
		height: 100%;
		border-radius: 999px;
	}
	.hb-bar-budget {
		background: linear-gradient(90deg, #38bdf8 0%, #fbbf24 100%);
	}
	.hb-bar-sky {
		background: linear-gradient(90deg, #0ea5e9 0%, #38bdf8 100%);
	}
	.hb-bar-ok {
		background: linear-gradient(90deg, #10b981 0%, #34d399 100%);
	}
	.hb-bar-warn {
		background: #fbbf24;
	}
	.hb-bar-bad {
		background: #fb7185;
	}
	.hb-pil-sub {
		border-top: 1px solid var(--exec-border);
	}
	.hb-spot::before {
		content: '';
		position: absolute;
		left: 0;
		right: 0;
		top: 0;
		height: 2px;
		background: linear-gradient(90deg, var(--exec-accent) 0%, transparent 60%);
	}
	.hb-spot.is-urgent::before {
		background: linear-gradient(90deg, #fbbf24 0%, transparent 60%);
	}
	.hb-spot-badge {
		padding: 4px 11px;
		border-radius: 999px;
		font-size: 0.75rem;
		font-weight: 600;
		background: var(--exec-sky-bg);
		color: var(--exec-sky);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
	}
	.hb-spot-badge.is-urgent {
		background: var(--exec-warn-bg);
		color: var(--exec-warn);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
	}
	.hb-dot {
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: var(--exec-border-hi);
		transition: width 0.2s ease, background-color 0.16s ease;
	}
	.hb-dot.is-urgent {
		background: color-mix(in srgb, var(--warning) 65%, transparent);
	}
	.hb-dot.is-cur {
		width: 22px;
		background: var(--exec-accent);
	}
	.hb-dot.is-cur.is-urgent {
		background: var(--warning);
	}
	.hb-arrow {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-body);
	}
	.hb-arrow:disabled {
		opacity: 0.35;
	}
	.hb-rec {
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring), inset 3px 0 0 var(--exec-accent);
		color: var(--exec-body);
	}
	.hb-cell-sep {
		border-top: 1px solid var(--exec-border);
		padding-top: 10px;
	}
	@media (min-width: 640px) {
		.hb-cell-sep {
			border-top: 0;
			padding-top: 0;
			border-left: 1px solid var(--exec-border);
		}
	}
	.hb-well-ico.tone-ok {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}
	.hb-well-ico.tone-warn {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
	}
	.hb-well-ico.tone-sky {
		background: var(--exec-sky-bg);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
		color: var(--exec-sky);
	}
	.hb-well-ico.tone-muted {
		background: var(--exec-surface);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-muted);
	}
	.hb-spot-foot {
		border-top: 1px solid var(--exec-border);
	}
	.hb-outline-btn {
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.hb-outline-btn:hover {
		background: var(--exec-surface-hi);
	}
	.hb-danger-btn {
		background: var(--exec-bad-bg);
		color: var(--exec-bad);
		box-shadow: inset 0 0 0 1px var(--exec-bad-ring);
	}
	.hb-input {
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.hb-next-bar:hover,
	.hb-cu-row:hover {
		background: var(--exec-surface-hi);
	}
	.hb-cu-pill {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--exec-muted);
	}
	.hb-key-tag {
		padding: 1px 6px;
		border-radius: 5px;
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.hb-blocker {
		color: var(--exec-body);
	}
	.hb-blocker.tone-bad {
		background: var(--exec-bad-bg);
		box-shadow: inset 0 0 0 1px var(--exec-bad-ring);
	}
	.hb-blocker.tone-warn {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
	}
	.hb-blocker.tone-sky {
		background: var(--exec-sky-bg);
		box-shadow: inset 0 0 0 1px var(--exec-sky-ring);
	}
	.hb-blocker.tone-ok {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	.hb-unblock {
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.hb-av {
		background: var(--exec-avatar-bg);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.hb-av-stack {
		margin-left: -6px;
		box-shadow: 0 0 0 2px var(--exec-avatar-ring);
	}
	.hb-av-stack:first-child {
		margin-left: 0;
	}
	.hb-ring.is-ok {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}
	.hb-ring.is-bad {
		background: var(--exec-bad-bg);
		box-shadow: inset 0 0 0 1px var(--exec-bad-ring);
		color: var(--exec-bad);
	}
	.hb-skel {
		background: var(--exec-bar-track);
	}
	.hb-seg-agents {
		background: linear-gradient(90deg, #38bdf8 0%, #7dd3fc 100%);
	}
	.hb-seg-ci {
		background: linear-gradient(90deg, #10b981 0%, #34d399 100%);
	}
	.hb-seg-human {
		background: linear-gradient(90deg, #eab308 0%, #f5c76b 100%);
	}
	.hb-seg-queued {
		background: linear-gradient(90deg, #7c3aed 0%, #a78bfa 100%);
	}
	.hb-seg-queued-high {
		background: linear-gradient(90deg, #fb7185 0%, #fda4af 100%);
	}
</style>
