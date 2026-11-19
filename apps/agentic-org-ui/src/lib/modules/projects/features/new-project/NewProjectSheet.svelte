<!--
  @component
  New project (offices-and-teams-ux §7.9, TEAM-8/14, development-processes §3): Name and Slug (suggested from the
  name until edited); Accountable team, which starts as "New team: <name>" (a team named after the project, made with
  it in one operation, under the top-level team) or an existing team, prefilled from the launching team; and the
  Development process with what choosing it does. No project is created without a team. A refusal shows on its field
  and focuses it; the same attempt is resent on Try again, so a lost answer never creates a second project. Success is
  said only after HQ accepts it.
-->
<script lang="ts">
	import { tick } from 'svelte';
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
	import { newId } from '$lib/org/commands';
	import { toAppError } from '$lib/office/api';
	import { projectsApi } from '../../api';
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

	let open = $state(false);
	let name = $state('');
	let slug = $state('');
	let slugEdited = $state(false);
	let ownerKind = $state<'new' | 'existing'>('new');
	let teamId = $state('');
	let processId = $state('');
	let touched = $state(false);
	let busy = $state(false);
	let operationId = '';
	let refusal = $state<{ field?: ProjectField; message: string }>();
	let teams = $state.raw<TeamChoice[] | undefined>();
	let teamsError = $state<string>();

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
	const topTeam = $derived(teams?.find((t) => t.topLevel));
	const newTeamName = $derived(name.trim() || 'named after the project');

	/** Opens the sheet; `presetTeamId` (a team's page or menu) makes that team the accountable one. */
	export function start(presetTeamId?: string) {
		name = '';
		slug = '';
		slugEdited = false;
		ownerKind = presetTeamId ? 'existing' : 'new';
		teamId = presetTeamId ?? '';
		processId = '';
		touched = false;
		busy = false;
		refusal = undefined;
		operationId = newId();
		open = true;
		teamsError = undefined;
		void projectsApi.teamChoices(organizationId).then(
			(rows) => (teams = rows),
			(e) => (teamsError = toAppError(e).message)
		);
	}

	/** A field changed after a refusal: what was sent is no longer what is asked, so the next try is a new attempt. */
	function edited() {
		refusal = undefined;
		operationId = newId();
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		touched = true;
		const form = event.currentTarget as HTMLFormElement;
		if (nameProblem(name) || slugProblem(slug) || (ownerKind === 'existing' && !teamId)) {
			await tick();
			form.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
			return;
		}
		busy = true;
		refusal = undefined;
		const created = name.trim();
		try {
			const answer = await projectsApi.create(organizationId, {
				operationId,
				name: created,
				slug: slug.trim(),
				owner:
					ownerKind === 'new'
						? { kind: 'new', parentTeamId: topTeam?.id }
						: { kind: 'existing', teamId },
				developmentProcessId: processId || null
			});
			open = false;
			await oncreated({ ...answer, name: created });
		} catch (e) {
			const failure = toAppError(e);
			const field = (failure as { field?: ProjectField }).field;
			refusal = { field, message: failure.message };
			busy = false;
			await tick();
			if (field)
				form
					.querySelector<HTMLElement>(
						field === 'owner'
							? '#new-project-team, #new-project-owner-new'
							: field === 'process'
								? '#new-project-process'
								: `#new-project-${field}`
					)
					?.focus();
		} finally {
			busy = false;
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
						disabled={busy}
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
						disabled={busy}
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

				<fieldset aria-describedby={ownerError ? 'new-project-owner-error' : undefined}>
					<legend class="text-sm/6 font-medium text-fg">Accountable team</legend>
					<div class="mt-2 space-y-3">
						<label class="flex items-start gap-x-3 max-[639px]:min-h-11">
							<input
								id="new-project-owner-new"
								type="radio"
								name="owner"
								value="new"
								bind:group={ownerKind}
								disabled={busy}
								onchange={edited}
								class="mt-1 size-4 accent-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
							/>
							<span class="min-w-0 text-sm/6">
								<span class="block font-medium text-fg [overflow-wrap:anywhere]"
									>New team: {newTeamName}</span
								>
								<span class="block text-xs/5 text-fg-muted">
									Made with the project{topTeam ? `, under ${topTeam.name}` : ''}. Add people and
									agents to it from its page.
								</span>
							</span>
						</label>
						<label class="flex items-start gap-x-3 max-[639px]:min-h-11">
							<input
								type="radio"
								name="owner"
								value="existing"
								bind:group={ownerKind}
								disabled={busy}
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
									disabled={busy || !teams}
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
								{#if teamsError}
									<p class={errorClass}>Couldn't load the teams: {teamsError}</p>
								{/if}
							</div>
						{/if}
					</div>
					{#if ownerError}<p id="new-project-owner-error" class={errorClass}>{ownerError}</p>{/if}
				</fieldset>

				<ProcessSelect
					id="new-project-process"
					{organizationId}
					projectId={null}
					bind:value={processId}
					disabled={busy}
					error={refusal?.field === 'process' ? refusal.message : undefined}
				/>

				{#if refusal && !refusal.field}
					<div role="alert" class="rounded-md border border-line p-3 text-sm/6">
						<p class="font-semibold text-danger-text">The project wasn't created</p>
						<p class="mt-1 text-fg-secondary [overflow-wrap:anywhere]">{refusal.message}</p>
					</div>
				{/if}
			</div>
			<!-- Outside the scrolling fields, so Cancel and Create project stay in view. -->
			<div
				data-sheet-footer
				class="flex flex-wrap justify-end gap-3 border-t border-line p-4 max-[639px]:[&>*]:flex-1"
			>
				<Button
					type="button"
					variant="outline"
					disabled={busy}
					class={mobileButton}
					onclick={() => (open = false)}>Cancel</Button
				>
				<Button type="submit" disabled={busy} aria-busy={busy} class="min-w-32 {mobileButton}"
					>{busy ? 'Creating…' : 'Create project'}</Button
				>
			</div>
		</form>
	</SheetContent>
</Sheet>
