<!--
  @component
  Side-column Cost and time block on a work item page (costs-and-insights-ux §2, §4; work-items-ux §9.2).
  Explicitly handles all five Resource states (`loading`, `unavailable`, `unsupported`, `error`, `ready`).
  Enforces cost visibility scope: amounts only when `scope === 'amounts'`, budget state chip only otherwise.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import { Resource } from '$lib/components/resource.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { budgetPercent, formatBudgetAmount, formatMicroUsd, formatTokens } from '../../budget';
	import { formatDuration, timeSplit, when } from '../../present';
	import type { BudgetWord, TaskCost, TaskTime, TaskType } from '../../types';
	import BudgetChip from '../../components/BudgetChip.svelte';
	import SetBudgetDialog from './SetBudgetDialog.svelte';
	import BudgetReachedActions from './BudgetReachedActions.svelte';

	let {
		organizationId,
		taskId,
		taskRef,
		taskType,
		time,
		stale = false,
		ontaskchanged
	}: {
		organizationId: string;
		taskId: string;
		taskRef: string;
		taskType: TaskType;
		time: TaskTime;
		stale?: boolean;
		ontaskchanged: () => void;
	} = $props();

	const costRead = new Resource<TaskCost>(() => workItemsApi.cost(organizationId, taskId));
	let byAgentOpen = $state(false);
	let setBudgetOpen = $state(false);

	const split = $derived(timeSplit(time));

	function budgetWordOf(spent: number, limit: number, hold?: 'stop_pending' | 'held'): BudgetWord {
		if (hold || spent >= limit) return 'reached';
		if (spent * 5 >= limit * 4) return 'warned';
		return 'ok';
	}

	function handleBudgetChanged(message: string) {
		toast.success(message);
		void costRead.reload();
		ontaskchanged();
	}
</script>

<section
	aria-labelledby="cost-time-heading"
	class="flex flex-col gap-4 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="cost-time-block"
>
	<h3 id="cost-time-heading" class="text-sm/6 font-semibold text-fg">Cost and time</h3>

	<!-- Cost & Budget -->
	{#if costRead.state === 'loading'}
		<p class="text-xs/5 text-fg-muted" role="status">Loading cost…</p>
	{:else if costRead.state === 'unavailable'}
		<p class="text-xs/5 text-fg-muted">Cost accounting isn't served on this server yet.</p>
	{:else if costRead.state === 'unsupported'}
		<div data-testid="cost-unsupported">
			<p class="text-xs/5 font-medium text-fg">Not available in this version</p>
			<p class="mt-0.5 text-xs/5 text-fg-muted">
				{costRead.message || "This version of the office doesn't support task cost reads."}
			</p>
		</div>
	{:else if costRead.state === 'error'}
		<div class="flex flex-col gap-2" role="alert">
			<p class="text-xs/5 text-danger-text">Couldn't load costs. {costRead.message}</p>
			<div>
				<Button
					type="button"
					variant="outline"
					size="sm"
					class={mobileButton}
					disabled={costRead.reloading}
					onclick={() => costRead.reload()}
				>
					{costRead.reloading ? 'Retrying…' : 'Try again'}
				</Button>
			</div>
		</div>
	{:else if costRead.data}
		{@const cost = costRead.data}
		{#if cost.scope === 'state'}
			<div class="flex flex-col gap-1.5" data-testid="cost-state-only">
				{#if cost.budget}
					<BudgetChip budget={cost.budget} hold={cost.hold} />
				{:else}
					<p class="text-xs/5 text-fg-muted">No budget set.</p>
				{/if}
			</div>
		{:else}
			{@const a = cost.amounts}
			{@const rights = cost.rights}
			<div class="flex flex-col gap-3" data-testid="cost-amounts">
				{#if a.records === 0}
					<p class="text-xs/5 text-fg-muted">No usage records yet.</p>
				{:else}
					<div class="flex flex-wrap items-baseline justify-between gap-2">
						<div class="flex flex-wrap items-baseline gap-x-2 text-sm/6 text-fg tabular-nums">
							<span class="font-semibold" data-testid="task-cost-value">
								{a.knownCostMicroUsd !== null ? `≈ ${formatMicroUsd(a.knownCostMicroUsd)}` : '—'}
							</span>
							<span class="text-xs/5 text-fg-muted">
								{a.knownTokens !== null ? `${formatTokens(a.knownTokens)} tokens` : '— tokens'}
							</span>
							{#if a.byAgent.length > 0}
								<span class="text-xs/5 text-fg-muted">
									· {a.byAgent.length}
									{a.byAgent.length === 1 ? 'agent' : 'agents'}
								</span>
							{/if}
						</div>
						{#if a.byAgent.length > 0}
							<button
								type="button"
								aria-expanded={byAgentOpen}
								onclick={() => (byAgentOpen = !byAgentOpen)}
								class="rounded-sm text-xs/5 font-medium text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
							>
								By agent {byAgentOpen ? '▴' : '▾'}
							</button>
						{/if}
					</div>

					{#if a.includesApiEquivalent || a.unpricedRecords > 0 || a.incompleteReason}
						<div class="flex flex-col gap-0.5 text-xs/5 text-fg-muted">
							{#if a.includesApiEquivalent}
								<p>≈ includes API-equivalent estimates</p>
							{/if}
							{#if a.unpricedRecords > 0}
								<p>
									Excludes {a.unpricedRecords} unpriced {a.unpricedRecords === 1
										? 'record'
										: 'records'}
								</p>
							{/if}
							{#if a.incompleteReason}
								<p>Usage incomplete: {a.incompleteReason}</p>
							{/if}
						</div>
					{/if}

					{#if byAgentOpen && a.byAgent.length > 0}
						<div class="overflow-x-auto rounded-md border border-line-subtle bg-surface-sunken">
							<table class="w-full text-left text-xs/5" data-testid="by-agent-table">
								<thead>
									<tr class="border-b border-line text-fg-muted">
										<th scope="col" class="px-2.5 py-1.5 font-medium">Agent</th>
										<th scope="col" class="px-2.5 py-1.5 font-medium">Tokens</th>
										<th scope="col" class="px-2.5 py-1.5 font-medium">Cost</th>
										<th scope="col" class="px-2.5 py-1.5 font-medium">Last</th>
									</tr>
								</thead>
								<tbody class="divide-y divide-line-subtle text-fg-body tabular-nums">
									{#each a.byAgent as row (row.agent.id)}
										<tr>
											<td class="px-2.5 py-1.5 text-fg">{row.agent.name}</td>
											<td class="px-2.5 py-1.5"
												>{row.tokens !== null ? formatTokens(row.tokens) : '—'}</td
											>
											<td class="px-2.5 py-1.5"
												>{row.costMicroUsd !== null
													? `≈ ${formatMicroUsd(row.costMicroUsd)}`
													: '—'}</td
											>
											<td class="px-2.5 py-1.5 text-fg-muted">{when(row.lastActivityAt)}</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					{/if}
				{/if}

				<!-- Budget bar & controls -->
				{#if a.budget}
					{@const b = a.budget}
					{@const pct = budgetPercent(b.spent, b.limit)}
					{@const word = budgetWordOf(b.spent, b.limit, b.hold)}
					<div
						class="flex flex-col gap-2 border-t border-line-subtle pt-3"
						data-testid="task-budget"
					>
						<div class="flex flex-wrap items-center justify-between gap-2">
							<span class="text-xs/5 font-medium text-fg">
								Budget {formatBudgetAmount(b.unit, b.limit)}
							</span>
							<BudgetChip budget={word} hold={b.hold} />
						</div>

						<!-- Progress bar with 80% marker -->
						<div
							class="relative h-2 w-full overflow-hidden rounded-full bg-surface-overlay"
							role="progressbar"
							aria-label="Task budget usage"
							aria-valuenow={Math.min(pct, 100)}
							aria-valuemin={0}
							aria-valuemax={100}
						>
							<div
								class="h-full transition-all {word === 'reached'
									? 'bg-warning-emphasis'
									: 'bg-primary'}"
								style:width="{Math.min(pct, 100)}%"
							></div>
							<span
								aria-hidden="true"
								class="absolute top-0 bottom-0 left-[80%] w-px bg-line-strong"
								title="80% notice mark"
							></span>
						</div>

						<p class="text-xs/5 text-fg-muted tabular-nums">
							{formatBudgetAmount(b.unit, b.spent)} ({pct}%)
							{#if !b.complete && b.incompleteReason}
								· {b.incompleteReason}
							{/if}
						</p>

						{#if b.unmeasured}
							<p class="text-xs/5 text-fg-muted">
								This runtime does not report usage, so this budget cannot stop its unmeasured work.
							</p>
						{/if}

						{#if word === 'reached'}
							<BudgetReachedActions
								{organizationId}
								{taskId}
								{taskRef}
								{taskType}
								budget={b}
								{rights}
								onraise={() => (setBudgetOpen = true)}
								onchanged={handleBudgetChanged}
							/>
						{:else if (rights.set || rights.raise) && !stale}
							<div>
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={() => (setBudgetOpen = true)}>Change budget…</Button
								>
							</div>
						{/if}
					</div>
				{:else}
					<div
						class="flex flex-wrap items-center justify-between gap-2 border-t border-line-subtle pt-3"
					>
						<span class="text-xs/5 text-fg-muted">No budget set</span>
						{#if (rights.set || rights.raise) && !stale}
							<Button
								type="button"
								variant="outline"
								size="sm"
								class={mobileButton}
								onclick={() => (setBudgetOpen = true)}>Set budget…</Button
							>
						{/if}
					</div>
				{/if}

				<SetBudgetDialog
					bind:open={setBudgetOpen}
					{organizationId}
					{taskId}
					{taskRef}
					current={a.budget}
					{rights}
					onsaved={handleBudgetChanged}
				/>
			</div>
		{/if}
	{/if}

	<!-- Time split (§9.2, TIME-1, TIME-3, TIME-4) -->
	<div
		class="flex flex-col gap-1.5 border-t border-line-subtle pt-3 text-xs/5"
		data-testid="task-time"
	>
		{#if split}
			<p class="text-fg-secondary tabular-nums">
				<span class="font-medium text-fg">Lead time {split.lead}{split.soFar ? ' so far' : ''}</span
				>
				·
				{#each split.parts as part, i (part.label)}
					{#if i > 0}
						·
					{/if}
					<span title={part.precise}>{part.label} {part.value}</span>
				{/each}
				(working time isn't tracked yet)
			</p>
		{:else}
			<p class="text-fg-muted">Lead time unavailable (earlier history baseline missing).</p>
		{/if}

		{#if time.cycleSeconds !== null}
			<p class="text-fg-muted tabular-nums">
				Cycle time {formatDuration(time.cycleSeconds)}{time.open ? ' so far' : ''}
			</p>
		{:else if time.startUnavailable}
			<p class="text-fg-muted">Cycle time unavailable (start predates recorded history).</p>
		{/if}

		{#if time.typical}
			<p class="text-fg-muted">
				{time.typical.label}: {time.typical.cost} · {time.typical.time}
			</p>
		{/if}
	</div>
</section>
