<!--
  @component
  Edit how an agent runs (our-org-in-the-app §5 "Agent setup fields"; agent-run-modes-ux §2; document 11 §4): its
  model profile and canonical model, written to `execution.md` against the execution revision the person saw. The
  employee record is never part of it. Profiles and models are visible radios with what the office reports about
  each; nothing is picked for the person, and a profile whose models the office hasn't reported takes a typed
  model ID, checked before the next turn. A conflict shows the newer version behind the dialog and keeps the
  person's choices; any other refusal stays inline. Mount it in a {#key} that changes on each opening.
-->
<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
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
	import { toAppError } from '$lib/proto';
	import { agentSetupApi } from '../../api';
	import { modelNote, profileLabel } from '../../present';
	import type { AgentSetupDetail } from '../../types';
	import { latestOf } from '../agent-page/lookup';
	import { chooseProfile, initialForm, missingText, modelOptions, toEdit, unchanged } from './form';

	let {
		open = $bindable(false),
		organizationId,
		agent,
		onlatest,
		onsaved,
		onCloseAutoFocus
	}: {
		open: boolean;
		organizationId: string;
		/** The latest setup: after a conflict, the newer one, whose revision the next save is checked against. */
		agent: AgentSetupDetail;
		onlatest: (latest: AgentSetupDetail) => void;
		onsaved: (saved: AgentSetupDetail) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	const ids = $props.id();
	let form = $state(untrack(() => initialForm(agent)));
	let busy = $state(false);
	let error = $state<string | undefined>();
	let tried = $state(false);
	/** After a conflict: what is saved now, so the person can compare without leaving the dialog. */
	let nowSaved = $state<string | undefined>();
	let formElement = $state<HTMLFormElement | null>(null);
	let alertElement = $state<HTMLElement | null>(null);

	/** A refusal focuses what it is about (app-shell-ux §6a): the first field still missing, else the reason itself. */
	async function focusRefusal() {
		await tick();
		const field = !form.profileId
			? `input[name="${ids}-profile"]`
			: typed
				? `#${CSS.escape(`${ids}-model`)}`
				: `input[name="${ids}-model"]`;
		const target = missing ? formElement?.querySelector<HTMLElement>(field) : alertElement;
		target?.focus();
	}

	const profile = $derived(agent.profiles.find((p) => p.id === form.profileId));
	const saved = $derived(
		agent.execution && agent.execution.profile.id === form.profileId
			? agent.execution.modelId
			: undefined
	);
	const options = $derived(modelOptions(profile, saved));
	const typed = $derived(!!profile && profile.models === null);
	const missing = $derived(missingText(form));
	const name = $derived(agent.name);

	function changeOpen(next: boolean) {
		if (!next && busy) open = true;
	}

	function pickProfile(id: string) {
		form = chooseProfile(
			form,
			agent.profiles.find((p) => p.id === id)
		);
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		tried = true;
		if (missing) return void focusRefusal();
		if (unchanged(form, agent)) return void (open = false);
		busy = true;
		error = undefined;
		try {
			const result = await agentSetupApi.saveExecution(
				organizationId,
				agent.memberId,
				toEdit(form, agent.execution?.revision ?? 0)
			);
			open = false;
			onsaved(result);
		} catch (e) {
			const failure = toAppError(e);
			const latest = failure.code === 'conflict' ? latestOf(e) : undefined;
			if (latest) {
				onlatest(latest);
				const e = latest.execution;
				nowSaved = e
					? `Saved now, rev ${e.revision}${e.updated?.byName ? ` by ${e.updated.byName}` : ''}: ${e.profile.name} · ${e.modelId}.`
					: 'Saved now: execution not configured.';
			} else nowSaved = undefined;
			error = failure.message;
			busy = false;
			void focusRefusal();
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={changeOpen}>
	<DialogContent
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4 sm:max-w-lg"
	>
		<form class="grid gap-6" onsubmit={save} novalidate bind:this={formElement}>
			<DialogHeader class="pr-8">
				<DialogTitle text="Edit how {name} runs" />
				<DialogDescription>
					Its model profile and model. Saving changes {name}'s execution configuration only, from
					its next turn; a turn already running keeps its model.
				</DialogDescription>
			</DialogHeader>

			<fieldset disabled={busy}>
				<legend class="text-sm/6 font-medium text-fg">Model profile</legend>
				<p class="mt-1 text-xs/5 text-fg-muted">
					A profile is an ordered pool of connections. The runtime comes from its connections.
				</p>
				<div class="mt-2 grid gap-2">
					{#each agent.profiles as p (p.id)}
						<label
							class="flex min-h-11 cursor-pointer items-start gap-x-3 rounded-md px-3 py-2 ring-1 ring-line ring-inset has-checked:bg-surface-raised has-checked:ring-brand has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-focus min-[640px]:min-h-9"
						>
							<input
								type="radio"
								name="{ids}-profile"
								value={p.id}
								checked={form.profileId === p.id}
								onchange={() => pickProfile(p.id)}
								class="mt-1 size-4 shrink-0 accent-primary focus-visible:outline-none"
							/>
							<span class="min-w-0 text-sm/6 text-fg [overflow-wrap:anywhere]"
								>{profileLabel(p)}</span
							>
						</label>
					{/each}
				</div>
			</fieldset>

			{#if profile}
				{#if typed}
					<div>
						<Label for="{ids}-model">Model</Label>
						<Input
							id="{ids}-model"
							bind:value={form.modelId}
							disabled={busy}
							autocomplete="off"
							spellcheck={false}
							class="mt-2 font-mono {inputClass}"
							aria-invalid={tried && !form.modelId.trim() ? true : undefined}
							aria-describedby="{ids}-model-help"
						/>
						<p id="{ids}-model-help" class={helpClass}>
							The office hasn't reported this profile's models yet. Type the canonical model ID; it
							is checked before {name}'s next turn and never replaced by another model.
						</p>
					</div>
				{:else}
					<fieldset disabled={busy} aria-describedby="{ids}-models-help">
						<legend class="text-sm/6 font-medium text-fg">Model</legend>
						<p id="{ids}-models-help" class="mt-1 text-xs/5 text-fg-muted">
							As the agent's office last reported them. Ready connections aren't a promise of
							capacity.
						</p>
						<div class="mt-2 grid gap-2">
							{#each options as option (option.id)}
								{@const m = profile.models?.find((x) => x.id === option.id)}
								<label
									class="flex min-h-11 cursor-pointer items-start gap-x-3 rounded-md px-3 py-2 ring-1 ring-line ring-inset has-checked:bg-surface-raised has-checked:ring-brand has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-focus min-[640px]:min-h-9"
								>
									<input
										type="radio"
										name="{ids}-model"
										value={option.id}
										bind:group={form.modelId}
										class="mt-1 size-4 shrink-0 accent-primary focus-visible:outline-none"
									/>
									<span class="min-w-0 text-sm/6">
										<span class="block font-mono text-fg [overflow-wrap:anywhere]">{option.id}</span
										>
										<span class="block text-xs/5 text-fg-muted"
											>{m
												? modelNote(m)
												: "Saved, but the office doesn't report it for this profile"}</span
										>
									</span>
								</label>
							{/each}
						</div>
						{#if options.length === 0}
							<p class="mt-2 text-sm/6 text-fg-muted">
								The office reports no models for this profile. Choose another profile.
							</p>
						{/if}
					</fieldset>
				{/if}
			{/if}

			{#if tried && missing}
				<p class={errorClass} role="alert">{missing}</p>
			{/if}
			{#if error}<div
					role="alert"
					tabindex="-1"
					bind:this={alertElement}
					class="text-sm/6 [overflow-wrap:anywhere] focus:outline-hidden"
				>
					<p class="text-danger-text">{error}</p>
					{#if nowSaved}<p class="mt-1 text-fg-secondary" data-testid="now-saved">
							{nowSaved}
						</p>{/if}
				</div>{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => (open = false)}>Cancel</Button
				>
				<Button type="submit" class="min-w-20 {mobileButton}" aria-busy={busy} aria-disabled={busy}
					>{busy ? 'Saving…' : 'Save'}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
