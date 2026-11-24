<!--
  @component
  Add a rate (costs-and-insights-ux §5): a new price version for a provider and model, from today or a later date.
  Used versions never change, so editing a rate is adding a new one; started from a model's row, the form is prefilled
  with its current rates. The values stay while Saving…; a refusal names the fix beside its field and focuses it (the
  date for date refusals). A verified failure can be tried again with the same operation ID. When the outcome is
  unknown, the dialog says "Save not confirmed" and offers only Check status until the rate is known to be added or not:
  it never sends the rate a second time blind. Closing does not abandon it: reopening the dialog, from any button,
  returns to that save until Check status settles it, and `onunconfirmed` tells the page so it can say so and
  read the prices again.
  Open it with `start(prefill?)`; `onadded` gets the version HQ recorded.
-->
<script lang="ts">
	import { tick } from 'svelte';
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
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
	import { toAppError } from '$lib/proto';
	import { costsApi } from '../../api';
	import { OUTCOME_UNKNOWN } from '../../outcome';
	import { rateInput } from '../../money';
	import { todayKey } from '../../present';
	import type { PriceField, PriceVersion } from '../../types';
	import {
		checkRateForm,
		fieldOf,
		firstError,
		type RateErrors,
		type RateForm,
		type RateFormField
	} from './rate-form';

	let {
		organizationId,
		zone,
		providers = [],
		models = [],
		onadded,
		onunconfirmed,
		onCloseAutoFocus
	}: {
		organizationId: string;
		/** The viewer's zone: the effective date starts at the beginning of that day there. */
		zone: string;
		/** Names already in use, offered as suggestions. */
		providers?: string[];
		models?: string[];
		onadded: (price: PriceVersion) => void;
		/** A save's outcome became unknown (`true`), or was settled (`false`); `closed` when the dialog closed over it. */
		onunconfirmed?: (state: { unconfirmed: boolean; closed?: boolean }) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	const id = $props.id();
	let open = $state(false);
	let form = $state<RateForm>(blank());
	let errors = $state<RateErrors>({});
	let busy = $state<'adding' | 'checking' | undefined>();
	/** What is known about the last attempt that did not succeed. */
	let outcome = $state<
		| { kind: 'unknown' }
		| { kind: 'pending' }
		| { kind: 'not_applied'; reason: string }
		| { kind: 'failed'; message: string }
		| undefined
	>();
	let prefilledFrom = $state<string | undefined>();
	let checkError = $state<string | undefined>();
	let operationId = '';
	const inputs = $state<Record<RateFormField, HTMLInputElement | null>>({
		provider: null,
		model: null,
		input: null,
		output: null,
		cacheRead: null,
		cacheWrite: null,
		effectiveFrom: null
	});

	const today = $derived(todayKey(zone));
	/** Until the outcome is known, the rate is not sent again. */
	const unresolved = $derived(outcome?.kind === 'unknown' || outcome?.kind === 'pending');

	function blank(): RateForm {
		return {
			provider: '',
			model: '',
			input: '',
			output: '',
			cacheRead: '',
			cacheWrite: '',
			effectiveFrom: ''
		};
	}

	export function start(prefill?: PriceVersion) {
		// An unconfirmed save is never replaced by a new operation: reopen on it until it is settled.
		if (unresolved) {
			open = true;
			return;
		}
		form = prefill
			? {
					provider: prefill.provider,
					model: prefill.model,
					input: rateInput(prefill.inputMicroUsdPerMillion),
					output: rateInput(prefill.outputMicroUsdPerMillion),
					cacheRead: rateInput(prefill.cacheReadMicroUsdPerMillion),
					cacheWrite: rateInput(prefill.cacheWriteMicroUsdPerMillion),
					effectiveFrom: todayKey(zone)
				}
			: { ...blank(), effectiveFrom: todayKey(zone) };
		prefilledFrom = prefill ? prefill.model : undefined;
		errors = {};
		outcome = undefined;
		busy = undefined;
		operationId = crypto.randomUUID();
		open = true;
	}

	async function focusField(field: RateFormField | undefined) {
		await tick();
		if (field) inputs[field]?.focus();
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || unresolved) return;
		const checked = checkRateForm(form, today);
		if (!checked.ok) {
			errors = checked.errors;
			await focusField(firstError(errors));
			return;
		}
		errors = {};
		outcome = undefined;
		busy = 'adding';
		try {
			const price = await costsApi.addPrice(organizationId, {
				...checked.values,
				timeZone: zone,
				operationId
			});
			busy = undefined;
			open = false;
			onadded(price);
		} catch (e) {
			busy = undefined;
			const failure = toAppError(e) as ReturnType<typeof toAppError> & { field?: PriceField };
			const field = fieldOf((e as { field?: PriceField })?.field);
			if (failure.code === OUTCOME_UNKNOWN) {
				outcome = { kind: 'unknown' };
				onunconfirmed?.({ unconfirmed: true });
			} else if (field) {
				// A refusal about one field: it is fixed there, and a fresh attempt gets a new operation.
				errors = { [field]: failure.message };
				operationId = crypto.randomUUID();
				await focusField(field);
			} else {
				// Verified failure: nothing was added; trying again reuses this operation's ID.
				outcome = { kind: 'failed', message: failure.message };
			}
		}
	}

	async function checkStatus() {
		if (busy) return;
		busy = 'checking';
		try {
			const result = await costsApi.priceOperation(organizationId, operationId);
			busy = undefined;
			if (result.state !== 'pending') onunconfirmed?.({ unconfirmed: false });
			if (result.state === 'applied') {
				open = false;
				onadded(result.price);
			} else if (result.state === 'not_applied') {
				outcome = { kind: 'not_applied', reason: result.reason };
			} else {
				outcome = { kind: 'pending' };
			}
		} catch (e) {
			busy = undefined;
			outcome = { kind: 'unknown' };
			errors = {};
			checkError = toAppError(e).message;
			return;
		}
		checkError = undefined;
	}
	function cancel() {
		open = false;
		openChanged(false);
	}

	function openChanged(next: boolean) {
		if (next) return;
		if (busy) {
			open = true;
			return;
		}
		if (unresolved) onunconfirmed?.({ unconfirmed: true, closed: true });
	}

	const rateFields: { field: 'input' | 'output' | 'cacheRead' | 'cacheWrite'; label: string }[] = [
		{ field: 'input', label: 'Input' },
		{ field: 'output', label: 'Output' },
		{ field: 'cacheRead', label: 'Cache read' },
		{ field: 'cacheWrite', label: 'Cache write' }
	];
	const describedBy = (field: RateFormField, help?: string) =>
		errors[field] ? `${id}-${field}-error` : help;
</script>

{#snippet fieldError(field: RateFormField)}
	{#if errors[field]}<p id="{id}-{field}-error" class={errorClass}>{errors[field]}</p>{/if}
{/snippet}

<Dialog bind:open onOpenChange={openChanged}>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-xl">
		<form class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-6" onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle text={prefilledFrom ? `New rate for ${prefilledFrom}` : 'Add a rate'} />
				<DialogDescription class="[overflow-wrap:anywhere]">
					Rates are US dollars per million tokens at API list prices. A rate in use never changes: a
					new rate applies from its date on and never re-estimates spend already recorded.
				</DialogDescription>
			</DialogHeader>

			<!-- While a save is in flight or its outcome unknown, the values are kept and can't be changed. -->
			<fieldset
				disabled={!!busy || unresolved}
				class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-6"
			>
				<div class="grid grid-cols-[minmax(0,1fr)] gap-4 min-[640px]:grid-cols-2">
					<div>
						<Label for="{id}-provider" class="block text-sm/6 font-medium text-fg">Provider</Label>
						<Input
							bind:ref={inputs.provider}
							id="{id}-provider"
							list="{id}-providers"
							autocomplete="off"
							bind:value={form.provider}
							readonly={!!prefilledFrom}
							aria-invalid={errors.provider ? true : undefined}
							aria-describedby={describedBy('provider')}
							class="mt-2 w-full min-w-0 {inputClass}"
						/>
						<datalist id="{id}-providers">
							{#each providers as name (name)}<option value={name}></option>{/each}
						</datalist>
						{@render fieldError('provider')}
					</div>
					<div>
						<Label for="{id}-model" class="block text-sm/6 font-medium text-fg">Model</Label>
						<Input
							bind:ref={inputs.model}
							id="{id}-model"
							list="{id}-models"
							autocomplete="off"
							bind:value={form.model}
							readonly={!!prefilledFrom}
							aria-invalid={errors.model ? true : undefined}
							aria-describedby={describedBy('model')}
							class="mt-2 w-full min-w-0 {inputClass}"
						/>
						<datalist id="{id}-models">
							{#each models as name (name)}<option value={name}></option>{/each}
						</datalist>
						{@render fieldError('model')}
					</div>
				</div>

				<fieldset class="min-w-0">
					<legend class="text-sm/6 font-medium text-fg">Rates ($ per million tokens)</legend>
					<p id="{id}-rates-help" class="mt-1 text-xs/5 text-fg-muted">
						Leave a rate blank when it isn't known: records with those tokens are then unpriced.
						Enter 0 only for tokens that cost nothing.
					</p>
					<div class="mt-3 grid grid-cols-[minmax(0,1fr)] gap-4 min-[480px]:grid-cols-2">
						{#each rateFields as { field, label } (field)}
							<div>
								<Label for="{id}-{field}" class="block text-sm/6 font-medium text-fg">{label}</Label
								>
								<Input
									bind:ref={inputs[field]}
									id="{id}-{field}"
									inputmode="decimal"
									autocomplete="off"
									placeholder="—"
									bind:value={form[field]}
									aria-invalid={errors[field] ? true : undefined}
									aria-describedby={describedBy(field, `${id}-rates-help`)}
									class="mt-2 w-full min-w-0 {inputClass}"
								/>
								{@render fieldError(field)}
							</div>
						{/each}
					</div>
				</fieldset>

				<div class="min-w-0 min-[480px]:max-w-56">
					<Label for="{id}-effectiveFrom" class="block text-sm/6 font-medium text-fg"
						>Takes effect on</Label
					>
					<Input
						bind:ref={inputs.effectiveFrom}
						id="{id}-effectiveFrom"
						type="date"
						min={today}
						bind:value={form.effectiveFrom}
						aria-invalid={errors.effectiveFrom ? true : undefined}
						aria-describedby={describedBy('effectiveFrom', `${id}-effectiveFrom-help`)}
						class="mt-2 w-full min-w-0 {inputClass}"
					/>
					{#if errors.effectiveFrom}
						{@render fieldError('effectiveFrom')}
					{:else}
						<p id="{id}-effectiveFrom-help" class={helpClass}>
							From the start of this day in {zone}. Today or later.
						</p>
					{/if}
				</div>
			</fieldset>
			{#if outcome}
				<div
					role="alert"
					class="rounded-md border border-line bg-tint/[0.03] p-3 text-sm/6"
					data-testid="add-rate-outcome"
				>
					{#if outcome.kind === 'unknown'}
						<p class="font-semibold text-fg">Save not confirmed</p>
						<p class="mt-1 text-fg-secondary">
							HQ didn't confirm whether the rate was added. Check its status before trying again, so
							the rate isn't added twice.{checkError ? ` ${checkError}` : ''}
						</p>
					{:else if outcome.kind === 'pending'}
						<p class="font-semibold text-fg">Still not confirmed</p>
						<p class="mt-1 text-fg-secondary">
							HQ hasn't finished with this rate yet. Check again in a moment.
						</p>
					{:else if outcome.kind === 'not_applied'}
						<p class="font-semibold text-fg">The rate wasn't added</p>
						<p class="mt-1 text-fg-secondary">
							{outcome.reason} Your values are kept; add it again when ready.
						</p>
					{:else}
						<p class="font-semibold text-danger-text">Couldn't add the rate</p>
						<p class="mt-1 text-fg-secondary">
							{outcome.message} Your values are kept; try again.
						</p>
					{/if}
				</div>
			{/if}

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={!!busy}
					onclick={cancel}>Cancel</Button
				>
				{#if unresolved}
					<Button
						type="button"
						class="min-[640px]:min-w-32 {mobileButton}"
						aria-busy={busy === 'checking'}
						aria-disabled={busy === 'checking'}
						onclick={checkStatus}>{busy === 'checking' ? 'Checking…' : 'Check status'}</Button
					>
				{:else}
					<Button
						type="submit"
						class="min-[640px]:min-w-28 {mobileButton}"
						aria-busy={busy === 'adding'}
						aria-disabled={busy === 'adding'}>{busy === 'adding' ? 'Saving…' : 'Add rate'}</Button
					>
				{/if}
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
