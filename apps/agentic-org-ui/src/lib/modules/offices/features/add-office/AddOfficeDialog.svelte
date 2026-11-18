<!--
  @component
  Add office (offices-and-teams-ux §6.2, OFC-2, OFC-3, OFC-7, OFC-29..31): a unique name, then On this instance (the administrator
  only) or On another machine. An invitation is shown once. Adding on this instance shows Creating, Synchronizing and Ready
  separately, and may wait for running agents to stop; closing the dialog doesn't cancel it.
-->
<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/office/api';
	import { trimWhiteSpace, validateName } from '$lib/name';
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
		mobileButton
	} from '$lib/modules/settings/components/classes';
	import { officesApi } from '../../api';
	import { waitingText } from '../../copy';
	import type { AddProgress, Issued, PendingAdd } from '../../types';
	import InvitationPanel from './InvitationPanel.svelte';

	type Stage = 'form' | 'working' | 'waiting' | 'invitation' | 'refused' | 'failed';

	let {
		open = $bindable(false),
		organizationId,
		canAddOnInstance,
		resume,
		onchanged,
		onCloseAutoFocus
	}: {
		open: boolean;
		organizationId: string;
		/** Only an owner who is also the administrator is offered On this instance (OFC-7). */
		canAddOnInstance: boolean;
		/** A waiting add whose pending row was chosen: open on its Waiting state. */
		resume?: PendingAdd | undefined;
		onchanged: () => void | Promise<void>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let stage = $state<Stage>('form');
	let name = $state('');
	let where = $state<'instance' | 'machine'>('instance');
	let nameError = $state<string | undefined>();
	let error = $state<string | undefined>();
	let note = $state<string | undefined>();
	let busy = $state(false);
	let operationId = $state<string | undefined>();
	let progress = $state<AddProgress | undefined>();
	let issued = $state<Issued | undefined>();
	let nameInput = $state<HTMLInputElement | null>(null);
	let timer: ReturnType<typeof setInterval> | undefined;
	let ended = false;
	onDestroy(() => clearInterval(timer));

	$effect(() => {
		if (!open) return;
		// Only opening resets the form: a list that reloads behind the dialog must not clear what was typed.
		untrack(() => {
			clearInterval(timer);
			error = undefined;
			note = undefined;
			nameError = undefined;
			issued = undefined;
			progress = undefined;
			if (resume) {
				name = resume.name;
				operationId = resume.id;
				stage = 'waiting';
				watch(resume.id);
			} else {
				name = '';
				operationId = undefined;
				stage = 'form';
				where = canAddOnInstance ? 'instance' : 'machine';
			}
		});
	});

	const trimmed = $derived(trimWhiteSpace(name));
	function checkName(): string | undefined {
		const check = validateName(name, 'Enter a name for the office.');
		return check.ok ? undefined : check.message;
	}

	function watch(id: string) {
		ended = false;
		clearInterval(timer);
		const poll = async () => {
			if (ended) return;
			try {
				const next = await officesApi.addProgress(organizationId, id);
				if (ended || operationId !== id) return;
				progress = next;
				if (next.phase === 'waiting') stage = 'waiting';
				else if (next.phase === 'creating' || next.phase === 'synchronizing') stage = 'working';
				else {
					ended = true;
					clearInterval(timer);
					if (next.phase === 'ready') {
						open = false;
						toast.success(`${name} added. It's starting.`);
					} else if (next.phase === 'failed') {
						error = next.reason;
						stage = 'failed';
					} else {
						note = `${name} wasn't added.`;
						stage = 'form';
					}
					await onchanged();
				}
			} catch (e) {
				ended = true;
				clearInterval(timer);
				error = toAppError(e).message;
				stage = 'failed';
			}
		};
		void poll();
		timer = setInterval(poll, 700);
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		nameError = checkName();
		if (nameError) return nameInput?.focus();
		busy = true;
		error = undefined;
		try {
			if (where === 'machine') {
				issued = await officesApi.invite(organizationId, trimmed);
				stage = 'invitation';
				await onchanged();
			} else {
				const started = await officesApi.addOnInstance(organizationId, trimmed);
				operationId = started.operationId;
				stage = 'working';
				progress = { phase: 'creating' };
				watch(started.operationId);
			}
		} catch (e) {
			const failure = toAppError(e);
			if (failure.code === 'not_administrator') {
				stage = 'refused';
				error = failure.message;
			} else if (failure.code === 'already_exists' || failure.code === 'invalid_name') {
				nameError = failure.message;
				nameInput?.focus();
			} else error = failure.message;
		} finally {
			busy = false;
		}
	}

	async function cancel() {
		if (!operationId || busy) return;
		busy = true;
		try {
			const cancelled = await officesApi.cancelAdd(organizationId, operationId);
			if (cancelled) {
				ended = true;
				clearInterval(timer);
				note = `${name} wasn't added.`;
				stage = 'form';
				await onchanged();
			} else note = `Too late to cancel: ${name} is being added now.`;
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}

	function inviteInstead() {
		where = 'machine';
		error = undefined;
		stage = 'form';
	}
	const readOnly = $derived(stage === 'working' || stage === 'waiting');
	const title = $derived(
		stage === 'invitation' && issued
			? `Invitation for ${issued.invitation.officeName}`
			: canAddOnInstance
				? 'Add office'
				: 'Add an office on another machine'
	);
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent {onCloseAutoFocus} class="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-lg">
		<DialogHeader>
			<DialogTitle text={title} />
			<DialogDescription class="[overflow-wrap:anywhere]">
				{#if stage === 'invitation'}Send it to the machine that will run this office.{:else if stage === 'refused'}An
					office on this instance needs this HQ's administrator.{:else}An office is another machine
					or instance that runs agents and keeps a copy of the records.{/if}
			</DialogDescription>
		</DialogHeader>

		{#if stage === 'invitation' && issued}
			<InvitationPanel {issued} ondone={() => (open = false)} />
		{:else if stage === 'refused'}
			<p class="text-sm/6 text-red-300" role="alert">{error}</p>
			<DialogFooter>
				<Button type="button" variant="outline" class={mobileButton} onclick={() => (open = false)}
					>Close</Button
				>
				<Button type="button" class={mobileButton} onclick={inviteInstead}
					>Invite another machine</Button
				>
			</DialogFooter>
		{:else if stage === 'failed'}
			<p class="text-sm/6 text-red-300 [overflow-wrap:anywhere]" role="alert">{error}</p>
			<DialogFooter>
				<Button type="button" variant="outline" class={mobileButton} onclick={() => (open = false)}
					>Close</Button
				>
				<Button type="button" class={mobileButton} onclick={() => (stage = 'form')}
					>Try again</Button
				>
			</DialogFooter>
		{:else}
			<form class="grid gap-5" onsubmit={submit} novalidate>
				<div>
					<Label for="office-name" class="block text-sm/6 font-medium text-white">Office name</Label
					>
					<div class="mt-2">
						<Input
							bind:ref={nameInput}
							id="office-name"
							bind:value={name}
							readonly={readOnly}
							autocomplete="off"
							aria-invalid={nameError ? true : undefined}
							aria-describedby={nameError ? 'office-name-error' : 'office-name-help'}
							class={inputClass}
						/>
					</div>
					{#if nameError}<p id="office-name-error" class={errorClass} role="alert">
							{nameError}
						</p>{:else}<p id="office-name-help" class={helpClass}>
							Unique within this organization. You can rename it later.
						</p>{/if}
				</div>
				{#if canAddOnInstance}
					<fieldset class="space-y-2" disabled={readOnly}>
						<legend class="text-sm/6 font-medium text-white">Where it runs</legend>
						<label
							class="flex min-h-9 items-start gap-x-3 text-sm/6 text-gray-300 max-[639px]:min-h-11"
						>
							<input
								type="radio"
								name="where"
								value="instance"
								bind:group={where}
								class="mt-1.5 size-4 accent-indigo-500"
							/>
							<span
								><span class="block font-medium text-white">On this instance</span><span
									class="block text-xs/5 text-gray-400"
									>A new office on the machine that runs HQ.</span
								></span
							>
						</label>
						<label
							class="flex min-h-9 items-start gap-x-3 text-sm/6 text-gray-300 max-[639px]:min-h-11"
						>
							<input
								type="radio"
								name="where"
								value="machine"
								bind:group={where}
								class="mt-1.5 size-4 accent-indigo-500"
							/>
							<span
								><span class="block font-medium text-white">On another machine</span><span
									class="block text-xs/5 text-gray-400"
									>An invitation you send to it. You approve the join here.</span
								></span
							>
						</label>
					</fieldset>
				{/if}

				{#if stage === 'working' && progress}
					<ol class="space-y-1 text-sm/6" aria-label="Progress" data-testid="add-progress">
						{#each [['creating', 'Creating'], ['synchronizing', 'Synchronizing'], ['ready', 'Ready']] as [phase, label] (phase)}
							{@const order = ['creating', 'synchronizing', 'ready']}
							{@const at = order.indexOf(progress.phase)}
							{@const mine = order.indexOf(phase)}
							<li
								class={mine < at
									? 'text-gray-300'
									: mine === at
										? 'font-semibold text-white'
										: 'text-gray-500'}
								aria-current={mine === at ? 'step' : undefined}
							>
								{label}{mine < at ? ' · done' : mine === at ? '…' : ''}
							</li>
						{/each}
					</ol>
					<p class="sr-only" role="status">
						{progress.phase === 'creating' ? 'Creating' : 'Synchronizing'}
					</p>
				{/if}
				{#if stage === 'waiting'}
					<div
						class="space-y-2 rounded-lg bg-white/5 p-4 text-sm/6 text-gray-300 outline-1 -outline-offset-1 outline-white/10"
						data-testid="add-waiting"
					>
						<p class="font-medium text-white" role="status">
							{waitingText(progress?.waitingFor ?? resume?.waitingFor ?? [])}.
						</p>
						<p>
							{name} is added once the instance confirms it is safe. No agent is automatically stopped
							or restarted for this.
						</p>
					</div>
				{/if}
				{#if note}<p class="text-sm/6 text-gray-300" role="status" data-testid="add-note">
						{note}
					</p>{/if}
				{#if error}<p class="text-sm/6 text-red-300" role="alert">{error}</p>{/if}

				<DialogFooter>
					{#if readOnly}
						<Button
							type="button"
							variant="outline"
							class={mobileButton}
							onclick={() => (open = false)}>Close</Button
						>
						<Button
							type="button"
							class={mobileButton}
							aria-busy={busy}
							aria-disabled={busy}
							onclick={cancel}>{busy ? 'Cancelling…' : 'Cancel'}</Button
						>
					{:else}
						<Button
							type="button"
							variant="outline"
							class={mobileButton}
							disabled={busy}
							onclick={() => (open = false)}>Cancel</Button
						>
						<Button type="submit" class={mobileButton} aria-busy={busy} aria-disabled={busy}
							>{busy
								? where === 'machine'
									? 'Creating invitation…'
									: 'Adding…'
								: where === 'machine'
									? 'Create invitation'
									: 'Add office'}</Button
						>
					{/if}
				</DialogFooter>
			</form>
		{/if}
	</DialogContent>
</Dialog>
