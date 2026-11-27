<!--
  @component
  Project › Insights tab (costs-and-insights-ux §1, §3, INS-1..5):
  leads with total project investment to date, then period KPI tiles (Cost, Tokens, Working time — not tracked yet,
  Done), completeness/coverage checkpoint bar, actionable Insights list, spend trend chart/table, sortable & filterable
  breakdowns (Team · Consumer · Model · Task type · Work type), release caps, speed medians, and safe CSV export.
-->
<script lang="ts">
	import DownloadIcon from '@lucide/svelte/icons/download';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import StatTile from '$lib/components/StatTile.svelte';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { deriveProjectDeliverySummary } from '../../delivery-summary';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { toAppError } from '$lib/proto';
	import { costsApi } from '../../api';
	import { buildUsageCsv, downloadCsv } from '../../csv';
	import { formatDayTime } from '../../dates';
	import { formatDuration, formatMedianDuration, WORKING_TIME_NOT_TRACKED } from '../../duration';
	import { investmentLeadText, releaseCapText } from '../../insights-helpers';
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
	import type { InsightBreakdownRow, PeriodRequest, ProjectBreakdownKey } from '../../types';
	import PeriodMenu from '../organization-cost/PeriodMenu.svelte';
	import CostsError from '../states/CostsError.svelte';
	import UsageRecordsSheet from '../usage-records/UsageRecordsSheet.svelte';
	import InsightsList from './InsightsList.svelte';
	import ProjectBreakdowns from './ProjectBreakdowns.svelte';
	import ProjectDeliverySection from './ProjectDeliverySection.svelte';
	import ProjectTokenMixSection from './ProjectTokenMixSection.svelte';
	import TrendChart from './TrendChart.svelte';

	let {
		organizationId,
		projectId,
		projectName,
		zone
	}: {
		organizationId: string;
		projectId: string;
		projectName: string;
		zone: string;
	} = $props();

	let chosen = $state<Omit<PeriodRequest, 'timeZone'>>({ kind: 'this_month' });
	const period = $derived<PeriodRequest>({ ...chosen, timeZone: zone });
	let displayMode = $state<'chart' | 'table'>('chart');

	let shellData: ReturnType<typeof useShellData> | undefined;
	try {
		shellData = useShellData();
	} catch {
		shellData = undefined;
	}

	const insightsRes = new Resource(() =>
		scoped(() => costsApi.projectInsights(organizationId, projectId, period))
	);

	const scopedData = $derived(insightsRes.data);
	const data = $derived(scopedData?.allowed ? scopedData.value : undefined);

	const cycleSummary = $derived.by<{ label: string; value: string; note?: string }>(() => {
		const rows = data?.speed ?? [];
		const multiKind = rows.length > 1;
		const eligible = rows
			.filter((r) => r.sampleCount >= 5 && r.medianCycleSeconds !== null)
			.toSorted((a, b) => b.sampleCount - a.sampleCount);
		const chosenRow =
			rows.find((r) => r.kind === 'task' && r.sampleCount >= 5 && r.medianCycleSeconds !== null) ??
			eligible[0];
		if (chosenRow) {
			const formatted = formatMedianDuration(chosenRow.medianCycleSeconds, chosenRow.sampleCount);
			return {
				label: multiKind
					? `Median cycle time (${chosenRow.label.toLowerCase()})`
					: 'Median cycle time',
				value: formatted.value,
				note: formatted.note
			};
		}
		if (rows.some((r) => r.sampleCount >= 5)) {
			return { label: 'Median cycle time', value: '—', note: 'Cycle time not recorded yet' };
		}
		const maxRow = rows.toSorted((a, b) => b.sampleCount - a.sampleCount)[0];
		const formatted = formatMedianDuration(null, maxRow?.sampleCount ?? 0);
		return {
			label:
				multiKind && maxRow && maxRow.sampleCount > 0
					? `Median cycle time (${maxRow.label.toLowerCase()})`
					: 'Median cycle time',
			value: formatted.value,
			note: formatted.note
		};
	});

	const deliveryModel = $derived.by(() => {
		if (!shellData || !shellData.loaded || shellData.loadedOrganizationId !== organizationId) {
			return undefined;
		}
		if (shellData.workError || !shellData.snapshot) return undefined;
		return deriveProjectDeliverySummary({
			snapshot: shellData.snapshot,
			projectId,
			workItems: shellData.workItems,
			inboxItems: shellData.inboxItems,
			ownerId: shellData.ownerId
		});
	});

	let exporting = $state(false);
	let exportError = $state<string | undefined>();
	let sheet = $state<ReturnType<typeof UsageRecordsSheet>>();
	let opener: HTMLElement | null = null;

	function changePeriod(next: PeriodRequest) {
		chosen = { kind: next.kind, from: next.from, to: next.to };
		exportError = undefined;
		void insightsRes.reload();
	}

	function openRecords(key: ProjectBreakdownKey, label: string, from: HTMLElement) {
		opener = from;
		const title = 'all' in key ? `Usage records: ${projectName}` : `Usage records: ${label}`;
		const subtitle = `${periodLabel(period.kind)} · effective records, newest first`;
		sheet?.showCustom(title, subtitle, (cursor) =>
			costsApi.projectRecords(organizationId, projectId, period, key, cursor)
		);
	}

	function openRowRecords(row: InsightBreakdownRow, from: HTMLElement) {
		openRecords(row.key, row.label, from);
	}

	async function exportCsv() {
		if (exporting || !data) return;
		exporting = true;
		exportError = undefined;
		try {
			const res = await costsApi.exportRecords(
				organizationId,
				{ kind: 'project', projectId },
				period
			);
			const csv = buildUsageCsv(res.records, res.coverage);
			const filename = `${data.project.slug}-costs-${period.kind}.csv`;
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
	<NotBuilt title="Insights" place="Insights · {projectName}" area="Projects" />
{:else if insightsRes.state === 'loading'}
	<div role="status" aria-busy="true" class="py-6">
		<span class="sr-only">Loading project insights</span>
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
		<div class="mt-6"><SkeletonRows label="project insights breakdowns" rows={4} /></div>
	</div>
{:else if insightsRes.state === 'unsupported'}
	<div class="py-6 {gutters}" data-testid="resource-unsupported">
		<h3 class="text-sm/6 font-semibold text-fg">Not available in this version</h3>
		<p class="mt-1 max-w-xl text-sm/6 text-fg-muted [overflow-wrap:anywhere]">
			{insightsRes.message || "This version of the office doesn't support project insights."}
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
	<div class="py-6 {gutters}" data-testid="project-insights-not-authorized">
		<EmptyState
			level={3}
			title="You don't see costs here"
			description={scopedData.reason || "Only the owner and this project's manager see its costs."}
		/>
	</div>
{:else if data}
	{@const totals = data.totals}
	{@const incomplete = incompleteText(totals, data.coverage)}
	{@const investIncomplete = incompleteText(data.investment.toDate, data.coverage)}
	<div data-testid="project-insights">
		<div
			class="flex flex-wrap items-start justify-between gap-4 border-b border-line-subtle py-6 {gutters}"
		>
			<div class="min-w-0 flex-1 basis-72">
				<h3 class="text-base/7 font-semibold text-fg">Investment</h3>
				<p
					class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]"
					data-testid="project-investment-lead"
				>
					{investmentLeadText(data)}
				</p>
				{#if investIncomplete}
					<p class="mt-1 text-xs/5 text-fg-muted">{investIncomplete}</p>
				{/if}
				<p class="mt-1 text-xs/5 text-fg-muted">
					<a
						href={resolve('/insights')}
						class="rounded-sm font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
						>Compare across all projects in Organization insights</a
					>
				</p>
			</div>
			<div class="flex flex-wrap items-center gap-3">
				<PeriodMenu {period} busy={insightsRes.reloading} onchange={changePeriod} />
				<div
					class="inline-flex items-center gap-1 rounded-md border border-line bg-surface-raised p-0.5"
					role="group"
					aria-label="Insights display mode"
					data-testid="insights-display-toggle"
				>
					<Button
						type="button"
						variant={displayMode === 'chart' ? 'secondary' : 'ghost'}
						size="sm"
						class={mobileButton}
						aria-pressed={displayMode === 'chart'}
						onclick={() => (displayMode = 'chart')}
					>
						Charts
					</Button>
					<Button
						type="button"
						variant={displayMode === 'table' ? 'secondary' : 'ghost'}
						size="sm"
						class={mobileButton}
						aria-pressed={displayMode === 'table'}
						onclick={() => (displayMode = 'table')}
					>
						Tables
					</Button>
				</div>
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
			</div>
		</div>

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
				{periodLabel(data.period.kind)} · {periodRange(data.period)} · {data.period.timeZone}
			</p>

			{#if totals.records === 0}
				<EmptyState
					level={4}
					title="No spend in this period"
					description="No usage records were counted for this project in these dates. Usage that hasn't reported yet isn't here, so this is not proof that nothing ran."
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
					class="mt-3 grid grid-cols-1 gap-px border-y border-line-subtle bg-surface-raised min-[640px]:grid-cols-2 min-[1280px]:grid-cols-5"
					data-testid="project-kpi-tiles"
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
					<StatTile
						label={cycleSummary.label}
						value={cycleSummary.value}
						detail={cycleSummary.note}
					/>
				</dl>

				<div class="mt-3 flex flex-col gap-1 pb-6 text-xs/5 {gutters}">
					{#if data.work.cancelledRecords > 0}
						<p class="text-fg-secondary" data-testid="spent-on-cancelled">
							Spent on cancelled work (inside total): {formatCost(data.work.cancelledCostMicroUsd)} across
							{formatCount(data.work.cancelledRecords)}
							{data.work.cancelledRecords === 1 ? 'record' : 'records'}.
						</p>
					{/if}
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
						<button
							type="button"
							class="inline-flex min-h-6 items-center rounded-sm font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
							onclick={(e) => openRecords({ all: true }, projectName, e.currentTarget)}
							>Open all {formatCount(totals.records)} records</button
						>
					</p>
				</div>

				<InsightsList insights={data.insights} />

				<TrendChart buckets={data.trend} bind:mode={displayMode} />

				<ProjectTokenMixSection {totals} {displayMode} />

				<ProjectBreakdowns breakdowns={data.breakdowns} total={totals} onopen={openRowRecords} />
			{/if}

			{#if data.speed.length > 0}
				<section
					aria-labelledby="speed-heading"
					class="border-t border-line-subtle py-6 {gutters}"
					data-testid="speed-summary"
				>
					<h3 id="speed-heading" class="text-base/7 font-semibold text-fg">Speed (medians)</h3>
					<p class="mt-1 text-sm/6 text-fg-muted">
						Medians prevent one stuck item from distorting the summary. Working time is {WORKING_TIME_NOT_TRACKED}.
						{#if totals.records === 0}
							· {cycleSummary.label}: {cycleSummary.value}{#if cycleSummary.note}
								({cycleSummary.note}){/if}
						{/if}
					</p>
					<div class="mt-4 overflow-x-auto rounded-lg border border-line-subtle">
						<table class="w-full text-left text-sm/6">
							<caption class="sr-only">Median speed by work item kind</caption>
							<thead>
								<tr class="border-b border-line text-xs/5 font-semibold text-fg-muted">
									<th scope="col" class="px-4 py-2.5">Kind</th>
									<th scope="col" class="px-4 py-2.5 text-right">Median lead time</th>
									<th scope="col" class="px-4 py-2.5 text-right">Median cycle time</th>
									<th scope="col" class="px-4 py-2.5 text-right">Working time</th>
									<th scope="col" class="px-4 py-2.5 text-right">Median waiting on you</th>
								</tr>
							</thead>
							<tbody class="divide-y divide-line-subtle">
								{#each data.speed as s (s.kind)}
									{@const lead = formatMedianDuration(s.medianLeadSeconds, s.sampleCount)}
									{@const cycle = formatMedianDuration(s.medianCycleSeconds, s.sampleCount)}
									{@const wait = formatMedianDuration(s.medianWaitingSeconds, s.sampleCount)}
									<tr>
										<th scope="row" class="px-4 py-2.5 font-medium text-fg">
											{s.label}
											<span class="ml-1.5 text-xs/5 font-normal text-fg-muted">
												({s.sampleCount}
												{s.sampleCount === 1 ? 'item' : 'items'})
											</span>
										</th>
										<td class="px-4 py-2.5 text-right text-fg-secondary tabular-nums"
											>{lead.value}</td
										>
										<td class="px-4 py-2.5 text-right text-fg-secondary tabular-nums"
											>{cycle.value}</td
										>
										<td class="px-4 py-2.5 text-right text-fg-muted">
											— ({WORKING_TIME_NOT_TRACKED})
										</td>
										<td class="px-4 py-2.5 text-right text-fg-secondary tabular-nums"
											>{wait.value}</td
										>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</section>
			{/if}

			{#if data.releaseCaps.length > 0}
				<section
					aria-labelledby="release-caps-heading"
					class="border-t border-line-subtle py-6 {gutters}"
					data-testid="release-caps"
				>
					<h3 id="release-caps-heading" class="text-base/7 font-semibold text-fg">Release caps</h3>
					<p class="mt-1 text-sm/6 text-fg-muted">
						Release caps emit neutral warnings only and never place holds on work.
					</p>
					<ul role="list" class="mt-4 flex flex-col gap-4">
						{#each data.releaseCaps as rel (rel.releaseId)}
							{@const pct =
								rel.capLimit && rel.capLimit > 0
									? Math.floor((rel.spent * 100) / rel.capLimit)
									: undefined}
							<li
								class="rounded-lg border border-line-subtle bg-surface-raised/30 p-4"
								data-release-cap={rel.releaseId}
							>
								<div class="flex flex-wrap items-baseline justify-between gap-2">
									<a
										href={resolve(
											`/projects/${encodeURIComponent(projectId)}/releases/${encodeURIComponent(rel.releaseId)}` as '/'
										)}
										class="rounded-sm text-sm/6 font-semibold text-fg hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
									>
										<span class="font-mono text-xs/5 text-fg-muted">R-{rel.number}</span>
										{rel.title}{#if rel.versionLabel}
											<span class="ml-1.5 text-xs/5 font-normal text-fg-muted">
												{rel.versionLabel}
											</span>
										{/if}
									</a>
									<span class="text-xs/5 text-fg-secondary tabular-nums">
										{releaseCapText(rel)}
									</span>
								</div>
								{#if pct !== undefined}
									<div
										role="meter"
										aria-label="R-{rel.number} {rel.title} cap progress"
										aria-valuemin={0}
										aria-valuemax={100}
										aria-valuenow={Math.min(pct, 100)}
										aria-valuetext="{releaseCapText(rel)} ({pct}%)"
										class="relative mt-2 h-2 overflow-hidden rounded-full bg-surface-overlay"
									>
										<span
											class="absolute inset-y-0 left-0 rounded-full bg-brand"
											style:width="{Math.min(pct, 100)}%"
										></span>
										<span aria-hidden="true" class="absolute inset-y-0 left-4/5 w-px bg-fg-muted"
										></span>
									</div>
								{/if}
								{#if !rel.complete}
									<p class="mt-1.5 text-xs/5 text-fg-muted">
										Usage incomplete; spent shown is a lower bound.
									</p>
								{/if}
							</li>
						{/each}
					</ul>
				</section>
			{/if}

			{#if deliveryModel}
				<ProjectDeliverySection {projectId} {projectName} {deliveryModel} {displayMode} />
			{/if}
		</div>
	</div>

	<UsageRecordsSheet bind:this={sheet} {organizationId} {zone} onCloseAutoFocus={restoreFocus} />
{/if}
