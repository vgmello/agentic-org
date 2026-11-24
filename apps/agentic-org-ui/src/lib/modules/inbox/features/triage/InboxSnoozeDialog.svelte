<!--
  @component
  Custom date & time snooze dialog for Inbox items (TASK-81).
  Validates future return timestamps in the viewing account's time zone and warns if the chosen return time
  falls after the item's expiry deadline.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
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
	import { formatFull } from '$lib/modules/preferences/time';
	import {
		defaultCustomSnoozeInputs,
		parseCustomSnoozeDateTime,
		snoozeExceedsExpiry
	} from './snooze-presets';
	import type { TriageItem } from './triage-model';

	let {
		zone,
		busy = false,
		onsubmit,
		onCloseAutoFocus
	}: {
		zone: string;
		busy?: boolean;
		onsubmit: (item: TriageItem, untilIso: string, note?: string) => Promise<boolean>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	const id = $props.id();
	let open = $state(false);
	let target = $state<TriageItem | undefined>(undefined);
	let dateValue = $state('');
	let timeValue = $state('');
	let noteValue = $state('');
	let fieldError = $state<{ field: 'date' | 'time'; message: string } | undefined>(undefined);
	let submitting = $state(false);
	let dateInputEl = $state<HTMLInputElement | null>(null);
	let timeInputEl = $state<HTMLInputElement | null>(null);

	export function start(item: TriageItem) {
		target = item;
		const defaults = defaultCustomSnoozeInputs(new Date(), zone);
		dateValue = defaults.date;
		timeValue = defaults.time;
		noteValue = item.snoozed?.note ?? '';
		fieldError = undefined;
		submitting = false;
		open = true;
	}

	const livePreview = $derived.by(() => {
		if (!dateValue || !timeValue) return undefined;
		const parsedNoExpiry = parseCustomSnoozeDateTime(dateValue, timeValue, zone, new Date());
		if (!parsedNoExpiry.ok) return undefined;
		return {
			iso: parsedNoExpiry.iso,
			full: formatFull(parsedNoExpiry.date, zone),
			exceedsExpiry: snoozeExceedsExpiry(parsedNoExpiry.iso, target?.expiresAtIso)
		};
	});

	const expiryFull = $derived.by(() => {
		if (!target?.expiresAtIso) return undefined;
		const d = new Date(target.expiresAtIso);
		return Number.isNaN(d.getTime()) ? undefined : formatFull(d, zone);
	});

	async function handleSubmit(event: SubmitEvent) {
		event.preventDefault();
		if (!target || submitting || busy) return;
		const parsed = parseCustomSnoozeDateTime(
			dateValue,
			timeValue,
			zone,
			new Date(),
			target.expiresAtIso
		);
		if (!parsed.ok) {
			fieldError = { field: parsed.field, message: parsed.error };
			await tick();
			if (parsed.field === 'date') dateInputEl?.focus();
			else timeInputEl?.focus();
			return;
		}
		fieldError = undefined;
		submitting = true;
		try {
			const ok = await onsubmit(target, parsed.iso, noteValue.trim() || undefined);
			if (ok) open = false;
		} finally {
			submitting = false;
		}
	}

	function handleOpenChange(next: boolean) {
		if (!next && submitting) {
			open = true;
		}
	}
</script>

<Dialog bind:open onOpenChange={handleOpenChange}>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-md">
		{#if target}
			<form class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-5" onsubmit={handleSubmit} novalidate>
				<DialogHeader>
					<DialogTitle text={target.state === 'snoozed' ? 'Re-snooze item' : 'Snooze until…'} />
					<DialogDescription class="[overflow-wrap:anywhere]">
						{target.title} · Times use {zone}.
					</DialogDescription>
				</DialogHeader>

				<fieldset
					disabled={submitting || busy}
					class="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-4"
				>
					<div class="grid grid-cols-[minmax(0,1fr)] gap-4 min-[420px]:grid-cols-2">
						<div>
							<Label for="{id}-date" class="block text-sm/6 font-medium text-fg">Return date</Label>
							<Input
								bind:ref={dateInputEl}
								id="{id}-date"
								type="date"
								bind:value={dateValue}
								oninput={() => {
									fieldError = undefined;
								}}
								aria-invalid={fieldError?.field === 'date' ? 'true' : undefined}
								aria-describedby={fieldError?.field === 'date' ? `${id}-date-error` : undefined}
								class="mt-1.5 {inputClass}"
							/>
							{#if fieldError?.field === 'date'}
								<p id="{id}-date-error" class={errorClass}>{fieldError.message}</p>
							{/if}
						</div>

						<div>
							<Label for="{id}-time" class="block text-sm/6 font-medium text-fg">Return time</Label>
							<Input
								bind:ref={timeInputEl}
								id="{id}-time"
								type="time"
								bind:value={timeValue}
								oninput={() => {
									fieldError = undefined;
								}}
								aria-invalid={fieldError?.field === 'time' ? 'true' : undefined}
								aria-describedby={fieldError?.field === 'time' ? `${id}-time-error` : undefined}
								class="mt-1.5 {inputClass}"
							/>
							{#if fieldError?.field === 'time'}
								<p id="{id}-time-error" class={errorClass}>{fieldError.message}</p>
							{/if}
						</div>
					</div>

					{#if livePreview}
						<p class={helpClass} data-testid="snooze-preview">
							Returns to your open inbox on <strong class="font-medium text-fg"
								>{livePreview.full}</strong
							>.
						</p>
					{/if}

					{#if livePreview?.exceedsExpiry}
						<div
							role="status"
							data-testid="snooze-expiry-warning"
							class="flex items-start gap-2 rounded-md border border-line bg-surface-raised px-3 py-2.5 text-xs/5 text-fg-secondary"
						>
							<AlertTriangle class="mt-0.5 size-4 shrink-0 text-fg" aria-hidden="true" />
							<p class="[overflow-wrap:anywhere]">
								Return time must be earlier than the item's expiry deadline{expiryFull
									? ` (${expiryFull})`
									: ''}. Choose an earlier time to snooze this request.
							</p>
						</div>
					{/if}

					<div>
						<Label for="{id}-note" class="block text-sm/6 font-medium text-fg">
							Snooze note (optional)
						</Label>
						<Input
							id="{id}-note"
							type="text"
							maxlength={240}
							autocomplete="off"
							bind:value={noteValue}
							placeholder="Why you deferred this item…"
							class="mt-1.5 {inputClass}"
						/>
						<p class={helpClass}>
							Not private: visible to the sender, in your Snoozed list, and in activity history.
						</p>
					</div>
				</fieldset>

				<DialogFooter class="flex-wrap gap-2">
					<Button
						type="button"
						variant="outline"
						disabled={submitting}
						onclick={() => {
							open = false;
						}}
						class={mobileButton}
					>
						Cancel
					</Button>
					<Button type="submit" disabled={submitting || busy} class={mobileButton}>
						{submitting ? 'Snoozing…' : target.state === 'snoozed' ? 'Update snooze' : 'Snooze'}
					</Button>
				</DialogFooter>
			</form>
		{/if}
	</DialogContent>
</Dialog>
