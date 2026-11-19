<!--
  @component
  The cost period (costs-and-insights-ux §1, §3): a named button opening a radio menu of This week, This month, Last
  month, Custom and All time. Custom asks for inclusive whole dates right below, From and optionally To (up to now),
  checked when Show is pressed; a refused field says why and takes focus. The period in effect changes only when a
  choice is complete, so the figures never describe a half-entered range.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuLabel,
		DropdownMenuRadioGroup,
		DropdownMenuRadioItem,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		PERIODS,
		checkCustomRange,
		periodLabel,
		todayKey,
		type CustomErrors
	} from '../../present';
	import type { PeriodKind, PeriodRequest } from '../../types';

	let {
		period,
		busy = false,
		onchange
	}: {
		period: PeriodRequest;
		/** The figures for the new period are loading. */
		busy?: boolean;
		onchange: (next: PeriodRequest) => void;
	} = $props();

	const id = $props.id();
	/** Custom was chosen in the menu and the dates are being entered. */
	let editing = $state(false);
	let from = $state('');
	let to = $state('');
	let errors = $state<CustomErrors>({});
	let fromInput = $state<HTMLInputElement | null>(null);
	let toInput = $state<HTMLInputElement | null>(null);
	let trigger = $state<HTMLElement | null>(null);

	/** Closing the form, by Show or Cancel, returns focus to the menu that opened it (app-shell §6a). */
	async function backToTrigger() {
		editing = false;
		await tick();
		trigger?.focus();
	}

	const shown = $derived<PeriodKind>(editing ? 'custom' : period.kind);
	const today = $derived(todayKey(period.timeZone));

	async function choose(value: string) {
		const kind = value as PeriodKind;
		if (kind === 'custom') {
			from = period.kind === 'custom' ? (period.from ?? '') : '';
			to = period.kind === 'custom' ? (period.to ?? '') : '';
			errors = {};
			editing = true;
			// The menu returns focus to its trigger as it closes; the first date takes it after that.
			setTimeout(() => fromInput?.focus(), 0);
			return;
		}
		editing = false;
		if (kind !== period.kind) onchange({ kind, timeZone: period.timeZone });
	}

	async function apply(event: SubmitEvent) {
		event.preventDefault();
		errors = checkCustomRange(from, to, today);
		if (errors.from || errors.to) {
			await tick();
			(errors.from ? fromInput : toInput)?.focus();
			return;
		}
		onchange({ kind: 'custom', from, ...(to ? { to } : {}), timeZone: period.timeZone });
		await backToTrigger();
	}

	function cancel() {
		errors = {};
		void backToTrigger();
	}
</script>

<div class="flex flex-col gap-3">
	<DropdownMenu>
		<DropdownMenuTrigger>
			{#snippet child({ props })}
				<Button
					{...props}
					bind:ref={trigger}
					variant="outline"
					class="self-start {mobileButton}"
					aria-label="Period: {periodLabel(shown)}"
					aria-busy={busy}
					data-testid="period-menu"
				>
					{periodLabel(shown)}
					<MiniIcon name="chevronDown" class="size-5 text-fg-muted" />
				</Button>
			{/snippet}
		</DropdownMenuTrigger>
		<DropdownMenuContent preventScroll={false} align="start" class="w-52 max-w-[calc(100vw-2rem)]">
			<DropdownMenuLabel>Period</DropdownMenuLabel>
			<DropdownMenuRadioGroup value={shown} onValueChange={choose}>
				{#each PERIODS as option (option.kind)}
					<DropdownMenuRadioItem
						value={option.kind}
						closeOnSelect
						class="min-h-9 max-[639px]:min-h-11">{option.label}</DropdownMenuRadioItem
					>
				{/each}
			</DropdownMenuRadioGroup>
		</DropdownMenuContent>
	</DropdownMenu>

	{#if editing}
		<form
			class="flex min-w-0 flex-wrap items-start gap-x-4 gap-y-3"
			onsubmit={apply}
			novalidate
			aria-label="Custom period"
		>
			<div class="w-full min-w-0 min-[480px]:w-44">
				<Label for="{id}-from" class="block text-sm/6 font-medium text-fg">From</Label>
				<Input
					bind:ref={fromInput}
					id="{id}-from"
					type="date"
					max={today}
					bind:value={from}
					aria-invalid={errors.from ? true : undefined}
					aria-describedby={errors.from ? `${id}-from-error` : `${id}-from-help`}
					class="mt-2 w-full min-w-0 {inputClass}"
				/>
				{#if errors.from}
					<p id="{id}-from-error" class={errorClass}>{errors.from}</p>
				{:else}
					<p id="{id}-from-help" class={helpClass}>The first whole day counted.</p>
				{/if}
			</div>
			<div class="w-full min-w-0 min-[480px]:w-44">
				<Label for="{id}-to" class="block text-sm/6 font-medium text-fg">To (optional)</Label>
				<Input
					bind:ref={toInput}
					id="{id}-to"
					type="date"
					max={today}
					bind:value={to}
					aria-invalid={errors.to ? true : undefined}
					aria-describedby={errors.to ? `${id}-to-error` : `${id}-to-help`}
					class="mt-2 w-full min-w-0 {inputClass}"
				/>
				{#if errors.to}
					<p id="{id}-to-error" class={errorClass}>{errors.to}</p>
				{:else}
					<p id="{id}-to-help" class={helpClass}>The last whole day; empty means up to now.</p>
				{/if}
			</div>
			<div class="flex flex-wrap gap-3 min-[480px]:mt-8">
				<Button type="submit" variant="outline" class={mobileButton}>Show</Button>
				<Button type="button" variant="ghost" class={mobileButton} onclick={cancel}>Cancel</Button>
			</div>
		</form>
	{/if}
</div>
