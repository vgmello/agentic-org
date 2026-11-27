<!--
  @component
  Organization Insights `/insights` (costs-and-insights-ux §1 Insights, §3, INS-1..5):
  org-wide spend overview (4 KPI tiles, coverage/checkpoint bar, actionable Insights list, spend trend)
  and a sortable, filterable Projects table (investment to date, this month, last month, change,
  working time "— not tracked yet", items done) drilling down to `/projects/[id]/insights` and usage records,
  plus safe CSV export. Reached from Home and from Project › Insights.
-->
<script lang="ts">
	import DownloadIcon from '@lucide/svelte/icons/download';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import StatTile from '$lib/components/StatTile.svelte';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/proto';
	import { costsApi } from '../../api';
	import { buildUsageCsv, downloadCsv } from '../../csv';
	import { formatDayTime } from '../../dates';
	import { formatDuration, WORKING_TIME_NOT_TRACKED } from '../../duration';
	import { formatCount, formatTokens } from '../../money';
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
	import OrgProjectsTable from './OrgProjectsTable.svelte';
	import TrendChart from './TrendChart.svelte';

	let {
		organizationId,
		zone
	}: {
		organizationId: string;
		zone: string;
	} = $props();

	const HEADING = 'org-insights-heading';

	let chosen = $state<Omit<PeriodRequest, 'timeZone'>>({ kind: 'this_month' });
	const period = $derived<PeriodRequest>({ ...chosen, timeZone: zone });

	const insightsRes = new Resource(() =>
		scoped(() => costsApi.organizationInsights(organizationId, period))
	);

	const scopedData = $derived(insightsRes.data);
	const data = $derived(scopedData?.allowed ? scopedData.value : undefined);

	let exporting = $state(false);
	let exportError = $state<string | undefined>();

	let sheet = $state<ReturnType<typeof UsageRecordsSheet>>();
	let opener: HTMLElement | null = null;

	function changePeriod(next: PeriodRequest) {
		chosen = { kind: next.kind, from: next.from, to: next.to };
		exportError = undefined;
		void insightsRes.reload();
	}

	const thisMonthRequest = $derived<PeriodRequest>({ kind: 'this_month', timeZone: zone });

	function openOrgRecords(
		title: string,
		fetchPage: (cursor?: string) => Promise<import('../../types').UsageRecordPage>,
		from: HTMLElement,
		subtitle = `${periodLabel(period.kind)} · effective records, newest first`
	) {
		opener = from;
		sheet?.showCustom(title, subtitle, fetchPage);
	}

	function openProjectRecords(row: OrgInsightProjectRow, from: HTMLElement) {
		openOrgRecords(
			`Usage records: ${row.name}`,
			(cursor) =>
				costsApi.projectRecords(
					organizationId,
					row.projectId,
					thisMonthRequest,
					{ all: true },
					cursor
				),
			from,
			'This month · effective records, newest first'
		);
	}

	function openNotOnTaskRecords(from: HTMLElement) {
		openOrgRecords(
			'Usage records: Not on a task',
			(cursor) =>
				costsApi.records(
					organizationId,
					thisMonthRequest,
					{ dimension: 'project', notOnTask: true },
					cursor
				),
			from,
			'This month · effective records, newest first'
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
</script>

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

				{#if totals.records === 0 && data.projects.length === 0 && data.insights.length === 0}
					<EmptyState
						level={3}
						title="No spend yet"
						description="No usage records were counted for these dates. Usage that hasn't reported yet isn't here, so this is not proof that nothing ran."
						class="py-12"
					/>
					<div class="flex flex-col gap-1 pb-6 text-xs/5 {gutters}">
						{#each data.coverage.gaps as gap (gap.source)}
							<p class="text-fg-muted">{gap.source}: {gap.reason}</p>
						{/each}
						<p class="text-fg-muted">
							Counted up to the ledger checkpoint of {formatDayTime(
								new Date(data.coverage.checkpointAt),
								data.period.timeZone
							)}.
						</p>
					</div>
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

					<OrgProjectsTable
						projects={data.projects}
						notOnTask={data.notOnTask}
						onOpenProjectRecords={openProjectRecords}
						onOpenNotOnTaskRecords={openNotOnTaskRecords}
					/>
				{/if}
			</div>
		</div>

		<UsageRecordsSheet bind:this={sheet} {organizationId} {zone} onCloseAutoFocus={restoreFocus} />
	{/if}
{/if}
