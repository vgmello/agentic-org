<!--
  @component
  Actions when a task's budget is reached (costs-and-insights-ux §2, §4; BUD-2, BUD-3):
  Raise budget…, Allow 20% more once (base reach only, once), Clear budget, and Stop (confirm names transition).
  Agent managers see "Only a person can raise, clear or extend a budget." and keep Stop when authorized.
-->
<script lang="ts">
	import { newId } from '$lib/modules/organization/commands';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { dangerButton, mobileButton, textareaClass } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { OUTCOME_UNKNOWN, canAllowOnce, formatBudgetAmount } from '../../budget';
	import type { BudgetRights, TaskBudget, TaskType } from '../../types';

	let {
		organizationId,
		taskId,
		taskRef,
		taskType,
		budget,
		rights,
		stale = false,
		onraise,
		onchanged
	}: {
		organizationId: string;
		taskId: string;
		taskRef: string;
		taskType: TaskType;
		budget: TaskBudget;
		rights: BudgetRights;
		stale?: boolean;
		onraise: () => void;
		onchanged: (message: string) => void;
	} = $props();

	const uid = $props.id();
	let busy = $state<'allow_once' | 'clear' | 'stop' | null>(null);
	let error = $state<string | undefined>();
	let unknownOp = $state<{ id: string; label: string } | undefined>();
	let checking = $state(false);
	let clearOpen = $state(false);
	let stopOpen = $state(false);
	let stopReason = $state('');

	const allowCheck = $derived(canAllowOnce(budget));
	const stopTransitionName = $derived(
		taskType === 'bug' ? `Close ${taskRef} as Won't fix` : `Cancel ${taskRef}`
	);

	async function runAllowOnce() {
		if (stale || busy || checking) return;
		error = undefined;
		if (!allowCheck.ok) {
			error = allowCheck.reason;
			return;
		}
		const op = newId();
		busy = 'allow_once';
		try {
			await workItemsApi.budgetAction(organizationId, taskId, {
				action: 'allow_once',
				operationId: op
			});
			onchanged(
				`Allowed 20% more once on ${taskRef} (new limit ${formatBudgetAmount(budget.unit, allowCheck.nextLimit)}).`
			);
		} catch (e) {
			const raw = e as { code?: string };
			if (raw?.code === OUTCOME_UNKNOWN) {
				unknownOp = { id: op, label: 'Allow 20% more once' };
				error = 'Action not confirmed. Check status before trying again.';
			} else {
				error = toAppError(e).message;
			}
		} finally {
			busy = null;
		}
	}

	async function runClear() {
		if (stale) return;
		const op = newId();
		busy = 'clear';
		error = undefined;
		try {
			await workItemsApi.budgetAction(organizationId, taskId, {
				action: 'clear',
				operationId: op
			});
			onchanged(`Cleared budget on ${taskRef}.`);
		} catch (e) {
			const raw = e as { code?: string };
			if (raw?.code === OUTCOME_UNKNOWN) {
				unknownOp = { id: op, label: 'Clear budget' };
				error = 'Clear budget not confirmed. Check status before trying again.';
			} else {
				throw e;
			}
		} finally {
			busy = null;
		}
	}

	async function runStop(event: SubmitEvent) {
		event.preventDefault();
		if (stale || busy || checking) return;
		const op = newId();
		busy = 'stop';
		error = undefined;
		try {
			await workItemsApi.budgetAction(organizationId, taskId, {
				action: 'stop',
				reason: stopReason.trim() || undefined,
				operationId: op
			});
			stopOpen = false;
			stopReason = '';
			onchanged(`${stopTransitionName} requested.`);
		} catch (e) {
			const raw = e as { code?: string };
			if (raw?.code === OUTCOME_UNKNOWN) {
				stopOpen = false;
				unknownOp = { id: op, label: stopTransitionName };
				error = 'Stop not confirmed. Check status before trying again.';
			} else {
				error = toAppError(e).message;
			}
		} finally {
			busy = null;
		}
	}

	async function checkUnknown() {
		if (!unknownOp || checking) return;
		checking = true;
		try {
			const res = await workItemsApi.budgetOperation(organizationId, unknownOp.id);
			if (res.state === 'applied') {
				const label = unknownOp.label;
				unknownOp = undefined;
				error = undefined;
				onchanged(`${label} was applied.`);
			} else if (res.state === 'not_applied') {
				unknownOp = undefined;
				error = res.reason;
			} else {
				error = 'Still waiting for confirmation from the office. Check status again shortly.';
			}
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			checking = false;
		}
	}
</script>

<div
	class="flex flex-col gap-3 rounded-lg bg-warning/10 p-3 ring-1 ring-warning/30"
	data-testid="budget-reached-box"
>
	<div class="flex flex-col gap-1">
		<p class="text-xs/5 font-semibold text-warning-text-strong" data-testid="budget-hold-label">
			{budget.hold === 'stop_pending' ? 'Stopping · Budget reached' : 'Paused · Budget reached'}
		</p>
		<p class="text-xs/5 text-fg-secondary">
			{budget.hold === 'stop_pending'
				? 'No new turns are admitted. Waiting for the current turn to finish before the hold is verified.'
				: 'Execution is held until the budget is raised, extended, cleared or stopped.'}
		</p>
		{#if budget.extended}
			<p class="text-xs/5 text-fg-muted" data-testid="budget-already-extended">
				This budget already used its one 20% extension.
			</p>
		{/if}
		{#if !rights.raise}
			<p class="text-xs/5 text-fg-secondary" data-testid="budget-person-only">
				Only a person can raise, clear or extend a budget.
			</p>
		{/if}
	</div>

	{#if error}
		<p class="text-xs/5 text-danger-text" role="alert" data-testid="budget-action-error">
			{error}
		</p>
	{/if}

	{#if stale}
		<p class="text-xs/5 text-fg-muted" data-testid="budget-actions-stale">
			Budget actions are paused while viewing an offline copy.
		</p>
	{:else}
		<div class="flex flex-wrap gap-2">
			{#if unknownOp}
				<Button
					type="button"
					size="sm"
					class={mobileButton}
					disabled={checking}
					onclick={checkUnknown}>{checking ? 'Checking…' : 'Check status'}</Button
				>
			{:else}
				{#if rights.raise}
					<Button
						type="button"
						size="sm"
						class={mobileButton}
						disabled={busy !== null}
						onclick={onraise}>Raise budget…</Button
					>
					{#if !budget.extended}
						<Button
							type="button"
							variant="outline"
							size="sm"
							class={mobileButton}
							disabled={busy !== null || !allowCheck.ok}
							title={!allowCheck.ok ? allowCheck.reason : undefined}
							onclick={runAllowOnce}
						>
							{busy === 'allow_once' ? 'Working…' : 'Allow 20% more once'}
						</Button>
					{/if}
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						disabled={busy !== null}
						onclick={() => (clearOpen = true)}
					>
						{busy === 'clear' ? 'Working…' : 'Clear budget'}
					</Button>
				{/if}
				{#if rights.stop}
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						disabled={busy !== null}
						onclick={() => ((error = undefined), (stopOpen = true))}
					>
						{busy === 'stop' ? 'Working…' : 'Stop'}
					</Button>
				{/if}
			{/if}
		</div>
	{/if}
</div>

<ConfirmDialog
	bind:open={clearOpen}
	title="Clear budget on {taskRef}?"
	description="This records a new policy revision with no budget limit and releases the budget hold. Spend history is kept."
	confirmLabel="Clear budget"
	busyLabel="Clearing…"
	onconfirm={runClear}
/>

<Dialog bind:open={stopOpen} onOpenChange={(next) => !next && busy === 'stop' && (stopOpen = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		<DialogHeader>
			<DialogTitle text="{stopTransitionName}?" />
			<DialogDescription>
				Stopping requests {stopTransitionName} through the task transition service. The recorded spend
				and history stay preserved.
			</DialogDescription>
		</DialogHeader>
		<form class="flex flex-col gap-4" onsubmit={runStop}>
			<div>
				<Label for="{uid}-stop-reason" class="block text-sm/6 font-medium text-fg">
					Reason (optional)
				</Label>
				<textarea
					id="{uid}-stop-reason"
					rows={3}
					bind:value={stopReason}
					disabled={busy === 'stop'}
					placeholder="Why this work is stopping…"
					class="mt-1.5 {textareaClass}"></textarea>
			</div>
			{#if error}
				<p class="text-xs/5 text-danger-text" role="alert">{error}</p>
			{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy === 'stop'}
					onclick={() => (stopOpen = false)}>Cancel</Button
				>
				<Button
					type="submit"
					class="{mobileButton} {dangerButton}"
					disabled={busy === 'stop'}
					aria-busy={busy === 'stop'}
				>
					{busy === 'stop' ? 'Stopping…' : stopTransitionName}
				</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
