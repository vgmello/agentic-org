<script lang="ts">
	import { untrack } from 'svelte';
	import Check from '@lucide/svelte/icons/check';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import { ADD_STEPS, officesApi, type AddStep, type OfficeStartError } from '$lib/offices/api';
	import { suggestOfficeName, validateOfficeName } from '$lib/offices/model';
	import { toAppError } from '$lib/office/api';
	import type { AppError, OfficeWithExtras } from '$lib/proto';

	let {
		open = $bindable(false),
		offices,
		sample = false,
		onchanged
	}: { open: boolean; offices: OfficeWithExtras[]; /** Adding is simulated (no real host yet): say so. */ sample?: boolean; /** The list changed (an office was added, or was added but stopped): reload it. */ onchanged: () => void } = $props();

	let name = $state('');
	let nameError = $state<string | undefined>();
	let step = $state<AddStep | undefined>();
	let failure = $state<AppError | undefined>();
	let busy = $state(false);
	let copied = $state(false);
	// The office that was created but didn't start, so Try again starts that one instead of adding another.
	let stoppedId = $state<string | undefined>();

	// Each time the sheet opens, start fresh with a free "Office N".
	// (Only `open` triggers this: the list reloading behind the sheet must not wipe a failure message.)
	$effect(() => {
		if (open) {
			untrack(() => {
				name = suggestOfficeName(offices);
				nameError = undefined;
				failure = undefined;
				step = undefined;
				stoppedId = undefined;
				copied = false;
			});
		}
	});

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (busy) return;
		const retrying = !!stoppedId;
		if (!retrying) {
			const checked = validateOfficeName(name, offices);
			if (!checked.ok) {
				nameError = checked.message;
				return;
			}
			name = checked.name;
		}
		busy = true;
		failure = undefined;
		try {
			if (retrying && stoppedId) await officesApi.restart(stoppedId, (s) => (step = s));
			else await officesApi.addHere(name, (s) => (step = s));
			open = false;
			onchanged();
		} catch (error) {
			failure = toAppError(error);
			const officeId = (failure as Partial<OfficeStartError>).office_id;
			if (failure.code === 'invalid_name' || failure.code === 'already_exists') {
				nameError = failure.message; // about what was typed: fix it in the field
				failure = undefined;
			} else if (failure.code === 'not_found') {
				stoppedId = undefined; // the stopped office is gone: the next try adds a new one
				onchanged();
			} else {
				stoppedId = officeId ?? stoppedId; // only an error that names an office leaves a Stopped one
				onchanged(); // show how things are now
			}
		} finally {
			busy = false;
		}
	}

	async function copyDetails() {
		try {
			await navigator.clipboard.writeText(`${failure?.message ?? ''}\nOffice: ${name}`);
			copied = true;
		} catch {
			copied = false; // clipboard unavailable: nothing to do
		}
	}
	const reached = (s: AddStep) => (step ? ADD_STEPS.indexOf(s) < ADD_STEPS.indexOf(step) : false);
</script>

<!-- Escape, a click outside or the X don't dismiss the sheet while an office is being added: the work would go on unseen. -->
<Sheet bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<SheetContent side="right" class="w-full max-w-md">
		<SheetHeader>
			<SheetTitle>Add office</SheetTitle>
			<SheetDescription>An office is where agents run and a copy of the organization's records is kept.</SheetDescription>
			{#if sample}<p class="flex items-center gap-x-2 text-xs/5 text-gray-400" data-testid="sample-data-note-add"><Badge variant="secondary">Sample data</Badge> Simulated: nothing is started on this Mac.</p>{/if}
		</SheetHeader>

		<form class="flex flex-1 flex-col gap-y-6 px-4" onsubmit={submit} novalidate>
			<div class="space-y-2">
				<Label for="office-name">Name</Label>
				<Input
					id="office-name"
					bind:value={name}
					disabled={busy || !!stoppedId}
					autocomplete="off"
					aria-invalid={nameError ? true : undefined}
					aria-describedby={nameError ? 'office-name-error' : undefined}
					oninput={() => (nameError = undefined)}
				/>
				{#if nameError}<p id="office-name-error" class="text-sm/6 text-red-400" role="alert">{nameError}</p>{/if}
			</div>

			<fieldset class="space-y-3" disabled={busy}>
				<legend class="text-sm/6 font-medium text-white">Where will it run?</legend>
				<label class="flex gap-x-3 rounded-lg bg-white/[0.03] p-3 opacity-60 outline-1 -outline-offset-1 outline-white/10">
					<input type="radio" name="where" value="other" disabled class="mt-1" />
					<span class="text-sm/6">
						<span class="block font-semibold text-white">Another computer</span>
						<span class="block text-gray-400">You'll get a one-time link to open on that computer. Available in a later release.</span>
					</span>
				</label>
				<label class="flex gap-x-3 rounded-lg bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10">
					<input type="radio" name="where" value="here" checked class="mt-1" />
					<span class="text-sm/6">
						<span class="block font-semibold text-white">This Mac</span>
						<span class="block text-gray-400">A separate office on this Mac, with its own copy and agents. Ready in seconds.</span>
					</span>
				</label>
			</fieldset>

			{#if busy || failure}
				<ol class="min-w-0 space-y-2" aria-live="polite" data-testid="add-steps">
					{#each ADD_STEPS as s (s)}
						<li class="flex min-w-0 items-center gap-x-3 text-sm/6 [overflow-wrap:anywhere] {step === s || reached(s) ? 'text-white' : 'text-gray-500'}">
							{#if reached(s)}<Check class="size-4 text-green-400" aria-hidden="true" />
							{:else if step === s && busy}<LoaderCircle class="size-4 animate-spin text-indigo-400" aria-hidden="true" />
							{:else if step === s && failure}<CircleAlert class="size-4 text-red-400" aria-hidden="true" />
							{:else}<span class="size-4 rounded-full border border-white/20" aria-hidden="true"></span>{/if}
							{s}{s === 'Creating' ? ` ${name}` : ''}
						</li>
					{/each}
				</ol>
			{/if}

			{#if failure}
				<Alert variant="destructive">
					<CircleAlert aria-hidden="true" />
					<AlertTitle class="[overflow-wrap:anywhere]">{failure.message}</AlertTitle>
					<AlertDescription>{stoppedId ? 'It is listed as Stopped. Try again, or remove it from the list.' : 'Try again in a moment.'}</AlertDescription>
				</Alert>
			{/if}

			<SheetFooter class="mt-auto px-0">
				{#if failure}
					<Button type="button" variant="outline" onclick={copyDetails}>{copied ? 'Copied' : 'Copy details'}</Button>
					<Button type="button" variant="outline" onclick={() => (open = false)}>Close</Button>
					<Button type="submit" disabled={busy}>Try again</Button>
				{:else}
					<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
					<Button type="submit" disabled={busy}>Continue</Button>
				{/if}
			</SheetFooter>
		</form>
	</SheetContent>
</Sheet>
