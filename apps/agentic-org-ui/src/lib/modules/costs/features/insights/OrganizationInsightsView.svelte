<!--
  @component
  Organization Insights `/insights` (costs-and-insights-ux §1 Insights, §3, INS-1..5):
  org-wide spend overview (4 KPI tiles, coverage/checkpoint bar, actionable Insights list, spend trend)
  and a sortable, filterable Projects table (investment to date, this month, last month, change,
  working time "— not tracked yet", items done) drilling down to `/projects/[id]/insights` and usage records,
  plus safe CSV export. Reached from Home and from Project › Insights.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ArrowUpDownIcon from '@lucide/svelte/icons/arrow-up-down';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import StatTile from '$lib/components/StatTile.svelte';
	import { gutters, inputClass, mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/proto';
	import { costsApi } from '../../api';
	import { buildUsageCsv, downloadCsv } from '../../csv';
	import { formatDayTime } from '../../dates';
	import { formatDuration, WORKING_TIME_NOT_TRACKED } from '../../duration';
	import {
		filterAndSortOrgProjects,
		formatChangePercent,
		type OrgProjectSortKey,
		type SortDir
	} from '../../insights-helpers';
	import { formatCost, formatCount, formatTokens } from '../../money';
	import {
		API_EQUIVALENT_NOTE,
		cacheShareText,
		changeText,
		costValue,
		incompleteText,
		periodLabel,
		periodRange,
		unknownCostReason
	} from '../../present';
	import { scoped } from '../../scoped';
	import type { OrgInsightProjectRow, PeriodRequest } from '../../types';
	import PeriodMenu from '../organization-cost/PeriodMenu.svelte';
	import CostsError from '../states/CostsError.svelte';
	import UsageRecordsSheet from '../usage-records/UsageRecordsSheet.svelte';
	import InsightsList from './InsightsList.svelte';
	import TrendChart from './TrendChart.svelte';

	let {
		organizationId,
		zone
	}: {
		organizationId: string;
		zone: string;
	} = $props();

	const HEADING = 'org-insights-heading';
	const filterId = $props.id();

	let chosen = $state<Omit<PeriodRequest, 'timeZone'>>({ kind: 'this_month' });
	const period = $derived<PeriodRequest>({ ...chosen, timeZone: zone });

	const insightsRes = new Resource(() =>
		scoped(() => costsApi.organizationInsights(organizationId, period))
	);

	const scopedData = $derived(insightsRes.data);
	const data = $derived(scopedData?.allowed ? scopedData.value : undefined);

	let query = $state('');
	let includeArchived = $state(false);
	let sortKey = $state<OrgProjectSortKey>('toDate');
	let sortDir = $state<SortDir>('desc');
	let exporting = $state(false);
	let exportError = $state<string | undefined>();

	let sheet = $state<ReturnType<typeof UsageRecordsSheet>>();
	let opener: HTMLElement | null = null;

	const visibleProjects = $derived(
		data ? filterAndSortOrgProjects(data.projects, query, includeArchived, sortKey, sortDir) : []
	);
	const hasArchived = $derived(data?.projects.some((p) => p.archived) ?? false);

	function changePeriod(next: PeriodRequest) {
		chosen = { kind: next.kind, from: next.from, to: next.to };
		exportError = undefined;
		void insightsRes.reload();
	}

	function toggleSort(key: OrgProjectSortKey) {
		if (sortKey === key) {
			sortDir = sortDir === 'desc' ? 'asc' : 'desc';
		} else {
			sortKey = key;
			sortDir = key === 'name' ? 'asc' : 'desc';
		}
	}

	function ariaSort(key: OrgProjectSortKey): 'ascending' | 'descending' | 'none' {
		if (sortKey !== key) return 'none';
		return sortDir === 'asc' ? 'ascending' : 'descending';
	}

	function openOrgRecords(
		title: string,
		fetchPage: (cursor?: string) => Promise<import('../../types').UsageRecordPage>,
		from: HTMLElement
	) {
		opener = from;
		sheet?.showCustom(
			title,
			`${periodLabel(period.kind)} · effective records, newest first`,
			fetchPage
		);
	}

	function openProjectRecords(row: OrgInsightProjectRow, from: HTMLElement) {
		openOrgRecords(
			`Usage records: ${row.name}`,
			(cursor) =>
				costsApi.projectRecords(organizationId, row.projectId, period, { all: true }, cursor),
			from
		);
	}

	async function exportCsv() {
		if (exporting || !data) return;
		exporting = true;
		exportError = undefined;
		try {
			const res = await costsApi.exportRecords(organizationId, { kind: 'organization' }, period);
			const csv = buildUsageCsv(res.records, res.coverage);
			const filename = `organization-costs-${period.kind}.csv`;
			downloadCsv(filename, csv);
			toast.success(
				`Exported ${formatCount(res.records.length)} ${res.records.length === 1 ? 'record' : 'records'} to ${filename}.`
			);
		} catch (e) {
			exportError = toAppError(e).message;
		} finally {
			exporting = false;
		}
	}

	function restoreFocus(event: Event) {
		event.preventDefault();
		if (opener?.isConnected) opener.focus();
	}

	const sortBtnClass =
		'inline-flex items-center gap-1 rounded-sm font-semibold text-fg-muted hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
	const figureClass =
		'inline-flex min-h-6 items-center rounded-sm font-semibold text-link tabular-nums hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11';
</script>

{#snippet sortIcon(key: OrgProjectSortKey)}
	{#if sortKey === key && sortDir === 'asc'}
		<ArrowUpIcon class="size-3.5" aria-hidden="true" />
	{:else if sortKey === key && sortDir === 'desc'}
		<ArrowDownIcon class="size-3.5" aria-hidden="true" />
	{:else}
		<ArrowUpDownIcon class="size-3.5 opacity-60" aria-hidden="true" />
	{/if}
{/snippet}

{#if insightsRes.state === 'unavailable'}
	<NotBuilt title="Insights" area="Insights" />
{:else}
	<PageHeader
		title="Insights"
		id={HEADING}
		description={data?.viewer.scope === 'managed'
			? 'Spend, tokens and progress across the projects you manage. Amounts are API-equivalent estimates in US dollars.'
			: 'Organization-wide spend, tokens and project investment, valued at API list prices in US dollars.'}
	>
		{#snippet actions()}
			{#if data}
				<PeriodMenu {period} busy={insightsRes.reloading} onchange={changePeriod} />
				{#if data.viewer.canExport}
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={exporting || insightsRes.reloading}
						aria-busy={exporting}
						onclick={exportCsv}
						data-testid="export-csv-btn"
					>
						<DownloadIcon data-icon="inline-start" aria-hidden="true" />
						{exporting ? 'Exporting…' : 'Export CSV'}
					</Button>
				{/if}
			{/if}
		{/snippet}
	</PageHeader>

	{#if insightsRes.state === 'loading'}
		<div role="status" aria-busy="true" class="py-6">
			<span class="sr-only">Loading organization insights</span>
			<div
				class="grid grid-cols-1 gap-px bg-surface-raised min-[640px]:grid-cols-2 min-[1280px]:grid-cols-4"
			>
				{#each ['cost', 'tokens', 'working', 'done'] as tile (tile)}
					<div aria-hidden="true" class="bg-surface px-4 py-6 min-[640px]:px-6">
						<span
							class="block h-4 w-20 animate-pulse rounded bg-surface-raised motion-reduce:animate-none"
						></span>
						<span
							class="mt-3 block h-8 w-32 animate-pulse rounded bg-surface-overlay motion-reduce:animate-none"
						></span>
					</div>
				{/each}
			</div>
			<div class="mt-6"><SkeletonRows label="projects table" rows={4} /></div>
		</div>
	{:else if insightsRes.state === 'unsupported'}
		<div class="py-6 {gutters}" data-testid="resource-unsupported">
			<h3 class="text-sm/6 font-semibold text-fg">Not available in this version</h3>
			<p class="mt-1 max-w-xl text-sm/6 text-fg-muted [overflow-wrap:anywhere]">
				{insightsRes.message || "This version of the office doesn't support organization insights."}
			</p>
		</div>
	{:else if insightsRes.state === 'error'}
		<div class="py-6">
			<CostsError
				title="Couldn't load costs"
				message={insightsRes.message}
				onretry={() => insightsRes.reload()}
				retrying={insightsRes.reloading}
			/>
		</div>
	{:else if scopedData && !scopedData.allowed}
		<div class="py-6 {gutters}" data-testid="org-insights-not-authorized">
			<EmptyState
				level={3}
				title="You don't see costs here"
				description={scopedData.reason ||
					'Costs show to the owner, and to project managers for their projects.'}
			/>
		</div>
	{:else if data}
		{@const totals = data.totals}
		{@const incomplete = incompleteText(totals, data.coverage)}
		<div data-testid="organization-insights">
			{#if exportError}
				<p class="pt-3 text-xs/5 text-danger-text {gutters}" role="alert">{exportError}</p>
			{/if}
			{#if insightsRes.reloading}
				<p
					role="status"
					class="pt-4 text-xs/5 font-semibold text-fg-secondary {gutters}"
					data-testid="period-loading"
				>
					Loading {periodLabel(period.kind)}…
				</p>
			{/if}

			<div
				aria-busy={insightsRes.reloading}
				class="transition-opacity duration-150 motion-reduce:transition-none {insightsRes.reloading
					? 'opacity-50'
					: ''}"
			>
				<p class="pt-4 text-xs/5 text-fg-muted {gutters}" data-testid="period-range">
					{periodLabel(data.period.kind)} · {periodRange(data.period)} · {data.period
						.timeZone}{#if data.viewer.scope === 'managed'}
						· in your projects
					{/if}
				</p>

				{#if totals.records === 0 && data.projects.every((p) => p.toDate.records === 0)}
					<EmptyState
						level={3}
						title="No spend yet"
						description="No usage records were counted for these dates. Usage that hasn't reported yet isn't here, so this is not proof that nothing ran."
						class="py-12"
					/>
				{:else}
					<dl
						class="mt-3 grid grid-cols-1 gap-px border-y border-line-subtle bg-surface-raised min-[640px]:grid-cols-2 min-[1280px]:grid-cols-4"
						data-testid="org-kpi-tiles"
					>
						<StatTile
							label="Cost"
							value={costValue(totals)}
							detail={totals.knownCostMicroUsd === null
								? unknownCostReason(totals)
								: changeText(totals.knownCostMicroUsd, data.previous)}
						/>
						<StatTile
							label="Tokens"
							value={formatTokens(totals.knownTokens)}
							detail={cacheShareText(totals)}
						/>
						<StatTile
							label="Working time"
							value="—"
							detail={data.work.waitingOnYouSeconds !== null
								? `${WORKING_TIME_NOT_TRACKED} · ${formatDuration(data.work.waitingOnYouSeconds)} waiting on you`
								: WORKING_TIME_NOT_TRACKED}
						/>
						<StatTile
							label="Done"
							value="{formatCount(data.work.tasksDone)} {data.work.tasksDone === 1
								? 'task'
								: 'tasks'} · {formatCount(data.work.bugsDone)} {data.work.bugsDone === 1
								? 'bug'
								: 'bugs'}"
							detail={data.work.requestsScoped > 0
								? `${formatCount(data.work.requestsScoped)} ${data.work.requestsScoped === 1 ? 'request' : 'requests'} scoped`
								: undefined}
						/>
					</dl>

					<div class="mt-3 flex flex-col gap-1 pb-6 text-xs/5 {gutters}">
						{#if totals.includesApiEquivalent}
							<p class="text-fg-secondary">{API_EQUIVALENT_NOTE}</p>
						{/if}
						{#if incomplete}
							<p class="text-fg-secondary" data-testid="totals-incomplete">{incomplete}</p>
						{/if}
						{#each data.coverage.gaps as gap (gap.source)}
							<p class="text-fg-muted">{gap.source}: {gap.reason}</p>
						{/each}
						<p class="text-fg-muted">
							Counted up to the ledger checkpoint of {formatDayTime(
								new Date(data.coverage.checkpointAt),
								data.period.timeZone
							)}.
							{#if data.viewer.scope === 'organization' && totals.records > 0}
								<button
									type="button"
									class="inline-flex min-h-6 items-center rounded-sm font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
									onclick={(e) =>
										openOrgRecords(
											'All usage records',
											(cursor) => costsApi.records(organizationId, period, { all: true }, cursor),
											e.currentTarget
										)}>Open all {formatCount(totals.records)} records</button
								>
								·
								<a
									href={resolve('/settings/costs')}
									class="rounded-sm font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
									>Prices and agent budgets</a
								>
							{/if}
						</p>
					</div>

					<InsightsList insights={data.insights} />

					<TrendChart buckets={data.trend} />

					<section
						aria-labelledby="org-projects-heading"
						class="border-t border-line-subtle py-6"
						data-testid="org-projects-section"
					>
						<div class="flex flex-wrap items-end justify-between gap-4 {gutters}">
							<div>
								<h3 id="org-projects-heading" class="text-base/7 font-semibold text-fg">
									Projects
								</h3>
								<p class="mt-1 text-sm/6 text-fg-muted">
									Investment to date, this month vs last month, and items done. Select a project to
									open its Insights.
								</p>
							</div>
							<div class="flex flex-wrap items-center gap-3">
								{#if hasArchived}
									<label class="inline-flex min-h-9 items-center gap-2 text-sm/6 text-fg-secondary">
										<input
											type="checkbox"
											bind:checked={includeArchived}
											class="size-4 rounded border-line bg-surface-raised accent-brand"
										/>
										Show archived
									</label>
								{/if}
								<div class="w-full min-w-0 min-[640px]:w-64">
									<Label for={filterId} class="sr-only">Filter projects</Label>
									<Input
										id={filterId}
										type="search"
										bind:value={query}
										placeholder="Filter projects or team…"
										class="w-full {inputClass}"
										data-testid="org-projects-filter"
									/>
								</div>
							</div>
						</div>

						<div class="mt-4 @container">
							{#if visibleProjects.length === 0}
								<p class="py-8 text-sm/6 text-fg-muted {gutters}" data-testid="org-projects-empty">
									{query.trim()
										? `No projects match "${query.trim()}".`
										: 'No projects with visible cost records.'}
								</p>
							{:else}
								<table class="hidden w-full text-sm/6 @4xl:table" data-testid="org-projects-table">
									<caption class="sr-only">Projects investment and cost overview</caption>
									<thead>
										<tr
											class="border-b border-line text-left text-xs/5 font-semibold text-fg-muted"
										>
											<th
												scope="col"
												aria-sort={ariaSort('name')}
												class="py-3 pr-3 pl-4 min-[640px]:pl-6 min-[1024px]:pl-8"
											>
												<button
													type="button"
													class={sortBtnClass}
													onclick={() => toggleSort('name')}
												>
													Project
													{@render sortIcon('name')}
												</button>
											</th>
											<th scope="col" aria-sort={ariaSort('toDate')} class="px-3 py-3 text-right">
												<button
													type="button"
													class="{sortBtnClass} ml-auto"
													onclick={() => toggleSort('toDate')}
												>
													Investment to date
													{@render sortIcon('toDate')}
												</button>
											</th>
											<th
												scope="col"
												aria-sort={ariaSort('thisMonth')}
												class="px-3 py-3 text-right"
											>
												<button
													type="button"
													class="{sortBtnClass} ml-auto"
													onclick={() => toggleSort('thisMonth')}
												>
													This month
													{@render sortIcon('thisMonth')}
												</button>
											</th>
											<th
												scope="col"
												aria-sort={ariaSort('lastMonth')}
												class="px-3 py-3 text-right"
											>
												<button
													type="button"
													class="{sortBtnClass} ml-auto"
													onclick={() => toggleSort('lastMonth')}
												>
													Last month
													{@render sortIcon('lastMonth')}
												</button>
											</th>
											<th scope="col" aria-sort={ariaSort('change')} class="px-3 py-3 text-right">
												<button
													type="button"
													class="{sortBtnClass} ml-auto"
													onclick={() => toggleSort('change')}
												>
													Change
													{@render sortIcon('change')}
												</button>
											</th>
											<th scope="col" class="px-3 py-3 text-right">Working time</th>
											<th
												scope="col"
												aria-sort={ariaSort('itemsDone')}
												class="py-3 pr-4 pl-3 text-right min-[640px]:pr-6 min-[1024px]:pr-8"
											>
												<button
													type="button"
													class="{sortBtnClass} ml-auto"
													onclick={() => toggleSort('itemsDone')}
												>
													Items done
													{@render sortIcon('itemsDone')}
												</button>
											</th>
										</tr>
									</thead>
									<tbody>
										{#each visibleProjects as proj (proj.projectId)}
											{@const rowInc = incompleteText(proj.toDate)}
											<tr
												class="border-b border-line-subtle align-top"
												data-org-project={proj.projectId}
											>
												<th
													scope="row"
													class="py-4 pr-3 pl-4 text-left font-normal min-[640px]:pl-6 min-[1024px]:pl-8"
												>
													<a
														href={resolve(
															`/projects/${encodeURIComponent(proj.projectId)}/insights` as '/'
														)}
														class="rounded-sm font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
														>{proj.name}</a
													>
													<span class="ml-1.5 font-mono text-xs/5 text-fg-muted">
														P-{proj.projectNumber}
													</span>
													{#if proj.accountableTeam}
														<span class="ml-1.5 text-xs/5 text-fg-muted">
															· {proj.accountableTeam}
														</span>
													{/if}
													{#if proj.archived}
														<span class="ml-1.5 text-xs/5 text-fg-muted">archived</span>
													{/if}
													{#if rowInc}
														<p class="mt-1 text-xs/5 text-fg-muted">{rowInc}</p>
													{/if}
												</th>
												<td class="px-3 py-4 text-right font-semibold text-fg tabular-nums">
													{formatCost(proj.toDate.knownCostMicroUsd)}
												</td>
												<td class="px-3 py-4 text-right">
													<button
														type="button"
														class={figureClass}
														aria-label="{formatCost(
															proj.thisMonth.knownCostMicroUsd
														)} this month. Open records for {proj.name}"
														onclick={(e) => openProjectRecords(proj, e.currentTarget)}
														>{formatCost(proj.thisMonth.knownCostMicroUsd)}</button
													>
												</td>
												<td class="px-3 py-4 text-right text-fg-secondary tabular-nums">
													{formatCost(proj.lastMonth.knownCostMicroUsd)}
												</td>
												<td class="px-3 py-4 text-right text-fg-secondary tabular-nums">
													{formatChangePercent(proj.changePercent)}
												</td>
												<td class="px-3 py-4 text-right text-fg-muted">
													<span aria-hidden="true">—</span>
													<span class="sr-only">Working time {WORKING_TIME_NOT_TRACKED}</span>
												</td>
												<td
													class="py-4 pr-4 pl-3 text-right text-fg-secondary tabular-nums min-[640px]:pr-6 min-[1024px]:pr-8"
												>
													{formatCount(proj.itemsDone)}
												</td>
											</tr>
										{/each}
										{#if data.notOnTask}
											<tr
												class="border-t border-t-line border-b border-line-subtle align-top"
												data-org-project="not-on-a-task"
											>
												<th
													scope="row"
													class="py-4 pr-3 pl-4 text-left font-normal min-[640px]:pl-6 min-[1024px]:pl-8"
												>
													<span class="font-semibold text-fg">Not on a task</span>
													<p class="mt-0.5 text-xs/5 text-fg-muted">
														Team and organization overhead with no project recorded.
													</p>
												</th>
												<td class="px-3 py-4 text-right text-fg-muted">—</td>
												<td class="px-3 py-4 text-right">
													<button
														type="button"
														class={figureClass}
														onclick={(e) =>
															openOrgRecords(
																'Usage records: Not on a task',
																(cursor) =>
																	costsApi.records(
																		organizationId,
																		period,
																		{ dimension: 'project', notOnTask: true },
																		cursor
																	),
																e.currentTarget
															)}>{formatCost(data.notOnTask.knownCostMicroUsd)}</button
													>
												</td>
												<td class="px-3 py-4 text-right text-fg-muted">—</td>
												<td class="px-3 py-4 text-right text-fg-muted">—</td>
												<td class="px-3 py-4 text-right text-fg-muted">—</td>
												<td
													class="py-4 pr-4 pl-3 text-right text-fg-muted min-[640px]:pr-6 min-[1024px]:pr-8"
													>—</td
												>
											</tr>
										{/if}
									</tbody>
								</table>

								<ul
									role="list"
									class="divide-y divide-line-subtle border-y border-line-subtle @4xl:hidden"
								>
									{#each visibleProjects as proj (proj.projectId)}
										{@const rowInc = incompleteText(proj.toDate)}
										<li class="py-4 {gutters}" data-org-project={proj.projectId}>
											<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-1">
												<p class="min-w-0 flex-1 basis-40 text-sm/6 font-semibold text-fg">
													<a
														href={resolve(
															`/projects/${encodeURIComponent(proj.projectId)}/insights` as '/'
														)}
														class="rounded-sm text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
														>{proj.name}</a
													>
													<span class="ml-1.5 font-mono text-xs/5 font-normal text-fg-muted">
														P-{proj.projectNumber}
													</span>
												</p>
												<span class="text-sm/6 font-semibold text-fg tabular-nums">
													{formatCost(proj.toDate.knownCostMicroUsd)} to date
												</span>
											</div>
											<p class="mt-1 text-xs/5 text-fg-secondary tabular-nums">
												This month {formatCost(proj.thisMonth.knownCostMicroUsd)} · Last month {formatCost(
													proj.lastMonth.knownCostMicroUsd
												)} ({formatChangePercent(proj.changePercent)}) · Working: — ({WORKING_TIME_NOT_TRACKED})
												· {formatCount(proj.itemsDone)} items done
											</p>
											{#if rowInc}
												<p class="mt-1 text-xs/5 text-fg-muted">{rowInc}</p>
											{/if}
										</li>
									{/each}
								</ul>
								<p class="mt-3 text-xs/5 text-fg-muted {gutters}">
									Working time: {WORKING_TIME_NOT_TRACKED}.
								</p>
							{/if}
						</div>
					</section>
				{/if}
			</div>
		</div>

		<UsageRecordsSheet bind:this={sheet} {organizationId} {zone} onCloseAutoFocus={restoreFocus} />
	{/if}
{/if}
