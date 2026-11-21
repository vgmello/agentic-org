<!--
  @component
  Owner-only dialog to set or change an agent's daily or monthly calendar budget (costs-and-insights-ux §4; BUD-1,
  BUD-4). Exact integer micro-USD or whole token limit; preserves inputs on error, focuses invalid fields, and
  reconciles unknown outcomes via `agentBudgetOperation` before repeating.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { newId } from '$lib/modules/organization/commands';
	import { toAppError } from '$lib/proto';
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		selectClass
	} from '$lib/components/classes';
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
	import { costsApi } from '../../api';
	import {
		budgetInputValue,
		formatBudgetAmount,
		parseBudgetTokens,
		parseBudgetUsd
	} from '../../money';
	import { OUTCOME_UNKNOWN } from '../../outcome';
	import { NO_USAGE_RUNTIME } from '../../present';
	import type { BudgetPeriodState, BudgetUnit } from '../../types';

	let {
		organizationId,
		memberId,
		agentName,
		zone,
		onsaved,
		onCloseAutoFocus
	}: {
		organizationId: string;
		memberId: string;
		agentName: string;
		zone: string;
		onsaved: (message: string) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	const uid = $props.id();
	let open = $state(false);
	let periodKind = $state<'daily' | 'monthly'>('daily');
	let current = $state<BudgetPeriodState | undefined>();
	let unit = $state<BudgetUnit>('usd');
	let amountText = $state('');
	let busy = $state(false);
	let checking = $state(false);
	let fieldError = $state<string | undefined>();
	let formError = $state<string | undefined>();
	let unknownOpId = $state<string | undefined>();
	let operationId = $state('');
	let amountInput = $state<HTMLInputElement | null>(null);

	export function start(kind: 'daily' | 'monthly', existing?: BudgetPeriodState) {
		if (!unknownOpId) {
			periodKind = kind;
			current = existing;
			unit = existing?.unit ?? 'usd';
			amountText = existing
				? budgetInputValue(existing.unit, existing.baseLimit ?? existing.limit)
				: '';
			fieldError = undefined;
			formError = undefined;
			operationId = newId();
		}
		open = true;
	}

	const heading = $derived(`${current ? 'Change' : 'Set'} ${periodKind} budget · ${agentName}`);

	async function focusAmount() {
		await tick();
		amountInput?.focus();
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || checking || unknownOpId) return;
		fieldError = undefined;
		formError = undefined;
		const parsed = unit === 'usd' ? parseBudgetUsd(amountText) : parseBudgetTokens(amountText);
		if (!parsed.ok) {
			fieldError = parsed.error;
			await focusAmount();
			return;
		}
		busy = true;
		const op = operationId || newId();
		operationId = op;
		try {
			await costsApi.setAgentBudget(organizationId, memberId, {
				periodKind,
				unit,
				limit: parsed.value,
				operationId: op
			});
			unknownOpId = undefined;
			open = false;
			onsaved(
				`${periodKind === 'daily' ? 'Daily' : 'Monthly'} budget for ${agentName} set to ${formatBudgetAmount(unit, parsed.value)}.`
			);
		} catch (e) {
			const err = toAppError(e);
			const raw = e as { code?: string; field?: string };
			if (err.code === OUTCOME_UNKNOWN || raw?.code === OUTCOME_UNKNOWN) {
				unknownOpId = op;
				formError =
					'Save not confirmed. The connection dropped before HQ answered. Check status before trying again.';
			} else if (raw?.field === 'limit') {
				operationId = newId();
				fieldError = err.message;
				await focusAmount();
			} else {
				formError = err.message;
			}
		} finally {
			busy = false;
		}
	}

	async function checkStatus() {
		if (!unknownOpId || checking) return;
		checking = true;
		try {
			const status = await costsApi.agentBudgetOperation(organizationId, unknownOpId);
			if (status.state === 'applied') {
				unknownOpId = undefined;
				open = false;
				onsaved(`Budget for ${agentName} was saved.`);
			} else if (status.state === 'not_applied') {
				unknownOpId = undefined;
				formError = status.reason;
			} else {
				formError = 'Still waiting for confirmation from HQ. Check status again shortly.';
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
</script>

<Dialog bind:open onOpenChange={(next) => !next && (busy || checking) && (open = true)}>
	<DialogContent class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md" {onCloseAutoFocus}>
		<DialogHeader>
			<DialogTitle text={heading} />
			<DialogDescription>
				Calendar {periodKind} limit in {zone}. Reaching 80% sends one neutral notice; reaching 100%
				requests a budget hold.
			</DialogDescription>
		</DialogHeader>

		<form class="flex flex-col gap-4" onsubmit={submit} novalidate>
			{#if current?.unmeasured}
				<p
					class="rounded-md bg-surface-raised px-3 py-2 text-xs/5 text-fg-secondary ring-1 ring-line"
				>
					{NO_USAGE_RUNTIME}
				</p>
			{/if}

			<div>
				<Label for="{uid}-unit" class="block text-sm/6 font-medium text-fg">Unit</Label>
				<select
					id="{uid}-unit"
					bind:value={unit}
					disabled={busy || checking || !!unknownOpId}
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
					placeholder={unit === 'usd' ? '10.00' : '25000000'}
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
							Current limit: {formatBudgetAmount(current.unit, current.limit)} · Known spend:
							{formatBudgetAmount(current.unit, current.spent)}.
						{:else if unit === 'usd'}
							Stored as exact integer micro-USD.
						{:else}
							Whole number of tokens across input, output, cache read and cache write.
						{/if}
					</p>
				{/if}
			</div>

			{#if formError}
				<div
					class="rounded-md bg-danger-emphasis/10 px-3 py-2.5 text-sm/6 text-danger-text ring-1 ring-danger-emphasis/20"
					role="alert"
					data-testid="agent-budget-error"
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
