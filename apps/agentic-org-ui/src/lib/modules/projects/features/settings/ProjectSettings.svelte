<!--
  @component
  A project's Settings tab (work-items-ux §3, PRJ-3, offices-and-teams-ux §7.9, development-processes §3): four
  sections, each saved on its own — Name and slug; Teams (the accountable team, required and never cleared, whose
  change is previewed and confirmed because it moves every project task's accountable team; and the contributing
  teams); Repositories, each association with its own required integration branch; and the Development process,
  with what choosing it adds. Viewers without the right, or while HQ can't be reached, see the values read-only and
  why (§0.1, PRJ-4). Refusals show on their field and focus it; a newer revision keeps what was typed.
-->
<script lang="ts">
	import { tick, untrack } from 'svelte';
	import Callout from '$lib/components/Callout.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		selectClass
	} from '$lib/components/classes';
	import { neighbourFocus } from '$lib/components/focus';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { Resource } from '$lib/components/resource.svelte';
	import { projectsApi } from '../../api';
	import {
		CHANGES_PAUSED,
		VIEW_ONLY,
		branchProblem,
		nameProblem,
		slugProblem
	} from '../../present';
	import type { TeamChoice } from '../../types';
	import ProcessSelect from '../../components/ProcessSelect.svelte';
	import { projectFrame } from '../frame/context';
	import { SectionSave } from './save.svelte';
	import SettingsSection from './SettingsSection.svelte';

	const frame = projectFrame();
	const p = $derived(frame.project);
	const org = $derived(frame.organizationId);
	const locked = $derived(!p.viewer.canEdit || !!p.stale);
	const reload = () => frame.reload();

	// What the fields choose from: read once per frame (the route keys the frame by organization and project).
	const choices = new Resource(() =>
		Promise.all([
			projectsApi.teamChoices(frame.organizationId),
			projectsApi.repositoryChoices(frame.organizationId)
		])
	);
	const teams = $derived(choices.data?.[0]);
	const registered = $derived(choices.data?.[1]);
	const choicesError = $derived(choices.state === 'error' ? choices.message : undefined);
	const teamLabel = (t: TeamChoice) => `${t.path}${t.ref ? ` · ${t.ref}` : ''}`;

	// ── Name and slug ────────────────────────────────────────────────────────────────────────────────────────────
	const nameSave = new SectionSave(reload);
	let name = $state(untrack(() => p.name));
	let slug = $state(untrack(() => p.slug));
	let nameTouched = $state(false);
	const nameError = $derived(
		nameSave.refusal?.field === 'name'
			? nameSave.refusal.message
			: nameTouched
				? nameProblem(name)
				: undefined
	);
	const slugError = $derived(
		nameSave.refusal?.field === 'slug'
			? nameSave.refusal.message
			: nameTouched
				? slugProblem(slug)
				: undefined
	);
	const nameDirty = $derived(name.trim() !== p.name || slug.trim() !== p.slug);

	async function saveName(form: HTMLFormElement) {
		nameTouched = true;
		if (nameProblem(name) || slugProblem(slug)) {
			await tick();
			form.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
			return;
		}
		const ok = await nameSave.run(
			() =>
				projectsApi.rename(org, p.id, {
					name: name.trim(),
					slug: slug.trim(),
					expectedRevision: p.revision
				}),
			p.name
		);
		if (ok) {
			name = p.name;
			slug = p.slug;
			nameTouched = false;
		} else if (nameSave.refusal?.field) {
			await tick();
			form.querySelector<HTMLElement>(`#settings-${nameSave.refusal.field}`)?.focus();
		}
	}

	// ── Teams ───────────────────────────────────────────────────────────────────────────────────────────────────
	const teamsSave = new SectionSave(reload);
	let ownerId = $state(untrack(() => p.ownerTeam.id));
	let contributorIds = $state<string[]>(untrack(() => p.contributors.map((c) => c.id)));
	const ownerChanged = $derived(ownerId !== p.ownerTeam.id);
	const sameSet = (a: string[], b: string[]) =>
		a.length === b.length && a.every((x) => b.includes(x));
	const teamsDirty = $derived(
		ownerChanged ||
			!sameSet(
				contributorIds,
				p.contributors.map((c) => c.id)
			)
	);
	let confirmOwner = $state(false);
	let ownerPreview = $state<{ tasks: number } | undefined>();
	let ownerPreviewError = $state<string>();
	const newOwner = $derived(teams?.find((t) => t.id === ownerId));

	async function saveTeams() {
		if (ownerChanged) {
			// Changing the accountable team moves every task's accountability: say how many before anything happens.
			ownerPreview = undefined;
			ownerPreviewError = undefined;
			confirmOwner = true;
			try {
				ownerPreview = await projectsApi.ownerChangePreview(org, p.id, ownerId);
			} catch (e) {
				ownerPreviewError = toAppError(e).message;
			}
			return;
		}
		await commitTeams();
	}

	async function commitTeams() {
		const contributors = contributorIds.filter((id) => id !== ownerId);
		const ok = await teamsSave.run(async () => {
			if (ownerChanged)
				await projectsApi.setOwner(org, p.id, { teamId: ownerId, expectedRevision: p.revision });
			const after = ownerChanged ? p.revision + 1 : p.revision;
			const current = p.contributors.map((c) => c.id).filter((id) => id !== ownerId);
			if (!sameSet(contributors, current))
				await projectsApi.setContributors(org, p.id, {
					teamIds: contributors,
					expectedRevision: after
				});
		}, p.name);
		if (ok) {
			ownerId = p.ownerTeam.id;
			contributorIds = p.contributors.map((c) => c.id);
		}
		return ok;
	}

	/** The confirmation stays open with the reason when HQ refuses; what was chosen is kept. */
	async function confirmTeams() {
		if (await commitTeams()) return;
		// Said once, in the dialog that is still open, not again in the section behind it.
		const message = teamsSave.refusal?.message ?? "The accountable team wasn't changed.";
		teamsSave.refusal = undefined;
		throw new Error(message);
	}

	// ── Repositories ────────────────────────────────────────────────────────────────────────────────────────────
	const repoSave = new SectionSave(reload);
	type Association = { repositoryId: string; integrationBranch: string };
	const fromProject = () =>
		p.repositories.map((r) => ({
			repositoryId: r.repositoryId,
			integrationBranch: r.integrationBranch
		}));
	let associations = $state<Association[]>(untrack(fromProject));
	let adding = $state('');
	let repoTouched = $state(false);
	const repoDirty = $derived(
		JSON.stringify(associations.map((a) => [a.repositoryId, a.integrationBranch.trim()])) !==
			JSON.stringify(fromProject().map((a) => [a.repositoryId, a.integrationBranch]))
	);
	const registration = (id: string) => registered?.find((r) => r.id === id);
	const unassociated = $derived(
		(registered ?? []).filter((r) => !associations.some((a) => a.repositoryId === r.id))
	);

	function addRepository() {
		const r = registration(adding);
		if (!r) return;
		associations.push({ repositoryId: r.id, integrationBranch: r.defaultBranch });
		adding = '';
		repoSave.edited();
		void tick().then(() => document.getElementById(`settings-branch-${r.id}`)?.focus());
	}

	function removeRepository(event: MouseEvent, id: string) {
		const focusNext = neighbourFocus(event.currentTarget as HTMLElement, () =>
			document.getElementById('settings-add-repository')
		);
		associations = associations.filter((a) => a.repositoryId !== id);
		repoSave.edited();
		void focusNext();
	}

	async function saveRepositories(form: HTMLFormElement) {
		repoTouched = true;
		if (associations.some((a) => branchProblem(a.integrationBranch))) {
			await tick();
			form.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
			return;
		}
		const ok = await repoSave.run(
			() =>
				projectsApi.setRepositories(org, p.id, {
					repositories: associations.map((a) => ({
						repositoryId: a.repositoryId,
						integrationBranch: a.integrationBranch.trim()
					})),
					expectedRevision: p.revision
				}),
			p.name
		);
		if (ok) {
			associations = fromProject();
			repoTouched = false;
		}
	}

	// ── Development process ─────────────────────────────────────────────────────────────────────────────────────
	const processSave = new SectionSave(reload);
	let processId = $state(untrack(() => p.developmentProcess?.id ?? ''));
	const processDirty = $derived(processId !== (p.developmentProcess?.id ?? ''));

	async function saveProcess() {
		const ok = await processSave.run(
			() =>
				projectsApi.setProcess(org, p.id, {
					processId: processId || null,
					expectedRevision: p.revision
				}),
			p.name
		);
		if (ok) processId = p.developmentProcess?.id ?? '';
	}
</script>

{#if locked}
	<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
		<Callout data-testid="settings-locked">
			<p class="text-sm/6">{p.stale ? CHANGES_PAUSED : VIEW_ONLY}</p>
		</Callout>
	</div>
{/if}
{#if choicesError}
	<p class="px-4 pt-6 text-sm/6 text-danger-text min-[640px]:px-6 min-[1024px]:px-8" role="alert">
		Couldn't load the teams and repositories to choose from: {choicesError}
	</p>
{/if}

<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8">
	<SettingsSection
		id="settings-name"
		title="Name and slug"
		description="The name people see, and the slug that identifies the project in links and paths. {p.name} is P-{p.number} whatever its name."
		save={nameSave}
		{locked}
		dirty={nameDirty}
		onsubmit={saveName}
	>
		<div>
			<Label for="settings-name">Name</Label>
			<Input
				id="settings-name"
				bind:value={name}
				autocomplete="off"
				class="mt-2 {inputClass}"
				aria-invalid={nameError ? true : undefined}
				aria-describedby={nameError ? 'settings-name-error' : undefined}
				oninput={() => nameSave.edited()}
			/>
			{#if nameError}<p id="settings-name-error" class={errorClass}>{nameError}</p>{/if}
		</div>
		<div>
			<Label for="settings-slug">Slug</Label>
			<Input
				id="settings-slug"
				bind:value={slug}
				autocomplete="off"
				autocapitalize="none"
				spellcheck={false}
				class="mt-2 font-mono {inputClass}"
				aria-invalid={slugError ? true : undefined}
				aria-describedby={slugError ? 'settings-slug-error' : 'settings-slug-help'}
				oninput={() => nameSave.edited()}
			/>
			{#if slugError}
				<p id="settings-slug-error" class="{errorClass} [overflow-wrap:anywhere]">{slugError}</p>
			{:else}
				<p id="settings-slug-help" class={helpClass}>
					Unique in the organization. Changing it changes the project's links; its ID and P-{p.number}
					stay.
				</p>
			{/if}
		</div>
	</SettingsSection>

	<SettingsSection
		id="settings-teams"
		title="Teams"
		description="One accountable team owns the project and its tasks; it can be replaced, never removed. Contributing teams take part without owning it."
		save={teamsSave}
		{locked}
		dirty={teamsDirty}
		onsubmit={saveTeams}
	>
		<div>
			<Label for="settings-owner">Accountable team</Label>
			<select
				id="settings-owner"
				bind:value={ownerId}
				onchange={() => {
					contributorIds = contributorIds.filter((id) => id !== ownerId);
					teamsSave.edited();
				}}
				aria-describedby="settings-owner-help"
				class="mt-2 {selectClass}"
			>
				{#if !teams}<option value={p.ownerTeam.id}>{p.ownerTeam.name}</option>{/if}
				{#each teams ?? [] as t (t.id)}<option value={t.id}>{teamLabel(t)}</option>{/each}
			</select>
			<p id="settings-owner-help" class={helpClass}>
				Changing it moves every task of {p.name} to the new team's accountability, with history. Who each
				task is assigned to stays the same.
			</p>
		</div>
		<fieldset>
			<legend class="text-sm/6 font-medium text-fg">Contributing teams</legend>
			{#if !teams}
				<p class="mt-2 text-sm/6 text-fg-muted">Loading teams…</p>
			{:else}
				<div class="mt-2 space-y-1">
					{#each teams.filter((t) => t.id !== ownerId) as t (t.id)}
						<label class="flex items-start gap-x-3 py-1 text-sm/6 max-[639px]:min-h-11">
							<input
								type="checkbox"
								value={t.id}
								bind:group={contributorIds}
								onchange={() => teamsSave.edited()}
								class="mt-1 size-4 shrink-0 accent-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
							/>
							<span class="min-w-0 text-fg-secondary [overflow-wrap:anywhere]">{teamLabel(t)}</span>
						</label>
					{/each}
				</div>
				<p class={helpClass}>Adding or removing a contributor changes no task or assignment.</p>
			{/if}
		</fieldset>
	</SettingsSection>

	<SettingsSection
		id="settings-repositories"
		title="Repositories"
		description="The code repositories the project's tasks work in. Each has its own integration branch for this project; a project needs none to keep docs."
		save={repoSave}
		{locked}
		dirty={repoDirty}
		onsubmit={saveRepositories}
	>
		{#if associations.length === 0}
			<p class="text-sm/6 text-fg-muted" data-testid="no-repositories">No repositories yet.</p>
		{:else}
			<ul role="list" class="divide-y divide-line-subtle border-y border-line-subtle">
				{#each associations as a (a.repositoryId)}
					{@const r =
						registration(a.repositoryId) ??
						p.repositories.find((x) => x.repositoryId === a.repositoryId)}
					{@const error = repoTouched ? branchProblem(a.integrationBranch) : undefined}
					<li class="py-4" data-repository={a.repositoryId}>
						<div class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
							<div class="min-w-0">
								<p class="font-mono text-sm/6 text-fg [overflow-wrap:anywhere]">
									{r?.key ?? a.repositoryId}
								</p>
								<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
									{r?.kind === 'local' ? 'Local only: no remote' : (r?.remoteUrl ?? '')}
								</p>
							</div>
							<Button
								type="button"
								variant="ghost"
								class="text-fg-secondary {mobileButton}"
								aria-label="Remove {r?.key ?? 'repository'}"
								onclick={(e: MouseEvent) => removeRepository(e, a.repositoryId)}>Remove</Button
							>
						</div>
						<Label for="settings-branch-{a.repositoryId}" class="mt-3">Integration branch</Label>
						<Input
							id="settings-branch-{a.repositoryId}"
							bind:value={a.integrationBranch}
							autocomplete="off"
							autocapitalize="none"
							spellcheck={false}
							class="mt-2 font-mono {inputClass}"
							aria-invalid={error ? true : undefined}
							aria-describedby={error ? `settings-branch-${a.repositoryId}-error` : undefined}
							oninput={() => repoSave.edited()}
						/>
						{#if error}<p id="settings-branch-{a.repositoryId}-error" class={errorClass}>
								{error}
							</p>{/if}
					</li>
				{/each}
			</ul>
		{/if}
		{#if repoSave.refusal?.field === 'repositories'}
			<p class={errorClass} role="alert">{repoSave.refusal.message}</p>
		{/if}
		<div class="flex flex-wrap items-end gap-3">
			<div class="min-w-0 flex-1 basis-56">
				<Label for="settings-add-repository">Add a repository</Label>
				<select
					id="settings-add-repository"
					bind:value={adding}
					class="mt-2 {selectClass}"
					aria-describedby="settings-add-help"
				>
					<option value="">{registered ? 'Choose a registered repository' : 'Loading…'}</option>
					{#each unassociated as r (r.id)}<option value={r.id}>{r.key}</option>{/each}
				</select>
			</div>
			<Button
				type="button"
				variant="outline"
				class={mobileButton}
				disabled={!adding}
				aria-describedby="settings-add-help"
				onclick={addRepository}>Add</Button
			>
			<p id="settings-add-help" class="w-full text-xs/5 text-fg-muted">
				{unassociated.length || !registered
					? 'Choose one, then Add. It is saved with Save repositories.'
					: 'Every registered repository is already here.'}
			</p>
		</div>
	</SettingsSection>

	<SettingsSection
		id="settings-process"
		title="Development process"
		description="How {p.name} builds software: its roles, skills and the tabs its pages show. Changing it removes no file, document or role."
		save={processSave}
		{locked}
		dirty={processDirty}
		onsubmit={saveProcess}
	>
		<ProcessSelect
			id="settings-process-select"
			organizationId={org}
			projectId={p.id}
			bind:value={processId}
			current={p.developmentProcess?.id ?? ''}
			disabled={locked}
			error={processSave.refusal?.field === 'process' ? processSave.refusal.message : undefined}
		/>
	</SettingsSection>
</div>

<ConfirmDialog
	bind:open={confirmOwner}
	title="Make {newOwner?.name ?? 'this team'} accountable for {p.name}?"
	confirmLabel="Change accountable team"
	busyLabel="Changing…"
	onconfirm={confirmTeams}
>
	{#snippet body()}
		<ul
			role="list"
			class="list-disc space-y-1 pl-5 text-sm/6 text-fg-secondary marker:text-fg-muted"
		>
			<li data-testid="owner-preview">
				{#if ownerPreviewError}
					Couldn't count the tasks that move: {ownerPreviewError}
				{:else if !ownerPreview}
					Counting the tasks that move…
				{:else if ownerPreview.tasks === 0}
					{p.name} has no tasks yet, so none move.
				{:else}
					{ownerPreview.tasks}
					{ownerPreview.tasks === 1 ? 'task moves its' : 'tasks move their'} accountable team from {p
						.ownerTeam.name} to {newOwner?.name}, with history.
				{/if}
			</li>
			<li>Assigned teams and assignees stay as they are.</li>
			<li>
				{p.ownerTeam.name}
				{contributorIds.includes(p.ownerTeam.id) ? 'stays as a contributor' : 'stops taking part'}.
			</li>
		</ul>
	{/snippet}
</ConfirmDialog>
