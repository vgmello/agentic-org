<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Activity from '@lucide/svelte/icons/activity';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Ban from '@lucide/svelte/icons/ban';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronLeft from '@lucide/svelte/icons/chevron-left';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clipboard from '@lucide/svelte/icons/clipboard';
	import Clock from '@lucide/svelte/icons/clock';
	import DollarSign from '@lucide/svelte/icons/dollar-sign';
	import Info from '@lucide/svelte/icons/info';
	import Plus from '@lucide/svelte/icons/plus';
	import RefreshCw from '@lucide/svelte/icons/refresh-cw';
	import Scale from '@lucide/svelte/icons/scale';
	import ShieldAlert from '@lucide/svelte/icons/shield-alert';
	import ShieldCheck from '@lucide/svelte/icons/shield-check';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Users from '@lucide/svelte/icons/users';
	import XCircle from '@lucide/svelte/icons/x-circle';
	import Zap from '@lucide/svelte/icons/zap';
	import type { OfficeInfo, WorkItemSummary } from '$lib/proto';
	import { activityApi, type ActivityRow } from '$lib/modules/activity';
	import { costsApi, type AgentBudgets, type OrganizationCost } from '$lib/modules/costs';
	import { homeApi } from '$lib/modules/home/api';
	import type { HomeProject } from '$lib/modules/home/types';
	import { inboxApi, type InboxItem } from '$lib/modules/inbox';
	import { membersApi, type MemberRow } from '$lib/modules/members';
	import { officesApi, layerOffices } from '$lib/modules/offices';
	import { accountZone } from '$lib/modules/preferences';
	import { projectsApi, type ProjectRow } from '$lib/modules/projects';
	import { executeDecisionAction } from '$lib/home/actions';
	import {
		deriveCatchUpDigest,
		deriveDecisionSpotlight,
		deriveExecutiveBrief,
		deriveKpiMetrics,
		deriveProjectCards,
		deriveTaskTimeBreakdown,
		greetingForHour,
		isDenyAction,
		type BriefLens,
		type HomeDomainData
	} from '$lib/home/model';
	import { pageTitle } from '$lib/nav';
	import { officeApi, toAppError } from '$lib/office/api';
	import { owner } from '$lib/org/model';
	import { workApi } from '$lib/work';

	let { data } = $props();
	const organizationId = $derived(data.connection.organization.id);
	const snapshot = $derived(data.snapshot);
	const ownerMember = $derived(owner(snapshot));
	const ownerId = $derived(ownerMember?.id ?? 'owner');

	let items = $state.raw<WorkItemSummary[] | undefined>(undefined);
	let offices = $state.raw<OfficeInfo[]>([]);
	let orgCost = $state.raw<OrganizationCost | undefined>(undefined);
	let agentBudgets = $state.raw<AgentBudgets | undefined>(undefined);
	let inboxItems = $state.raw<InboxItem[]>([]);
	let memberRows = $state.raw<MemberRow[] | undefined>(undefined);
	let projectRows = $state.raw<ProjectRow[] | undefined>(undefined);
	let homeProjects = $state.raw<HomeProject[] | undefined>(undefined);
	let activityRows = $state.raw<ActivityRow[] | undefined>(undefined);

	let loadError = $state<string | undefined>(undefined);
	let busyId = $state<string | undefined>(undefined);
	let spotIdx = $state(0);
	let decliningId = $state<string | undefined>(undefined);
	let declineReason = $state('');
	let denyInputEl = $state<HTMLInputElement | null>(null);
	let denyTriggerEl = $state<HTMLButtonElement | null>(null);
	let openContextIds = $state<Record<string, boolean>>({});
	let briefLens = $state<BriefLens>('today');
	let briefExpanded = $state(false);
	let briefCopied = $state(false);
	let briefCopyTimer: ReturnType<typeof setTimeout> | undefined;
	let nowMs = $state(Date.now());

	const BRIEF_LENSES: readonly { id: BriefLens; label: string }[] = [
		{ id: 'today', label: 'Today' },
		{ id: 'week', label: '7-Day' },
		{ id: 'board', label: 'Board' }
	];

	const activeOffices = $derived(
		offices.length > 0 ? offices : data.connection.organization.offices
	);
	const loading = $derived(items === undefined && loadError === undefined);
	const workItems = $derived(items ?? []);
	const domain = $derived<HomeDomainData>({
		orgCost,
		agentBudgets,
		inboxItems,
		memberRows,
		projectRows,
		homeProjects,
		activityRows
	});

	const kpis = $derived(deriveKpiMetrics(snapshot, workItems, activeOffices, ownerId, domain));
	const spotlight = $derived(
		deriveDecisionSpotlight(workItems, snapshot, ownerId, nowMs, inboxItems)
	);
	const safeIdx = $derived(spotlight.length > 0 ? Math.min(spotIdx, spotlight.length - 1) : 0);
	const currentDecision = $derived(spotlight[safeIdx]);
	const nextDecision = $derived(
		spotlight.length > 1 ? spotlight[(safeIdx + 1) % spotlight.length] : undefined
	);
	const currentCtxOpen = $derived(
		currentDecision ? Boolean(openContextIds[currentDecision.id]) : false
	);
	const projectCards = $derived(deriveProjectCards(snapshot, workItems, domain));
	const catchUp = $derived(deriveCatchUpDigest(workItems, snapshot, nowMs, activityRows));
	const brief = $derived(
		deriveExecutiveBrief(
			snapshot,
			workItems,
			activeOffices,
			ownerId,
			data.connection.organization.name,
			nowMs,
			domain
		)
	);
	const activeBrief = $derived(brief.lenses[briefLens]);
	const taskTime = $derived(
		deriveTaskTimeBreakdown(workItems, ownerId, kpis.offlineOffices, domain)
	);
	const greeting = $derived(greetingForHour(new Date(nowMs).getHours()));
	const isDayOne = $derived(
		!loading &&
			!loadError &&
			workItems.length === 0 &&
			inboxItems.length === 0 &&
			projectCards.length === 0 &&
			kpis.totalAgents === 0
	);

	let loadSeq = 0;
	let destroyed = false;

	async function loadLive(
		targetOrgId = organizationId,
		baseConnectionOffices = data.connection.organization.offices,
		isCancelled: () => boolean = () => false
	) {
		const seq = ++loadSeq;
		loadError = undefined;
		try {
			await accountZone.ensure(targetOrgId).catch(() => undefined);
			if (destroyed || isCancelled() || seq !== loadSeq) return;
			const timeZone = accountZone.zone || 'UTC';
			const [
				workRes,
				orgsRes,
				officesRes,
				costRes,
				budgetsRes,
				inboxRes,
				membersRes,
				projectsRes,
				homeProjRes,
				activityRes
			] = await Promise.allSettled([
				workApi.snapshot(targetOrgId),
				officeApi.peekOrganizations(),
				officesApi.overview(targetOrgId),
				costsApi.organizationCost(targetOrgId, { kind: 'this_month', timeZone }),
				costsApi.agentBudgets(targetOrgId),
				inboxApi.list(targetOrgId),
				membersApi.directory(targetOrgId),
				projectsApi.list(targetOrgId),
				homeApi.projects(targetOrgId),
				activityApi.feed(targetOrgId, { kind: 'all' }, 12)
			]);
			if (destroyed || isCancelled() || seq !== loadSeq) return;

			if (workRes.status === 'fulfilled') {
				items = workRes.value.items;
			} else {
				const err = toAppError(workRes.reason);
				if (err.source === 'api' && (err.code === 'unsupported' || err.code === 'unavailable')) {
					items = [];
				} else {
					loadError = err.message;
				}
			}

			const baseOffices =
				orgsRes.status === 'fulfilled'
					? (orgsRes.value.find((o) => o.id === targetOrgId)?.offices ?? baseConnectionOffices)
					: baseConnectionOffices;
			const overview = officesRes.status === 'fulfilled' ? officesRes.value : undefined;
			offices = layerOffices(baseOffices, overview);

			orgCost = costRes.status === 'fulfilled' ? costRes.value : undefined;
			agentBudgets = budgetsRes.status === 'fulfilled' ? budgetsRes.value : undefined;
			inboxItems = inboxRes.status === 'fulfilled' ? inboxRes.value : [];
			memberRows = membersRes.status === 'fulfilled' ? membersRes.value : undefined;
			projectRows = projectsRes.status === 'fulfilled' ? projectsRes.value.projects : undefined;
			homeProjects = homeProjRes.status === 'fulfilled' ? homeProjRes.value : undefined;
			activityRows = activityRes.status === 'fulfilled' ? activityRes.value.rows : undefined;

			nowMs = Date.now();
		} catch (e) {
			if (destroyed || isCancelled() || seq !== loadSeq) return;
			loadError = toAppError(e).message;
		}
	}

	$effect(() => {
		const id = organizationId;
		const baseOffices = data.connection.organization.offices;
		let cancelled = false;
		items = undefined;
		offices = [];
		orgCost = undefined;
		agentBudgets = undefined;
		inboxItems = [];
		memberRows = undefined;
		projectRows = undefined;
		homeProjects = undefined;
		activityRows = undefined;
		loadError = undefined;
		spotIdx = 0;
		decliningId = undefined;
		declineReason = '';
		void loadLive(id, baseOffices, () => cancelled);
		return () => {
			cancelled = true;
		};
	});

	onDestroy(() => {
		destroyed = true;
		clearTimeout(briefCopyTimer);
	});

	async function armDeny(id: string) {
		decliningId = id;
		declineReason = '';
		await tick();
		denyInputEl?.focus();
	}

	async function cancelDeny() {
		decliningId = undefined;
		declineReason = '';
		await tick();
		denyTriggerEl?.focus();
	}

	function handleDenyKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			void runAction('secondary');
		} else if (event.key === 'Escape') {
			event.preventDefault();
			void cancelDeny();
		}
	}

	function handleBriefTabKeydown(event: KeyboardEvent, index: number) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step && event.key !== 'Home' && event.key !== 'End') return;
		event.preventDefault();
		const nextIdx =
			event.key === 'Home'
				? 0
				: event.key === 'End'
					? BRIEF_LENSES.length - 1
					: (index + step + BRIEF_LENSES.length) % BRIEF_LENSES.length;
		const nextLens = BRIEF_LENSES[nextIdx].id;
		briefLens = nextLens;
		briefCopied = false;
		void tick().then(() => document.getElementById(`hb-brief-tab-${nextLens}`)?.focus());
	}

	function toggleContext(id: string) {
		openContextIds = { ...openContextIds, [id]: !openContextIds[id] };
	}

	function copyBrief() {
		if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
			void navigator.clipboard.writeText(activeBrief.copyText).catch(() => undefined);
		}
		briefCopied = true;
		clearTimeout(briefCopyTimer);
		briefCopyTimer = setTimeout(() => {
			briefCopied = false;
		}, 2400);
		toast.success(`${activeBrief.label} executive brief copied to clipboard.`);
	}

	async function runAction(kind: 'primary' | 'secondary') {
		if (!currentDecision || busyId) return;
		if (
			kind === 'secondary' &&
			isDenyAction(currentDecision.secondaryAction) &&
			decliningId !== currentDecision.id
		) {
			await armDeny(currentDecision.id);
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

<div class="hb-page mx-auto flex w-full max-w-[1260px] flex-col gap-6 px-6 pt-3 pb-12 sm:px-8">
	<!-- Zone 1: Executive Greeting Header -->
	<section class="hb-hero flex flex-wrap items-center justify-between gap-4">
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
			<h2 class="text-base font-semibold" style="color: var(--exec-ink);">
				Couldn't reach live work data
			</h2>
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
		<div
			class="grid grid-cols-1 gap-4 md:grid-cols-3"
			aria-busy="true"
			aria-label="Loading executive overview"
		>
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
		<!-- Zone 1b: 3 Executive KPI Pillars (Financials, Workforce & Offices, Work & Flow) -->
		<section
			class="hb-pillars grid grid-cols-1 gap-4 lg:grid-cols-3"
			aria-label="Executive overview: Financials, Workforce, and Work & Flow"
		>
			<!-- Pillar 1: Financials / Monthly Spend & Budget (costsApi -> /settings/costs) -->
			<article class="exec-card hb-pillar flex flex-col gap-3 p-4.5">
				<div class="flex items-center justify-between gap-2">
					<a href={resolve('/settings/costs')} class="hb-pil-name inline-flex items-center gap-1.5">
						<DollarSign class="size-3.5" style="color: var(--exec-warn);" aria-hidden="true" />
						<span>Financials</span>
					</a>
					<a
						href={resolve('/settings/costs')}
						class={[
							'hb-pil-pill',
							kpis.financials.pillTone === 'bad'
								? 'is-bad'
								: kpis.financials.pillTone === 'warn'
									? 'is-warn'
									: kpis.financials.pillTone === 'muted'
										? 'is-muted'
										: 'is-ok'
						]}
					>
						{kpis.financials.pillLabel}
					</a>
				</div>
				<a
					href={resolve('/settings/costs')}
					class="hb-pil-hero -m-1 flex flex-col gap-2 rounded-lg p-1"
				>
					<div class="flex items-baseline justify-between gap-2">
						<div class="flex items-baseline gap-1.5">
							<span class="hb-pil-num">{kpis.financials.spentLabel}</span>
							<span class="text-xs font-medium" style="color: var(--exec-muted);">
								{kpis.financials.budgetLabel}
							</span>
						</div>
						{#if kpis.financials.available && kpis.financials.budgetMicroUsd !== null}
							<span class="text-xs font-semibold tabular-nums" style="color: var(--exec-dim);">
								{kpis.financials.usagePct}% used
							</span>
						{/if}
					</div>
					<div class="hb-bar-track" aria-hidden="true">
						<span class="hb-bar-accent" style="width: {kpis.financials.usagePct}%"></span>
					</div>
				</a>
				<div class="hb-pil-sub grid grid-cols-2 gap-3 pt-2.5">
					<a
						href={resolve('/settings/costs')}
						class="hb-sub-cell -m-1 flex flex-col gap-0.5 rounded-md p-1"
					>
						<span class="text-[11px] font-medium" style="color: var(--exec-dim);">
							Cost / completed
						</span>
						<span class="text-sm font-semibold tabular-nums" style="color: var(--exec-ink);">
							{kpis.financials.costPerTaskLabel}
						</span>
					</a>
					<a
						href={resolve('/settings/costs')}
						class="hb-sub-cell -m-1 flex flex-col gap-0.5 rounded-md p-1"
					>
						<span class="text-[11px] font-medium" style="color: var(--exec-dim);">
							Agent budgets
						</span>
						<span class="text-sm font-semibold tabular-nums" style="color: var(--exec-ink);">
							{#if kpis.financials.available}
								{kpis.financials.budgetedAgentsCount} active
							{:else}
								Configure
							{/if}
						</span>
					</a>
				</div>
			</article>

			<!-- Pillar 2: Workforce & Offices (membersApi + officesApi -> /teams/members, /settings/offices) -->
			<article class="exec-card hb-pillar flex flex-col gap-3 p-4.5">
				<div class="flex items-center justify-between gap-2">
					<a href={resolve('/teams/members')} class="hb-pil-name inline-flex items-center gap-1.5">
						<Users class="size-3.5" style="color: var(--exec-accent-ink);" aria-hidden="true" />
						<span>Workforce &amp; Offices</span>
					</a>
					<a
						href={resolve('/settings/offices')}
						class={[
							'hb-pil-pill',
							kpis.officePillTone === 'bad'
								? 'is-bad'
								: kpis.officePillTone === 'muted'
									? 'is-muted'
									: 'is-ok'
						]}
						title={kpis.offlineOffices > 0
							? `Offline: ${kpis.offlineOfficeNames.join(', ')}`
							: 'All offices online'}
					>
						{kpis.officePillLabel}
					</a>
				</div>
				<a
					href={resolve('/teams/members')}
					class="hb-pil-hero -m-1 flex flex-col gap-2 rounded-lg p-1"
				>
					<div class="flex items-baseline justify-between gap-2">
						<div class="flex items-baseline gap-1.5">
							<span class="hb-pil-num">{kpis.totalAgents > 0 ? `${kpis.autonomyPct}%` : '—'}</span>
							<span class="text-xs font-medium" style="color: var(--exec-muted);">autonomous</span>
						</div>
						<span class="text-xs font-semibold tabular-nums" style="color: var(--exec-dim);">
							{kpis.peopleCount}
							{kpis.peopleCount === 1 ? 'person' : 'people'} · {kpis.totalAgents}
							agents
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
					<a
						href={resolve('/teams/members')}
						class="hb-sub-cell -m-1 flex flex-col gap-0.5 rounded-md p-1"
					>
						<span class="truncate text-[11px] font-medium" style="color: var(--exec-dim);">
							Agents
						</span>
						<span
							class="truncate text-sm font-semibold tabular-nums"
							style="color: var(--exec-ink);"
						>
							{kpis.activeAgents} active
							{#if kpis.pausedAgents > 0}
								<span class="ml-1 text-[11px]" style="color: var(--exec-warn);">
									· {kpis.pausedAgents} idle
								</span>
							{/if}
							{#if kpis.unplacedAgents > 0}
								<span class="ml-1 text-[11px]" style="color: var(--exec-bad);">
									· {kpis.unplacedAgents} off
								</span>
							{/if}
						</span>
					</a>
					<a
						href={resolve('/settings/offices')}
						class="hb-sub-cell -m-1 flex flex-col gap-0.5 rounded-md p-1"
						title={kpis.offlineOffices > 0
							? `${kpis.onlineOffices} online · ${kpis.offlineOffices} offline (${kpis.offlineOfficeNames.join(', ')})`
							: `${kpis.onlineOffices} online`}
					>
						<span class="truncate text-[11px] font-medium" style="color: var(--exec-dim);">
							Offices
						</span>
						<span
							class="flex items-baseline gap-1.5 truncate text-[13px] font-semibold tabular-nums"
						>
							<span style="color: var(--exec-ok);">{kpis.onlineOffices} online</span>
							{#if kpis.offlineOffices > 0}
								<span style="color: var(--exec-dim);" aria-hidden="true">·</span>
								<span style="color: var(--exec-bad);">{kpis.offlineOffices} offline</span>
							{/if}
						</span>
					</a>
				</div>
			</article>

			<!-- Pillar 3: Work & Flow / Delivery (workApi -> /work, /projects) -->
			<article class="exec-card hb-pillar flex flex-col gap-3 p-4.5">
				<div class="flex items-center justify-between gap-2">
					<a href={resolve('/work')} class="hb-pil-name inline-flex items-center gap-1.5">
						<Activity class="size-3.5" style="color: var(--exec-ok);" aria-hidden="true" />
						<span>Work &amp; Flow</span>
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
				<a href={resolve('/work')} class="hb-pil-hero -m-1 flex flex-col gap-2 rounded-lg p-1">
					<div class="flex items-baseline justify-between gap-2">
						<div class="flex items-baseline gap-1.5">
							<span class="hb-pil-num"
								>{kpis.doneCount > 0 ? `${kpis.firstTryPassPct}%` : `${kpis.completionPct}%`}</span
							>
							<span class="text-xs font-medium" style="color: var(--exec-muted);">
								{kpis.doneCount > 0 ? 'clean delivery' : 'complete'}
							</span>
						</div>
						<span class="text-xs font-semibold tabular-nums" style="color: var(--exec-dim);">
							{kpis.openCount} open · {kpis.doneCount} done
						</span>
					</div>
					<div class="hb-bar-track" aria-hidden="true">
						<span
							class="hb-bar-ok"
							style="width: {kpis.doneCount > 0 ? kpis.firstTryPassPct : kpis.completionPct}%"
						></span>
					</div>
				</a>
				<div class="hb-pil-sub grid grid-cols-2 gap-3 pt-2.5">
					<a href={resolve('/work')} class="hb-sub-cell -m-1 flex flex-col gap-0.5 rounded-md p-1">
						<span class="text-[11px] font-medium" style="color: var(--exec-dim);">Progress</span>
						<span class="text-sm font-semibold tabular-nums" style="color: var(--exec-ink);">
							{kpis.completionPct}%
							<span class="ml-1 text-[11px] font-semibold" style="color: var(--exec-ok);">
								{kpis.doneCount} done
							</span>
						</span>
					</a>
					<a href={resolve('/inbox')} class="hb-sub-cell -m-1 flex flex-col gap-0.5 rounded-md p-1">
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
		</section>

		<!-- Zone 2: Decision & Approval Queue + Executive Brief -->
		<section
			class="hb-stage grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1.42fr)_minmax(340px,1fr)]"
		>
			<!-- Left: Decision Spotlight (inboxApi + workApi) -->
			<article
				class={[
					'exec-card hb-spot relative flex min-h-[280px] flex-col gap-4 overflow-hidden p-5 sm:p-6',
					currentDecision?.urgent && 'is-urgent'
				]}
				aria-label="Decision Spotlight"
			>
				<div class="hb-rep-only items-baseline justify-between gap-3">
					<h2 class="text-lg font-bold tracking-tight" style="color: var(--exec-ink);">
						Decision &amp; Approval Queue
					</h2>
				</div>

				{#if currentDecision}
					<div class="flex flex-wrap items-center justify-between gap-2">
						<span
							class={[
								'hb-spot-badge inline-flex items-center gap-1.5',
								currentDecision.urgent && 'is-urgent'
							]}
						>
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
											declineReason = '';
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
									onclick={() => {
										spotIdx = (safeIdx - 1 + spotlight.length) % spotlight.length;
										decliningId = undefined;
										declineReason = '';
									}}
									aria-label="Previous decision"
									class="hb-arrow flex size-7 items-center justify-center rounded-full"
								>
									<ChevronLeft class="size-4" aria-hidden="true" />
								</button>
								<button
									type="button"
									disabled={spotlight.length <= 1}
									onclick={() => {
										spotIdx = (safeIdx + 1) % spotlight.length;
										decliningId = undefined;
										declineReason = '';
									}}
									aria-label="Next decision"
									class="hb-arrow flex size-7 items-center justify-center rounded-full"
								>
									<ChevronRight class="size-4" aria-hidden="true" />
								</button>
							</div>
						</div>
					</div>

					<div class="flex flex-1 flex-col gap-3.5">
						<div
							class="flex flex-wrap items-center gap-2 text-xs"
							style="color: var(--exec-muted);"
						>
							<span
								class="hb-av inline-flex size-6 items-center justify-center rounded-full text-[10px] font-bold"
								style="color: var(--exec-ink);"
							>
								{currentDecision.actorInitials}
							</span>
							<span class="font-semibold" style="color: var(--exec-ink);">
								{currentDecision.actorName}
							</span>
							<span aria-hidden="true">·</span>
							<span>{currentDecision.kindLabel}</span>
							<span aria-hidden="true">·</span>
							<span>{currentDecision.ageLabel}</span>
							<span aria-hidden="true">·</span>
							<span>{currentDecision.projectName}</span>
						</div>

						<h2
							class="text-lg leading-snug font-semibold tracking-tight sm:text-xl"
							style="color: var(--exec-ink);"
						>
							{currentDecision.title}
						</h2>

						<div class="hb-rec flex items-start gap-2.5 rounded-xl px-3.5 py-2.5 text-sm">
							<Info
								class="mt-0.5 size-4 shrink-0"
								style="color: var(--exec-accent-ink);"
								aria-hidden="true"
							/>
							<span>{currentDecision.recommendation}</span>
						</div>

						<!-- 3-Column Risk, Impact & Proof Strip -->
						<div class="exec-well grid grid-cols-1 gap-2.5 p-3 sm:grid-cols-3 sm:gap-0 sm:px-1">
							{#each [currentDecision.risk, currentDecision.impact, currentDecision.proof] as cell, ci (cell.label + ci)}
								<div class={['flex min-w-0 items-center gap-2.5 sm:px-3', ci > 0 && 'hb-cell-sep']}>
									<span
										class={[
											'hb-well-ico flex size-8 shrink-0 items-center justify-center rounded-full',
											`tone-${cell.tone}`
										]}
									>
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
										<div class="truncate text-xs font-semibold" style="color: var(--exec-ink);">
											{cell.label}
										</div>
										<div class="truncate text-[11px]" style="color: var(--exec-muted);">
											{cell.sub}
										</div>
									</div>
								</div>
							{/each}
						</div>

						<!-- Inline expandable background context disclosure -->
						<div class="flex flex-col gap-2">
							<button
								type="button"
								onclick={() => toggleContext(currentDecision.id)}
								aria-expanded={currentCtxOpen}
								aria-controls="panel-spot-ctx-{currentDecision.id}"
								class="hb-ctx-toggle inline-flex w-fit items-center gap-1.5 rounded-md px-1.5 text-xs font-semibold"
							>
								<ChevronRight
									class={[
										'hb-ctx-chev size-3.5 transition-transform',
										currentCtxOpen && 'rotate-90'
									]}
									aria-hidden="true"
								/>
								<span>
									{currentCtxOpen ? 'Hide background context' : 'Read background context'}
								</span>
							</button>
							{#if currentCtxOpen}
								<div
									id="panel-spot-ctx-{currentDecision.id}"
									class="hb-ctx-body rounded-lg px-3.5 py-2.5 text-xs leading-relaxed"
								>
									<p>{currentDecision.backgroundContext}</p>
									{#if currentDecision.auditChecks.length > 0}
										<div
											class="hb-ctx-checks mt-2.5 flex flex-wrap gap-x-3.5 gap-y-1.5 pt-2.5"
											role="list"
											aria-label="Decision checks"
										>
											{#each currentDecision.auditChecks as chk (chk)}
												<span class="inline-flex items-center gap-1.5" role="listitem">
													<CheckCircle2
														class="size-3.5 shrink-0"
														style="color: var(--exec-ok);"
														aria-hidden="true"
													/>
													<span>{chk}</span>
												</span>
											{/each}
										</div>
									{/if}
								</div>
							{/if}
						</div>
					</div>

					<!-- Action Bar directly above UP NEXT -->
					<div class="hb-spot-foot flex flex-wrap items-center justify-between gap-3 pt-3.5">
						<div class="hb-spot-actions flex flex-1 flex-wrap items-center gap-2">
							<button
								type="button"
								disabled={busyId === currentDecision.id}
								onclick={() => void runAction('primary')}
								class="exec-cta hb-touch-btn"
							>
								<span class="exec-cta-fill" aria-hidden="true"></span>
								<span class="exec-cta-sheen" aria-hidden="true"></span>
								<span class="exec-cta-inner">
									<Check class="size-4" aria-hidden="true" />
									<span>{currentDecision.primaryAction.label}</span>
								</span>
							</button>
							{#if isDenyAction(currentDecision.secondaryAction)}
								{#if decliningId !== currentDecision.id}
									<span class="hb-act-sep" aria-hidden="true"></span>
									<button
										bind:this={denyTriggerEl}
										type="button"
										disabled={busyId === currentDecision.id}
										onclick={() => void armDeny(currentDecision.id)}
										class="hb-deny-btn hb-touch-btn inline-flex items-center gap-1.5 rounded-lg px-3 text-xs font-semibold"
									>
										<Ban class="size-3.5" aria-hidden="true" />
										<span>{currentDecision.secondaryAction.label}</span>
									</button>
								{/if}
							{:else if currentDecision.secondaryAction}
								<button
									type="button"
									disabled={busyId === currentDecision.id}
									onclick={() => void runAction('secondary')}
									class="hb-outline-btn hb-touch-btn inline-flex items-center gap-1.5 rounded-lg px-3 text-xs font-semibold"
								>
									<span>{currentDecision.secondaryAction.label}</span>
								</button>
							{/if}
						</div>
						<div class="flex items-center gap-2">
							<a
								href={resolve(currentDecision.detailHref as '/work')}
								class="hb-skip-btn hb-touch-btn inline-flex items-center gap-0.5 rounded-lg px-2 text-xs font-medium"
							>
								<span>Details</span>
								<ChevronRight class="size-3.5" aria-hidden="true" />
							</a>
							<a
								href={resolve('/inbox')}
								class="hb-skip-btn hb-touch-btn inline-flex items-center gap-0.5 rounded-lg px-2 text-xs font-medium"
							>
								<span>All {spotlight.length} in Inbox</span>
								<ChevronRight class="size-3.5" aria-hidden="true" />
							</a>
						</div>

						{#if decliningId === currentDecision.id}
							<div
								class="hb-deny-guard flex w-full flex-col gap-2 rounded-lg p-3"
								role="group"
								aria-label="Confirm deny"
							>
								<div class="flex flex-wrap items-center justify-between gap-2.5">
									<span
										class="hb-deny-guard-lbl inline-flex items-center gap-1.5 text-xs font-semibold"
									>
										<Ban class="size-3.5 shrink-0" aria-hidden="true" />
										<span>Confirm deny?</span>
									</span>
									<div class="flex items-center gap-1.5">
										<button
											type="button"
											disabled={busyId === currentDecision.id}
											onclick={() => void runAction('secondary')}
											class="hb-deny-confirm hb-touch-btn inline-flex items-center rounded-md px-3 text-xs font-semibold"
										>
											Confirm
										</button>
										<button
											type="button"
											onclick={() => void cancelDeny()}
											class="hb-skip-btn hb-touch-btn inline-flex items-center rounded-md px-2.5 text-xs font-semibold"
										>
											Keep
										</button>
									</div>
								</div>
								<label class="sr-only" for="deny-msg-{currentDecision.id}">
									Optional reason for {currentDecision.actorName}
								</label>
								<input
									bind:this={denyInputEl}
									bind:value={declineReason}
									onkeydown={handleDenyKeydown}
									id="deny-msg-{currentDecision.id}"
									type="text"
									autocomplete="off"
									placeholder="Optional reason for {currentDecision.actorName}… (Enter to confirm, Esc to cancel)"
									class="hb-input min-h-9 w-full rounded-md px-2.5 py-1.5 text-xs"
								/>
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
								declineReason = '';
							}}
							class="exec-well hb-next-bar flex items-center justify-between gap-2 px-3 py-2 text-left text-xs"
						>
							<span class="flex min-w-0 items-center gap-2">
								<span
									class="shrink-0 text-[10px] font-bold tracking-wider uppercase"
									style="color: var(--exec-dim);"
								>
									Up next
								</span>
								<span class="truncate font-medium" style="color: var(--exec-body);">
									{nextDecision.title}
								</span>
							</span>
							<ChevronRight
								class="size-3.5 shrink-0"
								style="color: var(--exec-dim);"
								aria-hidden="true"
							/>
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
							<a
								href={resolve('/teams/members')}
								class="hb-outline-btn hb-touch-btn inline-flex items-center gap-1.5 rounded-lg px-3.5 text-xs font-semibold"
							>
								<span>Manage Members</span>
							</a>
						</div>
					</div>
				{/if}
			</article>

			<!-- Right: Executive Brief Card with 3 Lenses (Today / 7-Day / Board) -->
			<aside class="exec-card hb-brief-card flex flex-col gap-3.5 p-5" aria-label="Executive Brief">
				<div class="flex flex-wrap items-center justify-between gap-2">
					<h2
						class="inline-flex items-center gap-2 text-sm font-semibold"
						style="color: var(--exec-ink);"
					>
						<Sparkles class="size-4" style="color: var(--exec-accent-ink);" aria-hidden="true" />
						<span>Executive Brief</span>
					</h2>
					<div
						class="hb-brief-lenses inline-flex items-center gap-0.5 rounded-lg p-0.5"
						role="tablist"
						aria-label="Brief timeframe"
					>
						{#each BRIEF_LENSES as option, idx (option.id)}
							<button
								type="button"
								role="tab"
								id="hb-brief-tab-{option.id}"
								aria-selected={briefLens === option.id}
								aria-controls="hb-cos-scroll"
								tabindex={briefLens === option.id ? 0 : -1}
								onclick={() => {
									briefLens = option.id;
									briefCopied = false;
								}}
								onkeydown={(e) => handleBriefTabKeydown(e, idx)}
								class={[
									'hb-lens-tab rounded-md px-2.5 py-1 text-[11px] font-semibold',
									briefLens === option.id && 'is-on'
								]}
							>
								{option.label}
							</button>
						{/each}
					</div>
				</div>

				<div
					class={[
						'hb-cos-memo flex min-h-0 flex-1 flex-col gap-3 rounded-xl p-4',
						briefExpanded && 'is-expanded'
					]}
				>
					<div
						id="hb-cos-scroll"
						role="tabpanel"
						aria-labelledby="hb-brief-tab-{briefLens}"
						class="hb-cos-scroll flex min-h-0 flex-1 flex-col gap-3.5"
						tabindex="0"
					>
						{#each activeBrief.sections as sec (sec.id)}
							<div class="flex flex-col gap-1">
								<span class={['hb-cos-k', `tone-${sec.tone}`]}>{sec.kicker}</span>
								<p class="text-[13px] leading-relaxed" style="color: var(--exec-body);">
									{sec.body}
								</p>
							</div>
						{/each}

						{#if briefLens === 'today' && catchUp.length > 0}
							<div class="flex flex-col gap-1.5 pt-1">
								<span class="hb-cos-k tone-accent">Recent Catch-up</span>
								<div class="flex flex-col gap-1.5">
									{#each catchUp as row (row.id)}
										<a
											href={resolve(row.href as '/activity')}
											class="hb-cu-row flex items-start gap-2 rounded-lg p-2 text-xs"
										>
											<span class="mt-0.5 flex size-3.5 shrink-0 items-center justify-center">
												{#if row.tone === 'ok'}
													<CheckCircle2
														class="size-3.5"
														style="color: var(--exec-ok);"
														aria-hidden="true"
													/>
												{:else if row.tone === 'warn'}
													<AlertTriangle
														class="size-3.5"
														style="color: var(--exec-warn);"
														aria-hidden="true"
													/>
												{:else if row.tone === 'live'}
													<span class="size-2 rounded-full" style="background: var(--exec-ok);"
													></span>
												{:else}
													<Clock
														class="size-3.5"
														style="color: var(--exec-accent-ink);"
														aria-hidden="true"
													/>
												{/if}
											</span>
											<span class="min-w-0 flex-1 leading-snug" style="color: var(--exec-body);">
												<strong style="color: var(--exec-ink);">{row.title}</strong> — {row.detail}
											</span>
											<span
												class="shrink-0 text-[11px] tabular-nums"
												style="color: var(--exec-dim);"
											>
												{row.timeLabel}
											</span>
										</a>
									{/each}
								</div>
							</div>
						{/if}
					</div>

					<button
						type="button"
						onclick={() => (briefExpanded = !briefExpanded)}
						aria-expanded={briefExpanded}
						aria-controls="hb-cos-scroll"
						class="hb-cos-expand hb-touch-btn items-center gap-1.5 self-start rounded-md px-3 text-xs font-semibold"
					>
						<span>{briefExpanded ? 'Show less' : 'Show full brief'}</span>
						<ChevronDown
							class={['size-3.5 transition-transform', briefExpanded && 'rotate-180']}
							aria-hidden="true"
						/>
					</button>

					<div class="hb-cos-sig mt-auto flex flex-wrap items-center justify-between gap-2 pt-3">
						<a
							href={resolve('/activity')}
							class="inline-flex items-center gap-1 text-xs font-medium"
							style="color: var(--exec-muted);"
						>
							<span>Full activity log</span>
							<ChevronRight class="size-3.5" aria-hidden="true" />
						</a>
						<div class="flex items-center gap-2">
							<span class="text-[11px]" style="color: var(--exec-dim);">
								{activeBrief.updatedLabel}
							</span>
							<button
								type="button"
								onclick={copyBrief}
								aria-live="polite"
								class={[
									'hb-cos-copy hb-touch-btn inline-flex items-center gap-1.5 rounded-md px-2.5 text-xs font-semibold',
									briefCopied && 'is-copied'
								]}
							>
								{#if briefCopied}
									<Check class="size-3.5" aria-hidden="true" />
									<span>Copied</span>
								{:else}
									<Clipboard class="size-3.5" aria-hidden="true" />
									<span>Copy brief</span>
								{/if}
							</button>
						</div>
					</div>
				</div>
			</aside>
		</section>

		<!-- Zone 3: Project Delivery Scorecard (projectsApi + workApi) -->
		{#if projectCards.length > 0}
			<section class="hb-proj-sec flex flex-col gap-3.5" aria-labelledby="hb-projects-heading">
				<div class="flex items-baseline justify-between gap-3">
					<div class="flex items-baseline gap-2.5">
						<h2
							id="hb-projects-heading"
							class="text-[15px] font-semibold"
							style="color: var(--exec-ink);"
						>
							Project Delivery Scorecard
						</h2>
						<span class="text-xs" style="color: var(--exec-muted);">
							{projectCards.length} active{kpis.projectsAttention > 0
								? ` · ${kpis.projectsAttention} need attention`
								: ''}
						</span>
					</div>
					<a
						href={resolve('/projects')}
						class="inline-flex items-center gap-0.5 text-xs font-semibold"
						style="color: var(--exec-accent-ink);"
					>
						<span>All projects</span>
						<ChevronRight class="size-3.5" aria-hidden="true" />
					</a>
				</div>
				<div class="hb-bento grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
					{#each projectCards.slice(0, 6) as proj (proj.id)}
						<article class="exec-card hb-proj-card flex flex-col gap-3.5 p-5">
							<div class="flex items-center justify-between gap-2">
								<div class="flex min-w-0 items-baseline gap-2">
									<a
										href={resolve(proj.workHref as '/projects')}
										class="truncate text-[15px] font-semibold hover:underline"
										style="color: var(--exec-ink);"
									>
										{proj.name}
									</a>
									{#if proj.key}
										<span class="hb-key-tag font-mono text-[11px]">{proj.key}</span>
									{/if}
								</div>
								<span class={['hb-pil-pill inline-flex items-center gap-1', proj.statusPillClass]}>
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

							<a href={resolve(proj.workHref as '/projects')} class="flex flex-col gap-1.5">
								<div
									class="flex items-baseline justify-between text-xs tabular-nums"
									style="color: var(--exec-muted);"
								>
									<span>
										{proj.doneCount} of {proj.totalCount} done{proj.dueLabel
											? ` · Due ${proj.dueLabel}`
											: ''}
									</span>
									<strong class="text-sm" style="color: var(--exec-ink);">{proj.pct}%</strong>
								</div>
								<div class="hb-bar-track" aria-hidden="true">
									<span class={`hb-bar-${proj.healthTone}`} style="width: {proj.pct}%"></span>
								</div>
							</a>

							<div
								class={[
									'hb-blocker flex min-h-[56px] items-center justify-between gap-2.5 rounded-xl px-3 py-2 text-xs',
									`tone-${proj.blockerTone}`
								]}
							>
								<span class="min-w-0 flex-1 leading-snug">{proj.blockerText}</span>
								<a
									href={resolve(proj.actionHref as '/work')}
									class="hb-unblock hb-touch-btn inline-flex shrink-0 items-center gap-0.5 rounded-md px-2.5 py-1 text-[11px] font-semibold"
								>
									<span>{proj.actionLabel}</span>
									<ChevronRight class="size-3" aria-hidden="true" />
								</a>
							</div>

							<div
								class="hb-spot-foot mt-auto flex items-center justify-between gap-2 pt-3 text-xs"
								style="color: var(--exec-muted);"
							>
								<div class="flex items-center">
									{#each proj.memberInitials as init, idx (init + idx)}
										<span
											class="hb-av hb-av-stack inline-flex size-6 items-center justify-center rounded-full text-[9px] font-bold"
											style="color: var(--exec-ink);"
										>
											{init}
										</span>
									{/each}
									{#if proj.extraMembers > 0}
										<span class="ml-1.5 text-[11px]">+{proj.extraMembers}</span>
									{/if}
								</div>
								<a
									href={resolve(proj.workHref as '/projects')}
									class="inline-flex items-center gap-0.5 hover:underline"
								>
									<span>Lead {proj.leadName}</span>
									<ChevronRight class="size-3" aria-hidden="true" />
								</a>
							</div>
						</article>
					{/each}
				</div>
			</section>
		{/if}

		<!-- Zone 4: Capacity & Task Time Breakdown -->
		{#if taskTime.totalOpen > 0}
			<section
				class="exec-card hb-time-card flex flex-col gap-3 p-5"
				aria-labelledby="hb-time-heading"
			>
				<div class="flex flex-wrap items-baseline justify-between gap-2">
					<h2
						id="hb-time-heading"
						class="text-[15px] font-semibold"
						style="color: var(--exec-ink);"
					>
						Where task time goes
					</h2>
					<span class="text-xs tabular-nums" style="color: var(--exec-dim);">
						{taskTime.totalOpen} open {taskTime.totalOpen === 1 ? 'item' : 'items'}
					</span>
				</div>

				<div
					class="hb-time-bar flex h-2.5 gap-1 overflow-hidden rounded-full"
					role="group"
					aria-label="Open task distribution"
				>
					{#if taskTime.workingPct > 0}
						<a
							href={resolve('/teams/members')}
							class="hb-seg-agents rounded-full"
							style="width: {taskTime.workingPct}%"
							aria-label="Agents working: {taskTime.workingPct}%"
						></a>
					{/if}
					{#if taskTime.reviewPct > 0}
						<a
							href={resolve('/work')}
							class="hb-seg-ci rounded-full"
							style="width: {taskTime.reviewPct}%"
							aria-label="In review: {taskTime.reviewPct}%"
						></a>
					{/if}
					{#if taskTime.waitingYouPct > 0}
						<a
							href={resolve('/inbox')}
							class="hb-seg-human rounded-full"
							style="width: {taskTime.waitingYouPct}%"
							aria-label="Waiting on you: {taskTime.waitingYouPct}%"
						></a>
					{/if}
					{#if taskTime.queuedPct > 0}
						<a
							href={resolve(kpis.offlineOffices > 0 ? '/settings/offices' : '/work')}
							class={[
								'rounded-full',
								taskTime.queuedIsHigh ? 'hb-seg-queued-high' : 'hb-seg-queued'
							]}
							style="width: {taskTime.queuedPct}%"
							aria-label="Waiting for an agent: {taskTime.queuedPct}%"
						></a>
					{/if}
				</div>

				<div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs">
					<a href={resolve('/teams/members')} class="inline-flex items-center gap-1.5">
						<span class="size-2 rounded-full" style="background: var(--tone-accent);"></span>
						<strong style="color: var(--exec-sky);">{taskTime.workingPct}%</strong>
						<span style="color: var(--exec-muted);">Agents working ({taskTime.workingCount})</span>
					</a>
					<a href={resolve('/work')} class="inline-flex items-center gap-1.5">
						<span class="size-2 rounded-full" style="background: var(--tone-ok);"></span>
						<strong style="color: var(--exec-ok);">{taskTime.reviewPct}%</strong>
						<span style="color: var(--exec-muted);">In review ({taskTime.reviewCount})</span>
					</a>
					<a href={resolve('/inbox')} class="inline-flex items-center gap-1.5">
						<span class="size-2 rounded-full" style="background: var(--tone-warn);"></span>
						<strong style="color: var(--exec-warn);">{taskTime.waitingYouPct}%</strong>
						<span style="color: var(--exec-muted);"
							>Waiting on you ({taskTime.waitingYouCount})</span
						>
					</a>
					<a
						href={resolve(kpis.offlineOffices > 0 ? '/settings/offices' : '/work')}
						class="inline-flex items-center gap-1.5"
					>
						<span
							class="size-2 rounded-full"
							style="background: {taskTime.queuedIsHigh
								? 'var(--tone-bad)'
								: 'var(--tone-queued)'};"
						></span>
						<strong
							style="color: {taskTime.queuedIsHigh ? 'var(--exec-bad)' : 'var(--exec-purple)'};"
						>
							{taskTime.queuedPct}%
						</strong>
						<span style="color: var(--exec-muted);">
							Waiting for an agent ({taskTime.queuedCount})
						</span>
					</a>
				</div>

				<div class="flex flex-wrap items-center justify-between gap-2 text-xs">
					<p style="color: var(--exec-muted);">{taskTime.summary}</p>
					<div class="flex items-center gap-3.5">
						<a
							href={resolve(taskTime.primaryLink.href as '/work')}
							class="inline-flex items-center gap-0.5 font-semibold"
							style="color: var(--exec-accent-ink);"
						>
							<span>{taskTime.primaryLink.label}</span>
							<ChevronRight class="size-3.5" aria-hidden="true" />
						</a>
						{#if taskTime.secondaryLink}
							<a
								href={resolve(taskTime.secondaryLink.href as '/work')}
								class="inline-flex items-center gap-0.5 font-semibold"
								style="color: var(--exec-accent-ink);"
							>
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
	.hb-pil-pill.is-muted {
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border);
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
		background: var(--bar-track);
	}
	.hb-bar-track > span {
		display: block;
		height: 100%;
		border-radius: 999px;
	}
	.hb-bar-accent {
		background: var(--tone-accent-bar);
	}
	.hb-bar-sky {
		background: var(--tone-accent-bar);
	}
	.hb-bar-ok {
		background: var(--tone-ok-bar);
	}
	.hb-bar-warn {
		background: var(--tone-warn-bar);
	}
	.hb-bar-bad {
		background: var(--tone-bad-bar);
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
		background: linear-gradient(90deg, var(--tone-warn) 0%, transparent 60%);
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
		transition:
			width 0.2s ease,
			background-color 0.16s ease;
	}
	.hb-dot.is-urgent {
		background: var(--tone-warn);
	}
	.hb-dot.is-cur {
		width: 22px;
		background: var(--tone-accent);
	}
	.hb-dot.is-cur.is-urgent {
		background: var(--tone-warn);
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
		box-shadow:
			inset 0 0 0 1px var(--exec-accent-ring),
			inset 3px 0 0 var(--tone-accent);
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
	.hb-ctx-toggle {
		min-height: 32px;
		color: var(--exec-accent-ink);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.hb-ctx-toggle:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.hb-ctx-body {
		background: var(--exec-well);
		color: var(--exec-body);
		box-shadow: inset 2px 0 0 var(--tone-accent);
	}
	.hb-ctx-checks {
		border-top: 1px solid var(--exec-border);
		color: var(--exec-muted);
	}
	.hb-spot-foot {
		border-top: 1px solid var(--exec-border);
	}
	.hb-act-sep {
		width: 1px;
		height: 20px;
		background: var(--exec-border-hi);
		margin: 0 2px;
		flex: none;
	}
	.hb-touch-btn {
		min-height: 36px;
	}
	.hb-outline-btn {
		background: var(--exec-well);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.hb-outline-btn:hover {
		background: var(--exec-surface-hi);
	}
	.hb-deny-btn {
		background: var(--exec-deny-bg);
		color: var(--exec-deny-ink);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.hb-deny-btn:hover {
		background: var(--exec-deny-bg-hover);
		color: var(--exec-deny-ink-hover);
		box-shadow: inset 0 0 0 1px var(--exec-deny-ring-hover);
	}
	.hb-deny-guard {
		background: var(--exec-deny-guard-bg);
		box-shadow: inset 0 0 0 1px var(--exec-deny-guard-ring);
	}
	.hb-deny-guard-lbl {
		color: var(--exec-deny-ink);
	}
	.hb-deny-confirm {
		background: var(--exec-deny-confirm-bg);
		color: var(--exec-deny-confirm-ink);
		min-height: 32px;
	}
	.hb-deny-confirm:hover {
		background: var(--exec-deny-confirm-hover);
	}
	.hb-skip-btn {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.hb-skip-btn:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.hb-input {
		background: var(--exec-surface);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
	}
	.hb-next-bar:hover,
	.hb-cu-row:hover {
		background: var(--exec-surface-hi);
	}
	/* Executive Brief 3-Lens Toggle & Memo */
	.hb-brief-lenses {
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.hb-lens-tab {
		color: var(--exec-muted);
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.hb-lens-tab:hover {
		color: var(--exec-ink);
	}
	.hb-lens-tab.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.hb-cos-memo {
		background: var(--exec-cos-bg);
		box-shadow:
			inset 3px 0 0 var(--exec-cos-rule),
			inset 0 0 0 1px var(--exec-cos-border);
	}
	.hb-cos-scroll {
		overflow-y: auto;
		padding-right: 4px;
		scrollbar-width: thin;
		scrollbar-color: var(--exec-border-hi) transparent;
	}
	.hb-cos-k {
		font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
		font-size: 0.7rem;
		font-weight: 700;
		letter-spacing: 0.1em;
		text-transform: uppercase;
		color: var(--exec-accent-ink);
	}
	.hb-cos-k.tone-ok {
		color: var(--exec-ok);
	}
	.hb-cos-k.tone-warn {
		color: var(--exec-warn);
	}
	.hb-cos-expand {
		display: none;
		color: var(--exec-accent-ink);
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.hb-cos-expand:hover {
		background: var(--exec-accent);
		color: var(--exec-on-accent);
	}
	.hb-cos-sig {
		border-top: 1px solid var(--exec-border);
	}
	.hb-cos-copy {
		min-height: 32px;
		color: var(--exec-accent-ink);
		background: var(--exec-accent-soft);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
		transition:
			background-color 0.14s ease,
			color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.hb-cos-copy:hover {
		background: var(--exec-accent);
		color: var(--exec-on-accent);
	}
	.hb-cos-copy.is-copied {
		background: var(--exec-ok-bg);
		color: var(--exec-ok);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
	}
	@media (min-width: 1280px) {
		.hb-brief-card {
			height: 0;
			min-height: 100%;
			overflow: hidden;
		}
	}
	@media (max-width: 1279px) {
		.hb-cos-scroll {
			position: relative;
			overflow: hidden;
			max-height: 215px;
			padding-right: 0;
		}
		.hb-cos-scroll::after {
			content: '';
			position: absolute;
			left: 0;
			right: 0;
			bottom: 0;
			height: 52px;
			background: linear-gradient(to bottom, transparent, var(--exec-cos-fade));
			pointer-events: none;
		}
		.hb-cos-memo.is-expanded .hb-cos-scroll {
			max-height: none;
			overflow: visible;
		}
		.hb-cos-memo.is-expanded .hb-cos-scroll::after {
			display: none;
		}
		.hb-cos-expand {
			display: inline-flex;
		}
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
		min-height: 28px;
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
		background: var(--bar-track);
	}
	.hb-time-bar {
		background: var(--bar-track);
	}
	.hb-seg-agents {
		background: var(--exec-seg-agents);
	}
	.hb-seg-ci {
		background: var(--exec-seg-ci);
	}
	.hb-seg-human {
		background: var(--exec-seg-human);
	}
	.hb-seg-queued {
		background: var(--exec-seg-queued);
	}
	.hb-seg-queued-high {
		background: var(--exec-seg-queued-high);
	}
</style>
