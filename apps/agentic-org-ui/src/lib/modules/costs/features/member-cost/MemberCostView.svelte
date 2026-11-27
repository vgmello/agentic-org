<!--
  @component
  Member profile › Cost tab (costs-and-insights-ux §1 Agent profile › Cost, COST-7, §4):
  - Spend by period (Today, This week, This month, Last month) in the viewer's time zone; labelled "in your projects"
    for a project manager.
  - Owner-only daily and monthly calendar budgets in the budget zone (hidden for project managers per COST-7), with
    Set/Change, Clear, Allow 20% more once, and Keep paused until reset controls, plus neutral "Budget reset" notes.
  - Spend by project (with Not on a task; org Not on a task hidden for project managers) and top tasks by cost, each
    opening its effective usage records in UsageRecordsSheet.
  - Explicit states: loading, unavailable, error ("Couldn't load costs. Try again."), empty ("No spend yet" /
    "No spend in your projects yet"), unauthorized ("You don't see costs here").
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import StatTile from '$lib/components/StatTile.svelte';
	import { gutters } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { costsApi } from '../../api';
	import { formatDayTime } from '../../dates';
	import { formatCost, formatCount, formatTokens } from '../../money';
	import { API_EQUIVALENT_NOTE, costValue, incompleteText, unknownCostReason } from '../../present';
	import { scoped } from '../../scoped';
	import type {
		BudgetPeriodState,
		MemberProjectSpendRow,
		MemberRecordsFilter,
		MemberTaskSpendRow,
		UsageTotals
	} from '../../types';
	import CostsError from '../states/CostsError.svelte';
	import UsageRecordsSheet from '../usage-records/UsageRecordsSheet.svelte';
	import AgentBudgetDialog from './AgentBudgetDialog.svelte';
	import MemberBudgetsSection from './MemberBudgetsSection.svelte';

	let {
		organizationId,
		memberId,
		agentName,
		zone
	}: {
		organizationId: string;
		memberId: string;
		agentName: string;
		zone: string;
	} = $props();

	const costRes = new Resource(() =>
		scoped(() => costsApi.memberCost(organizationId, memberId, { timeZone: zone }))
	);

	const scopedData = $derived(costRes.data);
	const data = $derived(scopedData?.allowed ? scopedData.value : undefined);

	let sheet = $state<ReturnType<typeof UsageRecordsSheet>>();
	let budgetDialog = $state<ReturnType<typeof AgentBudgetDialog>>();
	let opener: HTMLElement | null = null;

	const hasAnySpend = $derived.by(() => {
		if (!data) return false;
		return (
			data.periods.today.records > 0 ||
			data.periods.thisWeek.records > 0 ||
			data.periods.thisMonth.records > 0 ||
			data.periods.lastMonth.records > 0 ||
			data.byProject.length > 0
		);
	});

	/** Aggregated totals across all visible records (disjoint byProject partition) for page-level completeness notes. */
	const combinedTotals = $derived.by((): UsageTotals | undefined => {
		if (!data) return undefined;
		const sources =
			data.byProject.length > 0
				? data.byProject.map((r) => r.totals)
				: [
						data.periods.today,
						data.periods.thisWeek,
						data.periods.thisMonth,
						data.periods.lastMonth
					];
		let records = 0;
		let knownCostMicroUsd: number | null = null;
		let knownTokens: number | null = null;
		let cacheReadTokens: number | null = null;
		let tokenIncompleteRecords = 0;
		let costIncompleteRecords = 0;
		let unpricedRecords = 0;
		let unknownCostRecords = 0;
		let includesApiEquivalent = false;
		for (const s of sources) {
			records += s.records;
			if (s.knownCostMicroUsd !== null)
				knownCostMicroUsd = (knownCostMicroUsd ?? 0) + s.knownCostMicroUsd;
			if (s.knownTokens !== null) knownTokens = (knownTokens ?? 0) + s.knownTokens;
			if (s.cacheReadTokens !== null) cacheReadTokens = (cacheReadTokens ?? 0) + s.cacheReadTokens;
			tokenIncompleteRecords += s.tokenIncompleteRecords;
			costIncompleteRecords += s.costIncompleteRecords;
			unpricedRecords += s.unpricedRecords;
			unknownCostRecords += s.unknownCostRecords;
			if (s.includesApiEquivalent) includesApiEquivalent = true;
		}
		if (
			data.periods.today.includesApiEquivalent ||
			data.periods.thisWeek.includesApiEquivalent ||
			data.periods.thisMonth.includesApiEquivalent ||
			data.periods.lastMonth.includesApiEquivalent
		) {
			includesApiEquivalent = true;
		}
		return {
			records,
			knownCostMicroUsd,
			knownTokens,
			cacheReadTokens,
			tokenIncompleteRecords,
			costIncompleteRecords,
			unpricedRecords,
			unknownCostRecords,
			includesApiEquivalent
		};
	});

	function periodTileDetail(totals: UsageTotals, emptyLabel: string): string {
		if (totals.records === 0) return emptyLabel;
		if (totals.knownCostMicroUsd === null) return unknownCostReason(totals);
		return `${formatTokens(totals.knownTokens)} tokens`;
	}

	function openRecords(title: string, filter: MemberRecordsFilter, from: HTMLElement) {
		opener = from;
		sheet?.showCustom(title, `${agentName} · effective records, newest first`, (cursor) =>
			costsApi.memberRecords(organizationId, memberId, { timeZone: zone, filter }, cursor)
		);
	}

	function openProjectRow(row: MemberProjectSpendRow, from: HTMLElement) {
		const filter: MemberRecordsFilter = row.projectId
			? { kind: 'project', projectId: row.projectId, ...(row.notOnTask ? { notOnTask: true } : {}) }
			: { kind: 'org_not_on_task' };
		openRecords(`Usage records: ${row.label}`, filter, from);
	}

	function openTaskRow(row: MemberTaskSpendRow, from: HTMLElement) {
		openRecords(`Usage records: ${row.taskRef}`, { kind: 'task', taskRef: row.taskRef }, from);
	}

	function openBudgetDialog(
		kind: 'daily' | 'monthly',
		existing: BudgetPeriodState | undefined,
		from: HTMLElement
	) {
		opener = from;
		budgetDialog?.start(kind, existing);
	}

	async function onBudgetSaved(message: string) {
		toast.success(message);
		await costRes.reload();
	}

	function restoreFocus(event: Event) {
		event.preventDefault();
		if (opener?.isConnected) opener.focus();
	}

	const figureClass =
		'inline-flex min-h-6 items-center rounded-sm font-semibold text-link tabular-nums hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11';
</script>

{#if costRes.state === 'unavailable'}
	<NotBuilt title="Cost" place="Cost · {agentName}" area="Teams" />
{:else if costRes.state === 'loading'}
	<div role="status" aria-busy="true" class="py-6">
		<span class="sr-only">Loading {agentName}'s cost</span>
		<div
			class="grid grid-cols-1 gap-px bg-surface-raised min-[640px]:grid-cols-2 min-[1280px]:grid-cols-4"
		>
			{#each ['today', 'week', 'month', 'last'] as tile (tile)}
				<div aria-hidden="true" class="bg-surface px-4 py-6 min-[640px]:px-6">
					<span
						class="block h-4 w-20 animate-pulse rounded bg-surface-raised motion-reduce:animate-none"
					></span>
					<span
						class="mt-3 block h-8 w-28 animate-pulse rounded bg-surface-overlay motion-reduce:animate-none"
					></span>
				</div>
			{/each}
		</div>
		<div class="mt-6"><SkeletonRows label="agent cost breakdown" rows={3} /></div>
	</div>
{:else if costRes.state === 'unsupported'}
	<div class="py-6 {gutters}" data-testid="resource-unsupported">
		<h3 class="text-sm/6 font-semibold text-fg">Not available in this version</h3>
		<p class="mt-1 max-w-xl text-sm/6 text-fg-muted [overflow-wrap:anywhere]">
			{costRes.message || "This version of the office doesn't support member cost."}
		</p>
	</div>
{:else if costRes.state === 'error'}
	<div class="py-6">
		<CostsError
			title="Couldn't load costs"
			message={costRes.message}
			onretry={() => costRes.reload()}
			retrying={costRes.reloading}
		/>
	</div>
{:else if scopedData && !scopedData.allowed}
	<div class="py-6 {gutters}" data-testid="member-cost-not-authorized">
		<EmptyState
			level={3}
			title="You don't see costs here"
			description={scopedData.reason ||
				'Costs show to the owner, and to project managers for their projects.'}
		/>
	</div>
{:else if data}
	{@const inYourProjects = data.scope === 'managed'}
	<div data-testid="member-cost-tab" data-scope={data.scope}>
		<section aria-labelledby="member-spend-heading" class="py-6">
			<div class="flex flex-wrap items-baseline justify-between gap-2 {gutters}">
				<div>
					<h3 id="member-spend-heading" class="text-base/7 font-semibold text-fg">
						Spend{inYourProjects ? ' in your projects' : ''}
					</h3>
					<p class="mt-1 text-xs/5 text-fg-muted">
						Calendar periods in your time zone ({data.timeZone}).
						{#if inYourProjects}
							Counts only records attributed to projects you manage.
						{/if}
					</p>
				</div>
				{#if hasAnySpend}
					<button
						type="button"
						class="inline-flex min-h-6 items-center rounded-sm text-xs/5 font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
						onclick={(e) =>
							openRecords(`All usage records: ${agentName}`, { kind: 'all' }, e.currentTarget)}
						>Open all usage records</button
					>
				{/if}
			</div>

			{#if !hasAnySpend}
				<EmptyState
					level={4}
					title={inYourProjects ? 'No spend in your projects yet' : 'No spend yet'}
					description="No usage records were counted here yet. Usage that hasn't reported yet isn't here, so this is not proof of zero use."
					class="py-10"
				/>
				<div class="flex flex-col gap-1 text-xs/5 {gutters}">
					{#each data.coverage.gaps as gap (gap.source)}
						<p class="text-fg-muted">{gap.source}: {gap.reason}</p>
					{/each}
					<p class="text-fg-muted">
						Counted up to the ledger checkpoint of {formatDayTime(
							new Date(data.coverage.checkpointAt),
							data.timeZone
						)}.
					</p>
				</div>
			{:else}
				{@const overallInc = combinedTotals
					? incompleteText(combinedTotals, data.coverage)
					: undefined}
				<dl
					class="mt-4 grid grid-cols-1 gap-px border-y border-line-subtle bg-surface-raised min-[640px]:grid-cols-2 min-[1280px]:grid-cols-4"
					data-testid="member-period-tiles"
				>
					<StatTile
						label="Today"
						value={data.periods.today.records === 0 ? '—' : costValue(data.periods.today)}
						detail={periodTileDetail(data.periods.today, 'No records today')}
					/>
					<StatTile
						label="This week"
						value={data.periods.thisWeek.records === 0 ? '—' : costValue(data.periods.thisWeek)}
						detail={periodTileDetail(data.periods.thisWeek, 'No records this week')}
					/>
					<StatTile
						label="This month"
						value={data.periods.thisMonth.records === 0 ? '—' : costValue(data.periods.thisMonth)}
						detail={periodTileDetail(data.periods.thisMonth, 'No records this month')}
					/>
					<StatTile
						label="Last month"
						value={data.periods.lastMonth.records === 0 ? '—' : costValue(data.periods.lastMonth)}
						detail={periodTileDetail(data.periods.lastMonth, 'No records last month')}
					/>
				</dl>
				<div class="mt-3 flex flex-col gap-1 text-xs/5 {gutters}">
					{#if combinedTotals?.includesApiEquivalent}
						<p class="text-fg-secondary">{API_EQUIVALENT_NOTE}</p>
					{/if}
					{#if overallInc}
						<p class="text-fg-secondary" data-testid="member-incomplete">{overallInc}</p>
					{/if}
					{#each data.coverage.gaps as gap (gap.source)}
						<p class="text-fg-muted">{gap.source}: {gap.reason}</p>
					{/each}
					<p class="text-fg-muted">
						Counted up to the ledger checkpoint of {formatDayTime(
							new Date(data.coverage.checkpointAt),
							data.timeZone
						)}.
					</p>
				</div>
			{/if}
		</section>

		{#if data.budgets}
			<MemberBudgetsSection
				{organizationId}
				{memberId}
				{agentName}
				budgets={data.budgets}
				onOpenDialog={openBudgetDialog}
				onReload={() => costRes.reload()}
			/>
		{/if}

		{#if data.byProject.length > 0}
			<section
				aria-labelledby="member-by-project-heading"
				class="border-t border-line-subtle py-6"
				data-testid="member-by-project"
			>
				<div class={gutters}>
					<h3 id="member-by-project-heading" class="text-base/7 font-semibold text-fg">
						By project{inYourProjects ? ' (in your projects)' : ''}
					</h3>
				</div>
				<ul role="list" class="mt-3 divide-y divide-line-subtle border-y border-line-subtle">
					{#each data.byProject as row (row.id)}
						{@const inc = incompleteText(row.totals)}
						<li
							class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 py-4 {gutters}"
							data-member-project={row.id}
						>
							<div class="min-w-0 flex-1 basis-52">
								<p class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
									{#if row.projectId && !row.notOnTask}
										<a
											href={resolve(
												`/projects/${encodeURIComponent(row.projectId)}/insights` as '/'
											)}
											class="rounded-sm text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
											>{row.label}</a
										>
									{:else}
										{row.label}
									{/if}
									{#if row.archived}
										<span class="ml-1.5 text-xs/5 font-normal text-fg-muted">archived</span>
									{/if}
								</p>
								<p class="mt-0.5 text-xs/5 text-fg-muted tabular-nums">
									{formatTokens(row.totals.knownTokens)} tokens · {formatCount(row.totals.records)}
									{row.totals.records === 1 ? 'record' : 'records'}
								</p>
								{#if inc}
									<p class="mt-0.5 text-xs/5 text-fg-muted">{inc}</p>
								{/if}
							</div>
							<button
								type="button"
								class="text-sm/6 {figureClass}"
								aria-label="{formatCost(
									row.totals.knownCostMicroUsd
								)}. Open records for {row.label}"
								onclick={(e) => openProjectRow(row, e.currentTarget)}
								>{formatCost(row.totals.knownCostMicroUsd)}</button
							>
						</li>
					{/each}
				</ul>
			</section>
		{/if}

		{#if data.topTasks.length > 0}
			<section
				aria-labelledby="member-top-tasks-heading"
				class="border-t border-line-subtle py-6"
				data-testid="member-top-tasks"
			>
				<div class={gutters}>
					<h3 id="member-top-tasks-heading" class="text-base/7 font-semibold text-fg">
						Top tasks by cost{inYourProjects ? ' (in your projects)' : ''}
					</h3>
				</div>
				<ul role="list" class="mt-3 divide-y divide-line-subtle border-y border-line-subtle">
					{#each data.topTasks as task (task.taskRef)}
						<li
							class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 py-4 {gutters}"
							data-member-task={task.taskRef}
						>
							<div class="min-w-0 flex-1 basis-56">
								<p class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
									<a
										href={resolve(`/work/${encodeURIComponent(task.taskRef)}` as '/')}
										class="rounded-sm text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
										><span class="font-mono font-normal">{task.taskRef}</span> {task.title}</a
									>
								</p>
								<p class="mt-0.5 text-xs/5 text-fg-muted tabular-nums">
									{task.projectName ? `${task.projectName} · ` : ''}{formatTokens(
										task.totals.knownTokens
									)} tokens · {formatCount(task.totals.records)}
									{task.totals.records === 1 ? 'record' : 'records'}
								</p>
							</div>
							<button
								type="button"
								class="text-sm/6 {figureClass}"
								aria-label="{formatCost(
									task.totals.knownCostMicroUsd
								)}. Open records for {task.taskRef}"
								onclick={(e) => openTaskRow(task, e.currentTarget)}
								>{formatCost(task.totals.knownCostMicroUsd)}</button
							>
						</li>
					{/each}
				</ul>
			</section>
		{/if}
	</div>

	<UsageRecordsSheet bind:this={sheet} {organizationId} {zone} onCloseAutoFocus={restoreFocus} />
	{#if data.budgets?.zone.name}
		<AgentBudgetDialog
			bind:this={budgetDialog}
			{organizationId}
			{memberId}
			{agentName}
			zone={data.budgets.zone.name}
			onsaved={onBudgetSaved}
			onCloseAutoFocus={restoreFocus}
		/>
	{/if}
{/if}
