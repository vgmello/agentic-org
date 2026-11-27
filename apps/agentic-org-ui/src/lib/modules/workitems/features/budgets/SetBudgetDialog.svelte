<!--
  @component
  Set or raise a task's lifetime budget (costs-and-insights-ux §4; BUD-1, BUD-2).
  Accepts US dollars (converted exactly to integer micro-USD) or total tokens. Preserves inputs while saving,
  links field refusals with focus, and reconciles unknown outcomes through the same operation ID.
-->
<script lang="ts">
	import { tick } from 'svelte';
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
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		selectClass
	} from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import {
		OUTCOME_UNKNOWN,
		formatBudgetAmount,
		microUsdToInputValue,
		parseDollarsToMicroUsd,
		parseTokenLimit
	} from '../../budget';
	import type { BudgetRights, BudgetUnit, TaskBudget } from '../../types';

	let {
		open = $bindable(false),
		organizationId,
		taskId,
		taskRef,
		current,
		rights,
		onsaved
	}: {
		open: boolean;
		organizationId: string;
		taskId: string;
		taskRef: string;
		current: TaskBudget | null;
		rights: BudgetRights;
		onsaved: (message: string) => void;
	} = $props();

	const uid = $props.id();
	let unit = $state<BudgetUnit>('usd');
	let amountText = $state('');
	let busy = $state(false);
	let checking = $state(false);
	let fieldError = $state<string | undefined>();
	let formError = $state<string | undefined>();
	let unknownOpId = $state<string | undefined>();
	let operationId = $state('');
	let amountInput = $state<HTMLInputElement | null>(null);

	const modeLabel = $derived(current ? 'Change budget' : 'Set budget');
	const canChangeUnit = $derived(!current || rights.raise);

	function resetOnOpen() {
		if (unknownOpId) return; // Keep unresolved attempt until confirmed or cancelled.
		unit = current?.unit ?? 'usd';
		amountText = current
			? current.unit === 'usd'
				? microUsdToInputValue(current.limit)
				: String(current.limit)
			: '';
		fieldError = undefined;
		formError = undefined;
		operationId = newId();
	}

	async function focusAmount() {
		await tick();
		amountInput?.focus();
	}

	function validateBudgetLimit(): { ok: true; limit: number } | { ok: false; error: string } {
		const parsed =
			unit === 'usd' ? parseDollarsToMicroUsd(amountText) : parseTokenLimit(amountText);
		if (!parsed.ok) return parsed;
		const limit = 'microUsd' in parsed ? parsed.microUsd : parsed.tokens;
		// Agent project manager check (BUD-2): can first-set or lower in same unit, never raise or change unit.
		if (current && !rights.raise && (unit !== current.unit || limit > current.limit)) {
			return { ok: false, error: 'Only a person can raise, clear or extend a budget.' };
		}
		return { ok: true, limit };
	}

	async function handleSubmitError(e: unknown, op: string) {
		const err = toAppError(e);
		const raw = e as { code?: string; field?: string };
		if (raw?.code === OUTCOME_UNKNOWN) {
			unknownOpId = op;
			formError =
				'Save not confirmed. The connection dropped before the server answered. Check status before trying again.';
			return;
		}
		operationId = newId();
		if (raw?.field === 'limit') {
			fieldError = err.message;
			await focusAmount();
		} else {
			formError = err.message;
		}
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || checking || unknownOpId) return;
		fieldError = undefined;
		formError = undefined;

		const validated = validateBudgetLimit();
		if (!validated.ok) {
			fieldError = validated.error;
			await focusAmount();
			return;
		}

		busy = true;
		const op = operationId || newId();
		operationId = op;
		try {
			await workItemsApi.setBudget(organizationId, taskId, {
				unit,
				limit: validated.limit,
				expectedPolicyRevision: current?.policyRevision ?? 0,
				operationId: op
			});
			unknownOpId = undefined;
			open = false;
			onsaved(`Budget for ${taskRef} set to ${formatBudgetAmount(unit, validated.limit)}.`);
		} catch (e) {
			await handleSubmitError(e, op);
		} finally {
			busy = false;
		}
	}

	async function checkStatus() {
		if (!unknownOpId || checking) return;
		checking = true;
		try {
			const status = await workItemsApi.budgetOperation(organizationId, unknownOpId);
			if (status.state === 'applied') {
				unknownOpId = undefined;
				open = false;
				onsaved(`Budget for ${taskRef} was saved.`);
			} else if (status.state === 'not_applied') {
				unknownOpId = undefined;
				formError = status.reason;
			} else {
				formError = 'Still waiting for confirmation from the office. Check status again shortly.';
			}
		} catch (e) {
			formError = toAppError(e).message;
		} finally {
			checking = false;
		}
	}

	function cancel() {
		if (busy || checking) return;
		open = false;
	}

	function discardAttempt() {
		if (busy || checking) return;
		unknownOpId = undefined;
		formError = undefined;
		operationId = newId();
		open = false;
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && (busy || checking) && (open = true)}>
	<DialogContent
		onOpenAutoFocus={resetOnOpen}
		class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md"
	>
		<DialogHeader>
			<DialogTitle text="{modeLabel} · {taskRef}" />
			<DialogDescription>
				Set a lifetime budget in US dollars or total tokens. Reaching 80% sends one neutral notice;
				reaching 100% requests a budget hold.
			</DialogDescription>
		</DialogHeader>

		<form class="flex flex-col gap-4" onsubmit={submit} novalidate>
			{#if current && !rights.raise}
				<p
					class="rounded-md bg-surface-raised px-3 py-2 text-xs/5 text-fg-secondary ring-1 ring-line"
					data-testid="agent-pm-budget-note"
				>
					Only a person can raise, clear or extend a budget. You can lower the current limit in the
					same unit.
				</p>
			{/if}

			{#if current?.unmeasured}
				<p
					class="rounded-md bg-surface-raised px-3 py-2 text-xs/5 text-fg-secondary ring-1 ring-line"
				>
					This runtime does not report usage, so this budget cannot stop its unmeasured work.
				</p>
			{/if}

			<div>
				<Label for="{uid}-unit" class="block text-sm/6 font-medium text-fg">Unit</Label>
				<select
					id="{uid}-unit"
					bind:value={unit}
					disabled={busy || checking || !!unknownOpId || !canChangeUnit}
					class="mt-1.5 {selectClass}"
				>
					<option value="usd">US dollars (USD)</option>
					<option value="tokens">Total tokens</option>
				</select>
			</div>

			<div>
				<Label for="{uid}-limit" class="block text-sm/6 font-medium text-fg">
					{unit === 'usd' ? 'Limit (USD)' : 'Limit (tokens)'}
				</Label>
				<Input
					id="{uid}-limit"
					bind:ref={amountInput}
					bind:value={amountText}
					inputmode={unit === 'usd' ? 'decimal' : 'numeric'}
					placeholder={unit === 'usd' ? '10.00' : '2000000'}
					disabled={busy || checking || !!unknownOpId}
					aria-invalid={fieldError ? 'true' : undefined}
					aria-describedby={fieldError ? `${uid}-limit-err` : `${uid}-limit-help`}
					class="mt-1.5 {inputClass}"
				/>
				{#if fieldError}
					<p id="{uid}-limit-err" class={errorClass} role="alert">{fieldError}</p>
				{:else}
					<p id="{uid}-limit-help" class={helpClass}>
						{#if current}
							Current limit: {formatBudgetAmount(current.unit, current.limit)} · Known spend: {formatBudgetAmount(
								current.unit,
								current.spent
							)}.
						{:else if unit === 'usd'}
							Stored as exact integer micro-USD.
						{:else}
							Whole number of tokens across input, output and cache categories.
						{/if}
					</p>
				{/if}
			</div>

			{#if formError}
				<div
					class="rounded-md bg-danger-emphasis/10 px-3 py-2.5 text-sm/6 text-danger-text ring-1 ring-danger-emphasis/20"
					role="alert"
					data-testid="budget-form-error"
				>
					{formError}
				</div>
			{/if}

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy || checking}
					onclick={cancel}>Cancel</Button
				>
				{#if unknownOpId}
					<Button
						type="button"
						variant="outline"
						class={mobileButton}
						disabled={checking}
						onclick={discardAttempt}>Discard attempt</Button
					>
					<Button type="button" class={mobileButton} disabled={checking} onclick={checkStatus}
						>{checking ? 'Checking…' : 'Check status'}</Button
					>
				{:else}
					<Button type="submit" class={mobileButton} disabled={busy} aria-busy={busy}>
						{busy ? 'Saving…' : current ? 'Save budget' : 'Set budget'}
					</Button>
				{/if}
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
