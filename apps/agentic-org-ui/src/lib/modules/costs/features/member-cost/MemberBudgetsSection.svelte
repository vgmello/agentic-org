<!--
  @component
  Owner-only daily and monthly calendar budgets section for MemberCostView (COST-7).
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { newId } from '$lib/modules/organization/commands';
	import { toAppError } from '$lib/proto';
	import { costsApi } from '../../api';
	import { OUTCOME_UNKNOWN } from '../../outcome';
	import { budgetState, zoneSourceText } from '../../present';
	import type { BudgetPeriodState, MemberCostOverview } from '../../types';
	import BudgetBar from '../agent-budgets/BudgetBar.svelte';

	let {
		organizationId,
		memberId,
		agentName,
		budgets,
		onOpenDialog,
		onReload
	}: {
		organizationId: string;
		memberId: string;
		agentName: string;
		budgets: NonNullable<MemberCostOverview['budgets']>;
		onOpenDialog: (
			kind: 'daily' | 'monthly',
			existing: BudgetPeriodState | undefined,
			from: HTMLElement
		) => void;
		onReload: () => Promise<void>;
	} = $props();

	let actionBusy = $state<string | undefined>();
	let actionError = $state<string | undefined>();
	let unknownAction = $state<
		| {
				periodKind: 'daily' | 'monthly';
				action: 'clear' | 'allow_once' | 'keep_paused';
				operationId: string;
		  }
		| undefined
	>();
	let checkingAction = $state(false);

	const actionsLocked = $derived(!!actionBusy || checkingAction || !!unknownAction);

	const isReached = (b: BudgetPeriodState) =>
		b.hold === 'stop_pending' || b.hold === 'held' || b.spent >= b.limit;

	function triggerDialog(
		kind: 'daily' | 'monthly',
		existing: BudgetPeriodState | undefined,
		from: HTMLElement
	) {
		if (actionsLocked) return;
		if (!unknownAction) actionError = undefined;
		onOpenDialog(kind, existing, from);
	}

	async function runBudgetAction(
		periodKind: 'daily' | 'monthly',
		action: 'clear' | 'allow_once' | 'keep_paused'
	) {
		if (actionBusy || checkingAction || unknownAction) return;
		const key = `${periodKind}:${action}`;
		const op = newId();
		actionBusy = key;
		actionError = undefined;
		try {
			const res = await costsApi.agentBudgetAction(organizationId, memberId, {
				periodKind,
				action,
				operationId: op
			});
			toast.success(res.message);
			await onReload();
		} catch (e) {
			const err = toAppError(e);
			const raw = e as { code?: string };
			if (err.code === OUTCOME_UNKNOWN || raw?.code === OUTCOME_UNKNOWN) {
				unknownAction = { periodKind, action, operationId: op };
				actionError =
					'Change not confirmed. The connection dropped before HQ answered. Check status before trying again.';
			} else {
				actionError = err.message;
			}
		} finally {
			actionBusy = undefined;
		}
	}

	async function checkActionStatus() {
		if (!unknownAction || checkingAction) return;
		checkingAction = true;
		try {
			const status = await costsApi.agentBudgetOperation(organizationId, unknownAction.operationId);
			if (status.state === 'applied') {
				unknownAction = undefined;
				actionError = undefined;
				toast.success('Budget change was confirmed.');
				await onReload();
			} else if (status.state === 'not_applied') {
				unknownAction = undefined;
				actionError = status.reason;
			} else {
				actionError = 'Still waiting for confirmation from HQ. Check status again shortly.';
			}
		} catch (e) {
			actionError = toAppError(e).message;
		} finally {
			checkingAction = false;
		}
	}
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
						disabled={actionsLocked}
						onclick={(e) => triggerDialog(kind, budget, e.currentTarget)}
						>{reached ? 'Raise budget…' : 'Change budget…'}</Button
					>
					{#if reached && !budget.extended}
						<Button
							type="button"
							variant="outline"
							size="sm"
							class={mobileButton}
							disabled={actionsLocked}
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
							disabled={actionsLocked}
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
						disabled={actionsLocked}
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
						disabled={actionsLocked}
						onclick={(e) => triggerDialog(kind, undefined, e.currentTarget)}
						>Set {kind} budget…</Button
					>
				{/if}
			</div>
		{/if}
	</div>
{/snippet}

<section
	aria-labelledby="member-budgets-heading"
	class="border-t border-line-subtle py-6 {gutters}"
	data-testid="member-budgets-section"
>
	<div class="flex flex-wrap items-start justify-between gap-4">
		<div>
			<h3 id="member-budgets-heading" class="text-base/7 font-semibold text-fg">Budgets</h3>
			<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">
				{#if budgets.zone.name}
					Daily and monthly limits run in {budgets.zone.name}, {zoneSourceText(
						budgets.zone.source
					)}, so "today" here can differ from your viewer zone.
				{:else}
					Daily and monthly limits for {agentName}.
				{/if}
			</p>
		</div>
	</div>

	{#if budgets.resetNote}
		<p
			class="mt-3 rounded-md border border-line bg-surface-raised px-3 py-2 text-xs/5 text-fg-secondary"
			data-testid="budget-reset-note"
		>
			{budgets.resetNote}
		</p>
	{/if}

	{#if actionError || unknownAction}
		<div
			class="mt-3 flex flex-wrap items-center gap-3 rounded-md bg-danger-emphasis/10 px-3 py-2 text-xs/5 text-danger-text ring-1 ring-danger-emphasis/20"
			role="alert"
			data-testid="budget-action-outcome"
		>
			<p class="min-w-0 flex-1">
				{actionError ||
					'Change not confirmed. The connection dropped before HQ answered. Check status before trying again.'}
			</p>
			{#if unknownAction}
				<Button
					type="button"
					variant="outline"
					size="sm"
					class={mobileButton}
					disabled={checkingAction}
					onclick={checkActionStatus}>{checkingAction ? 'Checking…' : 'Check status'}</Button
				>
			{/if}
		</div>
	{/if}

	{#if !budgets.zone.name}
		<p class="mt-4 text-sm/6 text-fg-secondary" data-testid="budget-zone-unknown">
			Agent budgets can't run yet: {zoneSourceText(budgets.zone.source)} isn't known, and budget days
			and months are never counted in UTC by default.
		</p>
	{:else if !budgets.daily && !budgets.monthly}
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
			{#if budgets.canManage}
				<div class="flex flex-wrap gap-2">
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						onclick={(e) => triggerDialog('daily', undefined, e.currentTarget)}>Set budget…</Button
					>
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						onclick={(e) => triggerDialog('monthly', undefined, e.currentTarget)}
						>Set monthly budget…</Button
					>
				</div>
			{/if}
		</div>
	{:else}
		<div class="mt-4 grid grid-cols-1 gap-4 min-[1024px]:grid-cols-2">
			{@render budgetCard('daily', budgets.daily, budgets.zone.name, budgets.canManage)}
			{@render budgetCard('monthly', budgets.monthly, budgets.zone.name, budgets.canManage)}
		</div>
	{/if}
</section>
