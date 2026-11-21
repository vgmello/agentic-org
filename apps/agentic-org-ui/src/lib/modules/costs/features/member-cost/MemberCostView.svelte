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
	import { gutters, mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { newId } from '$lib/modules/organization/commands';
	import { toAppError } from '$lib/proto';
	import { costsApi } from '../../api';
	import { formatDayTime } from '../../dates';
	import { formatCost, formatCount, formatTokens } from '../../money';
	import {
		API_EQUIVALENT_NOTE,
		budgetState,
		costValue,
		incompleteText,
		unknownCostReason,
		zoneSourceText
	} from '../../present';
	import { scoped } from '../../scoped';
	import type {
		BudgetPeriodState,
		MemberProjectSpendRow,
		MemberRecordsFilter,
		MemberTaskSpendRow
	} from '../../types';
	import BudgetBar from '../agent-budgets/BudgetBar.svelte';
	import CostsError from '../states/CostsError.svelte';
	import UsageRecordsSheet from '../usage-records/UsageRecordsSheet.svelte';
	import AgentBudgetDialog from './AgentBudgetDialog.svelte';

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

	let actionBusy = $state<string | undefined>();
	let actionError = $state<string | undefined>();
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
		actionError = undefined;
		budgetDialog?.start(kind, existing);
	}

	async function runBudgetAction(
		periodKind: 'daily' | 'monthly',
		action: 'clear' | 'allow_once' | 'keep_paused'
	) {
		if (actionBusy) return;
		const key = `${periodKind}:${action}`;
		actionBusy = key;
		actionError = undefined;
		try {
			const res = await costsApi.agentBudgetAction(organizationId, memberId, {
				periodKind,
				action,
				operationId: newId()
			});
			toast.success(res.message);
			await costRes.reload();
		} catch (e) {
			actionError = toAppError(e).message;
		} finally {
			actionBusy = undefined;
		}
	}

	async function onBudgetSaved(message: string) {
		toast.success(message);
		await costRes.reload();
	}

	function restoreFocus(event: Event) {
		event.preventDefault();
		if (opener?.isConnected) opener.focus();
	}

	const isReached = (b: BudgetPeriodState) =>
		b.hold === 'stop_pending' || b.hold === 'held' || b.spent >= b.limit;

	const figureClass =
		'inline-flex min-h-6 items-center rounded-sm font-semibold text-link tabular-nums hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11';
</script>

{#snippet budgetCard(
	kind: 'daily' | 'monthly',
	budget: BudgetPeriodState | undefined,
	bZone: string,
	canManage: boolean
)}
	{@const stateWord = budget ? budgetState(budget) : undefined}
	{@const reached = budget ? isReached(budget) : false}
	<div
		class="flex flex-col gap-3 rounded-lg border border-line-subtle bg-surface-raised/30 p-4"
		data-member-budget={kind}
	>
		{#if budget}
			<BudgetBar {budget} {kind} zone={bZone} />
			{#if canManage}
				<div class="flex flex-wrap items-center gap-2 pt-1">
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						disabled={!!actionBusy}
						onclick={(e) => openBudgetDialog(kind, budget, e.currentTarget)}
						>{reached ? 'Raise budget…' : 'Change budget…'}</Button
					>
					{#if reached && !budget.extended}
						<Button
							type="button"
							variant="outline"
							size="sm"
							class={mobileButton}
							disabled={!!actionBusy}
							onclick={() => runBudgetAction(kind, 'allow_once')}
							>{actionBusy === `${kind}:allow_once` ? 'Working…' : 'Allow 20% more once'}</Button
						>
					{/if}
					{#if reached && budget.hold === 'stop_pending'}
						<Button
							type="button"
							variant="outline"
							size="sm"
							class={mobileButton}
							disabled={!!actionBusy}
							onclick={() => runBudgetAction(kind, 'keep_paused')}
							>{actionBusy === `${kind}:keep_paused`
								? 'Working…'
								: 'Keep paused until reset'}</Button
						>
					{/if}
					<Button
						type="button"
						variant="ghost"
						size="sm"
						class={mobileButton}
						disabled={!!actionBusy}
						onclick={() => runBudgetAction(kind, 'clear')}
						>{actionBusy === `${kind}:clear` ? 'Working…' : 'Clear budget'}</Button
					>
				</div>
			{/if}
			{#if stateWord?.tone === 'owner'}
				<p class="text-xs/5 text-fg-muted">
					Raising, extending, or clearing releases only this budget's hold once satisfied.
				</p>
			{/if}
		{:else}
			<div class="flex flex-wrap items-center justify-between gap-2">
				<div>
					<p class="text-sm/6 font-semibold text-fg">
						{kind === 'daily' ? 'Daily budget' : 'Monthly budget'}
					</p>
					<p class="text-xs/5 text-fg-muted">No budget set</p>
				</div>
				{#if canManage}
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						onclick={(e) => openBudgetDialog(kind, undefined, e.currentTarget)}
						>Set {kind} budget…</Button
					>
				{/if}
			</div>
		{/if}
	</div>
{/snippet}

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
			{:else}
				{@const monthInc = incompleteText(data.periods.thisMonth, data.coverage)}
				<dl
					class="mt-4 grid grid-cols-1 gap-px border-y border-line-subtle bg-surface-raised min-[640px]:grid-cols-2 min-[1280px]:grid-cols-4"
					data-testid="member-period-tiles"
				>
					<StatTile
						label="Today"
						value={data.periods.today.records === 0 ? '—' : costValue(data.periods.today)}
						detail={data.periods.today.records === 0
							? 'No records today'
							: `${formatTokens(data.periods.today.knownTokens)} tokens`}
					/>
					<StatTile
						label="This week"
						value={data.periods.thisWeek.records === 0 ? '—' : costValue(data.periods.thisWeek)}
						detail={data.periods.thisWeek.records === 0
							? 'No records this week'
							: `${formatTokens(data.periods.thisWeek.knownTokens)} tokens`}
					/>
					<StatTile
						label="This month"
						value={data.periods.thisMonth.records === 0 ? '—' : costValue(data.periods.thisMonth)}
						detail={data.periods.thisMonth.records === 0
							? 'No records this month'
							: data.periods.thisMonth.knownCostMicroUsd === null
								? unknownCostReason(data.periods.thisMonth)
								: `${formatTokens(data.periods.thisMonth.knownTokens)} tokens`}
					/>
					<StatTile
						label="Last month"
						value={data.periods.lastMonth.records === 0 ? '—' : costValue(data.periods.lastMonth)}
						detail={data.periods.lastMonth.records === 0
							? 'No records last month'
							: `${formatTokens(data.periods.lastMonth.knownTokens)} tokens`}
					/>
				</dl>
				<div class="mt-3 flex flex-col gap-1 text-xs/5 {gutters}">
					{#if data.periods.thisMonth.includesApiEquivalent}
						<p class="text-fg-secondary">{API_EQUIVALENT_NOTE}</p>
					{/if}
					{#if monthInc}
						<p class="text-fg-secondary" data-testid="member-incomplete">{monthInc}</p>
					{/if}
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
			<section
				aria-labelledby="member-budgets-heading"
				class="border-t border-line-subtle py-6 {gutters}"
				data-testid="member-budgets-section"
			>
				<div class="flex flex-wrap items-start justify-between gap-4">
					<div>
						<h3 id="member-budgets-heading" class="text-base/7 font-semibold text-fg">Budgets</h3>
						<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">
							{#if data.budgets.zone.name}
								Daily and monthly limits run in {data.budgets.zone.name}, {zoneSourceText(
									data.budgets.zone.source
								)}, so "today" here can differ from your viewer zone.
							{:else}
								Daily and monthly limits for {agentName}.
							{/if}
						</p>
					</div>
				</div>

				{#if data.budgets.resetNote}
					<p
						class="mt-3 rounded-md border border-line bg-surface-raised px-3 py-2 text-xs/5 text-fg-secondary"
						data-testid="budget-reset-note"
					>
						{data.budgets.resetNote}
					</p>
				{/if}

				{#if actionError}
					<p class="mt-3 text-xs/5 text-danger-text" role="alert">{actionError}</p>
				{/if}

				{#if !data.budgets.zone.name}
					<p class="mt-4 text-sm/6 text-fg-secondary" data-testid="budget-zone-unknown">
						Agent budgets can't run yet: {zoneSourceText(data.budgets.zone.source)} isn't known, and budget
						days and months are never counted in UTC by default.
					</p>
				{:else if !data.budgets.daily && !data.budgets.monthly}
					<div
						class="mt-4 flex flex-wrap items-center justify-between gap-4 rounded-lg border border-line-subtle bg-surface-raised/30 p-4"
						data-testid="member-no-budget"
					>
						<div>
							<p class="text-sm/6 font-semibold text-fg">No budget set</p>
							<p class="text-xs/5 text-fg-muted">
								Set an optional daily or monthly limit in US dollars or total tokens.
							</p>
						</div>
						{#if data.budgets.canManage}
							<div class="flex flex-wrap gap-2">
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={(e) => openBudgetDialog('daily', undefined, e.currentTarget)}
									>Set budget…</Button
								>
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={(e) => openBudgetDialog('monthly', undefined, e.currentTarget)}
									>Set monthly budget…</Button
								>
							</div>
						{/if}
					</div>
				{:else}
					<div class="mt-4 grid grid-cols-1 gap-4 min-[1024px]:grid-cols-2">
						{@render budgetCard(
							'daily',
							data.budgets.daily,
							data.budgets.zone.name,
							data.budgets.canManage
						)}
						{@render budgetCard(
							'monthly',
							data.budgets.monthly,
							data.budgets.zone.name,
							data.budgets.canManage
						)}
					</div>
				{/if}
			</section>
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
