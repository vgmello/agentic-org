<!--
  @component
  New project (offices-and-teams-ux §7.9, TEAM-8/14, development-processes §3): Name and Slug (suggested from the
  name until edited); Accountable team, which starts as "New team: <name>" (a team named after the project, made with
  it in one operation) placed under the organization's top-level team unless the person chooses another place, or an
  existing team, prefilled from the launching team; and the Development process with what choosing it does. No
  project is created without a team, and Create waits until the teams are known (its reason beside it, Retry if they
  can't be read).
  A refusal shows on its field and focuses it. A lost answer is never sent again blind (work-items-ux §12): the sheet
  says "Couldn't confirm whether <name> was created", keeps the values read-only and offers only Check status, which
  asks HQ by the same operation ID; the ID changes only after a definite refusal. Success is said only after HQ
  accepts it. Back with the sheet open closes it only (app-shell-ux §6).
-->
<script lang="ts">
	import { onDestroy, tick } from 'svelte';
	import { beforeNavigate } from '$app/navigation';
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		selectClass
	} from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { newId } from '$lib/modules/organization/commands';
	import { toAppError } from '$lib/proto';
	import { registerModalBack } from '$lib/platform/shell/modal-back';
	import { projectsApi } from '../../api';
	import { OUTCOME_UNKNOWN } from '../../outcome';
	import { nameProblem, slugFrom, slugProblem } from '../../present';
	import type { ProjectField, TeamChoice } from '../../types';
	import ProcessSelect from '../../components/ProcessSelect.svelte';

	let {
		organizationId,
		oncreated,
		onCloseAutoFocus
	}: {
		organizationId: string;
		/** After HQ accepted the project. */
		oncreated: (created: { projectId: string; number: number; name: string }) => Promise<void>;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	/** The parent value for "no parent: a new top-level team". */
	const NO_PARENT = ':root';

	type Outcome =
		| { kind: 'unknown'; checkError?: string }
		| { kind: 'pending' }
		| { kind: 'not_applied'; reason: string }
		| { kind: 'failed'; message: string };

	let open = $state(false);
	let name = $state('');
	let slug = $state('');
	let slugEdited = $state(false);
	let ownerKind = $state<'new' | 'existing'>('new');
	let teamId = $state('');
	let parentId = $state('');
	let processId = $state('');
	let touched = $state(false);
	let busy = $state<'creating' | 'checking' | undefined>();
	let operationId = '';
	let refusal = $state<{ field: ProjectField; message: string }>();
	let outcome = $state<Outcome>();
	let teams = $state.raw<TeamChoice[] | undefined>();
	let teamsError = $state<string>();
	let teamsLoading = $state(false);

	/** The attempt can't be repeated or changed until HQ says what became of it. */
	const unresolved = $derived(outcome?.kind === 'unknown' || outcome?.kind === 'pending');
	const locked = $derived(!!busy || unresolved);
	const nameError = $derived(
		refusal?.field === 'name' ? refusal.message : touched ? nameProblem(name) : undefined
	);
	const slugError = $derived(
		refusal?.field === 'slug'
			? refusal.message
			: touched || slugEdited
				? slugProblem(slug)
				: undefined
	);
	const ownerError = $derived(
		refusal?.field === 'owner'
			? refusal.message
			: touched && ownerKind === 'existing' && !teamId
				? 'Choose the team accountable for the project.'
				: undefined
	);
	const organizationTeam = $derived(teams?.find((t) => t.organizationTeam));
	const parent = $derived(
		parentId === NO_PARENT ? undefined : teams?.find((t) => t.id === parentId)
	);
	const newTeamName = $derived(name.trim() || 'named after the project');
	const waitingForTeams = $derived(!teams);

	function closeFromBack() {
		if (locked) return;
		open = false;
	}

	// Never let rapid Back or navigation unmount the sheet while creation/check is locked,
	// preserving the operation ID needed to resolve unknown outcomes (work-items-ux §12).
	beforeNavigate((navigation) => {
		if (open && locked) navigation.cancel();
	});

	// Back with the sheet open closes it only, and focus returns to its opener (app-shell-ux §6).
	$effect.pre(() => {
		if (open) return registerModalBack(closeFromBack, { locked: () => locked });
	});

	let teamsSeq = 0;
	let destroyed = false;

	onDestroy(() => {
		destroyed = true;
	});

	function loadTeams() {
		const seq = ++teamsSeq;
		const org = organizationId;
		teamsError = undefined;
		teamsLoading = true;
		projectsApi.teamChoices(org).then(
			(rows) => {
				if (destroyed || seq !== teamsSeq || org !== organizationId) return;
				teams = rows;
				teamsLoading = false;
				// The default place for a new team is the organization's top-level team (§7.9).
				if (!parentId) parentId = rows.find((t) => t.organizationTeam)?.id ?? NO_PARENT;
			},
			(e) => {
				if (destroyed || seq !== teamsSeq || org !== organizationId) return;
				teamsError = toAppError(e).message;
				teamsLoading = false;
			}
		);
	}

	/**
	 * Opens the sheet; `presetTeamId` (a team's page or menu) makes that team the accountable one. An attempt whose
	 * outcome is still unknown opens again as it was: it can only be checked, never started afresh.
	 */
	export function start(presetTeamId?: string) {
		open = true;
		if (busy || unresolved) return;
		name = '';
		slug = '';
		slugEdited = false;
		ownerKind = presetTeamId ? 'existing' : 'new';
		teamId = presetTeamId ?? '';
		parentId = '';
		processId = '';
		touched = false;
		busy = undefined;
		refusal = undefined;
		outcome = undefined;
		operationId = newId();
		teams = undefined;
		loadTeams();
	}

	/**
	 * A field changed. After a definite refusal the next try is a new attempt with a new ID; otherwise (a failure HQ
	 * reported as not applied, or nothing sent yet) the same ID is safe to send again.
	 */
	function edited() {
		if (refusal) operationId = newId();
		refusal = undefined;
		if (outcome?.kind === 'failed' || outcome?.kind === 'not_applied') outcome = undefined;
	}

	const fieldSelector = (field: ProjectField) =>
		field === 'owner'
			? ownerKind === 'existing'
				? '#new-project-team'
				: '#new-project-parent'
			: field === 'process'
				? '#new-project-process'
				: `#new-project-${field}`;

	async function finish(answer: { projectId: string; number: number }, created: string) {
		outcome = undefined;
		open = false;
		await oncreated({ ...answer, name: created });
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (locked || waitingForTeams) return;
		touched = true;
		const form = event.currentTarget as HTMLFormElement;
		if (nameProblem(name) || slugProblem(slug) || (ownerKind === 'existing' && !teamId)) {
			await tick();
			form.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
			return;
		}
		busy = 'creating';
		refusal = undefined;
		outcome = undefined;
		const created = name.trim();
		try {
			const answer = await projectsApi.create(organizationId, {
				operationId,
				name: created,
				slug: slug.trim(),
				owner:
					ownerKind === 'new'
						? { kind: 'new', parentTeamId: parent?.id }
						: { kind: 'existing', teamId },
				developmentProcessId: processId || null
			});
			if (destroyed) return;
			busy = undefined;
			await finish(answer, created);
		} catch (e) {
			if (destroyed) return;
			busy = undefined;
			const failure = toAppError(e);
			const field = (failure as { field?: ProjectField }).field;
			if (failure.code === OUTCOME_UNKNOWN) outcome = { kind: 'unknown' };
			else if (field) {
				refusal = { field, message: failure.message };
				await tick();
				if (!destroyed) form.querySelector<HTMLElement>(fieldSelector(field))?.focus();
			} else outcome = { kind: 'failed', message: failure.message };
		}
	}

	async function checkStatus() {
		if (busy) return;
		busy = 'checking';
		const created = name.trim();
		try {
			const result = await projectsApi.createOperation(organizationId, operationId);
			if (destroyed) return;
			busy = undefined;
			if (result.state === 'applied') await finish(result, created);
			else if (result.state === 'not_applied')
				outcome = { kind: 'not_applied', reason: result.reason };
			else outcome = { kind: 'pending' };
		} catch (e) {
			if (destroyed) return;
			busy = undefined;
			outcome = { kind: 'unknown', checkError: toAppError(e).message };
		}
	}
</script>

<Sheet bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<SheetContent
		side="right"
		class="gap-0 data-[side=right]:w-full data-[side=right]:sm:max-w-md"
		{onCloseAutoFocus}
	>
		<SheetHeader class="border-b border-line">
			<SheetTitle>New project</SheetTitle>
			<SheetDescription>
				A project always has one accountable team. Its tasks, docs and settings are added once it
				exists.
			</SheetDescription>
		</SheetHeader>
		<form class="flex min-h-0 flex-1 flex-col" onsubmit={submit} novalidate>
			<div class="flex min-h-0 flex-1 flex-col gap-y-6 overflow-y-auto p-4">
				<div>
					<Label for="new-project-name">Name</Label>
					<Input
						id="new-project-name"
						bind:value={name}
						disabled={locked}
						autocomplete="off"
						class="mt-2 {inputClass}"
						aria-invalid={nameError ? true : undefined}
						aria-describedby={nameError ? 'new-project-name-error' : undefined}
						oninput={() => {
							if (!slugEdited) slug = slugFrom(name);
							edited();
						}}
					/>
					{#if nameError}<p id="new-project-name-error" class={errorClass}>{nameError}</p>{/if}
				</div>

				<div>
					<Label for="new-project-slug">Slug</Label>
					<Input
						id="new-project-slug"
						bind:value={slug}
						disabled={locked}
						autocomplete="off"
						autocapitalize="none"
						spellcheck={false}
						class="mt-2 font-mono {inputClass}"
						aria-invalid={slugError ? true : undefined}
						aria-describedby={slugError ? 'new-project-slug-error' : 'new-project-slug-help'}
						oninput={() => {
							slugEdited = true;
							edited();
						}}
					/>
					{#if slugError}
						<p id="new-project-slug-error" class="{errorClass} [overflow-wrap:anywhere]">
							{slugError}
						</p>
					{:else}
						<p id="new-project-slug-help" class={helpClass}>
							Unique in the organization, such as reservations. Lowercase letters, digits and
							hyphens.
						</p>
					{/if}
				</div>

				<fieldset
					disabled={locked}
					aria-describedby={ownerError ? 'new-project-owner-error' : undefined}
				>
					<legend class="text-sm/6 font-medium text-fg">Accountable team</legend>
					<div class="mt-2 space-y-3">
						<label class="flex items-start gap-x-3 max-[639px]:min-h-11">
							<input
								id="new-project-owner-new"
								type="radio"
								name="owner"
								value="new"
								bind:group={ownerKind}
								onchange={edited}
								class="mt-1 size-4 accent-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
							/>
							<span class="min-w-0 text-sm/6">
								<span class="block font-medium text-fg [overflow-wrap:anywhere]"
									>New team: {newTeamName}</span
								>
								<span class="block text-xs/5 text-fg-muted">
									Made with the project. Add people and agents to it from its page.
								</span>
							</span>
						</label>
						{#if ownerKind === 'new'}
							<div class="pl-7">
								<Label for="new-project-parent">Place it under</Label>
								<select
									id="new-project-parent"
									bind:value={parentId}
									disabled={!teams}
									onchange={edited}
									aria-describedby="new-project-parent-help"
									class="mt-2 {selectClass}"
								>
									{#if !teams}<option value="">Loading teams…</option>{/if}
									{#each teams ?? [] as t (t.id)}
										<option value={t.id}
											>{t.path}{t.ref ? ` · ${t.ref}` : ''}{t.organizationTeam
												? ' (top-level team)'
												: ''}</option
										>
									{/each}
									{#if teams}<option value={NO_PARENT}>No parent: a new top-level team</option>{/if}
								</select>
								<p id="new-project-parent-help" class={helpClass}>
									{organizationTeam
										? `Under ${organizationTeam.name}, the organization's top-level team, unless you choose another place.`
										: 'Where the new team sits in the team tree.'}
								</p>
							</div>
						{/if}
						<label class="flex items-start gap-x-3 max-[639px]:min-h-11">
							<input
								type="radio"
								name="owner"
								value="existing"
								bind:group={ownerKind}
								onchange={edited}
								class="mt-1 size-4 accent-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
							/>
							<span class="text-sm/6 font-medium text-fg">An existing team</span>
						</label>
						{#if ownerKind === 'existing'}
							<div class="pl-7">
								<Label for="new-project-team" class="sr-only">Existing team</Label>
								<select
									id="new-project-team"
									bind:value={teamId}
									disabled={!teams}
									onchange={edited}
									aria-invalid={ownerError ? true : undefined}
									aria-describedby={ownerError ? 'new-project-owner-error' : undefined}
									class={selectClass}
								>
									<option value="" disabled>{teams ? 'Choose a team' : 'Loading teams…'}</option>
									{#each teams ?? [] as t (t.id)}
										<option value={t.id}>{t.path}{t.ref ? ` · ${t.ref}` : ''}</option>
									{/each}
								</select>
							</div>
						{/if}
					</div>
					{#if ownerError}<p id="new-project-owner-error" class={errorClass}>{ownerError}</p>{/if}
				</fieldset>
				{#if teamsError}
					<div role="alert" class="rounded-md border border-line p-3 text-sm/6">
						<p class="font-semibold text-danger-text">Couldn't load the teams</p>
						<p class="mt-1 text-fg-secondary [overflow-wrap:anywhere]">{teamsError}</p>
						<Button
							type="button"
							variant="outline"
							class="mt-3 min-h-9 min-w-20 {mobileButton} [@media(pointer:coarse)]:min-h-11"
							disabled={teamsLoading}
							aria-busy={teamsLoading}
							onclick={loadTeams}>{teamsLoading ? 'Retrying…' : 'Retry'}</Button
						>
					</div>
				{/if}

				<ProcessSelect
					id="new-project-process"
					{organizationId}
					projectId={null}
					bind:value={processId}
					disabled={locked}
					error={refusal?.field === 'process' ? refusal.message : undefined}
				/>

				{#if outcome}
					<div
						role="alert"
						class="rounded-md border border-line bg-tint/[0.03] p-3 text-sm/6"
						data-testid="create-outcome"
					>
						{#if outcome.kind === 'unknown'}
							<p class="font-semibold text-fg">
								Couldn't confirm whether {name.trim()} was created
							</p>
							<p class="mt-1 text-fg-secondary [overflow-wrap:anywhere]">
								The answer from HQ was lost. Check its status before anything else, so the project
								isn't created twice.{outcome.checkError ? ` ${outcome.checkError}` : ''}
							</p>
						{:else if outcome.kind === 'pending'}
							<p class="font-semibold text-fg">Still not confirmed</p>
							<p class="mt-1 text-fg-secondary">
								HQ hasn't finished with this project yet. Check again in a moment.
							</p>
						{:else if outcome.kind === 'not_applied'}
							<p class="font-semibold text-fg">{name.trim()} wasn't created</p>
							<p class="mt-1 text-fg-secondary [overflow-wrap:anywhere]">
								{outcome.reason} Your values are kept; create it when ready.
							</p>
						{:else}
							<p class="font-semibold text-danger-text">The project wasn't created</p>
							<p class="mt-1 text-fg-secondary [overflow-wrap:anywhere]">
								{outcome.message} Your values are kept; try again.
							</p>
						{/if}
					</div>
				{/if}
			</div>
			<!-- Outside the scrolling fields, so Cancel and the action stay in view. -->
			<div data-sheet-footer class="border-t border-line p-4">
				<div class="flex flex-wrap justify-end gap-3 max-[639px]:[&>*]:flex-1">
					<Button
						type="button"
						variant="outline"
						disabled={!!busy}
						class="min-h-9 {mobileButton} [@media(pointer:coarse)]:min-h-11"
						onclick={() => (open = false)}>Cancel</Button
					>
					{#if unresolved}
						<Button
							type="button"
							class="min-h-9 min-w-32 {mobileButton} [@media(pointer:coarse)]:min-h-11"
							aria-busy={busy === 'checking'}
							disabled={busy === 'checking'}
							onclick={checkStatus}>{busy === 'checking' ? 'Checking…' : 'Check status'}</Button
						>
					{:else}
						<Button
							type="submit"
							disabled={!!busy || waitingForTeams}
							aria-busy={busy === 'creating'}
							aria-describedby={waitingForTeams ? 'new-project-waiting' : undefined}
							class="min-h-9 min-w-32 {mobileButton} [@media(pointer:coarse)]:min-h-11"
							>{busy === 'creating' ? 'Creating…' : 'Create project'}</Button
						>
					{/if}
				</div>
				{#if waitingForTeams && !unresolved}
					<p id="new-project-waiting" class="mt-2 text-right text-xs/5 text-fg-muted">
						{teamsError
							? "Can't create until the teams are read: use Retry above."
							: 'Reading the teams first, so the new team goes in the right place…'}
					</p>
				{/if}
			</div>
		</form>
	</SheetContent>
</Sheet>
