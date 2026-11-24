<!--
  @component
  Hire, Suggestion as a form (our-org-in-the-app §7): the HR lead's proposal to check and change, or, when the hirer
  fills in the details themselves, the same form with name and handle empty and marked Your choice needed, no HR
  reason line and no invented defaults. Identity (name, handle, optional role, title and focus), place (home team,
  optional reports to; the HR lead's suggestion is applied only when the hirer chooses it) and the separately chosen
  execution profile, model and office, with what the office observed about the runtime there. Next stays disabled
  until the required fields are valid, and says what is missing.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
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
	import { suggestMessagingName } from '$lib/modules/organization/agents';
	import {
		handleProblem,
		isFull,
		missingFields,
		missingReason,
		runtimeText,
		teamFullText,
		teamsInTreeOrder
	} from '../../hire';
	import type { HireChoices, HireProposal, HireRequest, HireTeam } from '../../types';

	let {
		fields = $bindable(),
		choices,
		teams,
		manual,
		reason,
		request,
		onback,
		onnext
	}: {
		fields: Partial<HireProposal>;
		choices: HireChoices;
		teams: HireTeam[];
		/** Filled in by the hirer: no HR proposal, reason or suggestion. */
		manual: boolean;
		/** What was typed in Describe, kept above the form as a note. */
		reason: string;
		/** The drafting request, with the HR lead's reason and suggested manager. */
		request?: HireRequest;
		onback: () => void;
		onnext: () => void;
	} = $props();

	// In a form filled in by hand the handle follows the name until it is edited.
	let handleEdited = $state(!!fields.handle);

	const team = $derived(teams.find((t) => t.id === fields.homeTeamId));
	const full = $derived(team && isFull(team) ? teamFullText(team) : undefined);
	const handleError = $derived(handleProblem(fields.handle ?? '', choices.takenHandles));
	const profile = $derived(choices.profiles.find((p) => p.id === fields.executionProfileId));
	const office = $derived(choices.offices.find((o) => o.id === fields.officeId));
	const runtime = $derived(
		profile && office
			? {
					check: profile.readiness[office.id] ?? 'unknown',
					text: runtimeText(
						profile.driver,
						profile.readiness[office.id] ?? 'unknown',
						office.instance
					)
				}
			: undefined
	);
	const suggested = $derived(manual ? undefined : request?.suggestedReportsTo);
	const missing = $derived(missingFields(fields));
	const blocked = $derived(
		missingReason(missing) ??
			(handleError ? 'Choose another handle to continue' : undefined) ??
			(full ? 'Choose a team with room to continue' : undefined)
	);
	const needed = (v: string | undefined) => manual && !(v ?? '').trim();

	function nameInput() {
		if (!handleEdited) fields.handle = suggestMessagingName(fields.name ?? '');
	}

	function chooseProfile() {
		if (!profile?.models.includes(fields.model ?? '')) fields.model = '';
	}

	function next() {
		if (!blocked) onnext();
	}
</script>

{#snippet yourChoice(show: boolean)}
	{#if show}
		<span
			class="ml-2 inline-flex items-center rounded-full bg-brand/10 px-2 text-xs/5 font-medium text-brand-text ring-1 ring-brand/30 ring-inset"
			>Your choice needed</span
		>
	{/if}
{/snippet}

{#snippet optional()}
	<span class="ml-1 text-xs/5 font-normal text-fg-muted">Optional</span>
{/snippet}

<div class="max-w-2xl space-y-8">
	{#if manual}
		{#if reason.trim()}
			<figure class="rounded-md border border-line px-4 py-3">
				<figcaption class="text-xs/5 text-fg-muted">You asked for</figcaption>
				<blockquote class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
					{reason}
				</blockquote>
			</figure>
		{/if}
	{:else if request?.hrReason}
		<figure class="rounded-md border border-line px-4 py-3">
			<figcaption class="text-xs/5 text-fg-muted">The HR lead's reasons</figcaption>
			<blockquote class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
				{request.hrReason}
			</blockquote>
			{#if request.reason}
				<p class="mt-2 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
					You asked for: {request.reason}
				</p>
			{/if}
		</figure>
	{/if}

	<fieldset class="space-y-6">
		<legend class="float-left mb-4 w-full text-base/7 font-semibold text-fg">Who</legend>
		<div class="clear-left mt-0!">
			<Label for="hire-name" class="text-sm/6 font-medium text-fg"
				>Name{@render yourChoice(needed(fields.name))}</Label
			>
			<Input
				id="hire-name"
				class="mt-2 {inputClass}"
				autocomplete="off"
				bind:value={fields.name}
				oninput={nameInput}
			/>
		</div>
		<div>
			<Label for="hire-handle" class="text-sm/6 font-medium text-fg"
				>Handle{@render yourChoice(needed(fields.handle))}</Label
			>
			<Input
				id="hire-handle"
				class="mt-2 font-mono {inputClass}"
				autocomplete="off"
				spellcheck={false}
				aria-invalid={handleError ? true : undefined}
				aria-describedby="hire-handle-help"
				bind:value={fields.handle}
				oninput={() => (handleEdited = true)}
			/>
			<p id="hire-handle-help" class={handleError ? errorClass : helpClass}>
				{handleError ??
					'How colleagues message it, for example qa-engineer-marlowe. Letters, digits, - _ or ., unique in the organization.'}
			</p>
		</div>
		<div>
			<Label for="hire-role" class="text-sm/6 font-medium text-fg">Role{@render optional()}</Label>
			<select
				id="hire-role"
				class="mt-2 {selectClass}"
				aria-describedby="hire-role-help"
				bind:value={fields.roleId}
			>
				<option value={undefined}>No role</option>
				{#each choices.roles as r (r.id)}<option value={r.id}>{r.name}</option>{/each}
			</select>
			<p id="hire-role-help" class={helpClass}>
				An existing role; its instructions apply to the agent.
			</p>
		</div>
		<div>
			<Label for="hire-title" class="text-sm/6 font-medium text-fg">Title{@render optional()}</Label
			>
			<Input
				id="hire-title"
				class="mt-2 {inputClass}"
				aria-describedby="hire-title-help"
				bind:value={fields.title}
			/>
			<p id="hire-title-help" class={helpClass}>
				With its level, for example QA engineer (senior).
			</p>
		</div>
		<div>
			<Label for="hire-focus" class="text-sm/6 font-medium text-fg">Focus{@render optional()}</Label
			>
			<Input id="hire-focus" class="mt-2 {inputClass}" bind:value={fields.focus} />
		</div>
	</fieldset>

	<fieldset class="space-y-6 border-t border-line-subtle pt-6">
		<legend class="float-left mb-4 w-full text-base/7 font-semibold text-fg">Where</legend>
		<div class="clear-left mt-0!">
			<Label for="hire-home" class="text-sm/6 font-medium text-fg">Home team</Label>
			<select
				id="hire-home"
				class="mt-2 {selectClass}"
				aria-invalid={full ? true : undefined}
				aria-describedby="hire-home-help"
				bind:value={fields.homeTeamId}
			>
				{#if !team}<option value="" disabled>Choose a team</option>{/if}
				{#each teamsInTreeOrder(teams) as { team: t, depth } (t.id)}
					<option value={t.id}
						>{'\u00a0\u00a0'.repeat(depth)}{t.name} · {t.members} of {t.maxMembers} members</option
					>
				{/each}
			</select>
			{#if full && team}
				<p id="hire-home-help" class={errorClass}>
					{full}
					<a
						href={resolve(`/teams/${encodeURIComponent(team.id)}` as '/')}
						class="rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
						>Open {team.name}</a
					>
				</p>
			{:else}
				<p id="hire-home-help" class={helpClass}>Only teams you may hire into are listed.</p>
			{/if}
		</div>
		<div>
			<Label for="hire-reports" class="text-sm/6 font-medium text-fg"
				>Reports to{@render optional()}</Label
			>
			<select
				id="hire-reports"
				class="mt-2 {selectClass}"
				aria-describedby="hire-reports-help"
				bind:value={fields.reportsToId}
			>
				<option value={undefined}>No one recorded</option>
				{#each choices.people as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
			</select>
			<div id="hire-reports-help" class="{helpClass} flex flex-wrap items-center gap-x-3 gap-y-1">
				{#if suggested && fields.reportsToId !== suggested.id}
					<span>The HR lead suggests {suggested.name}. It isn't recorded until you choose it.</span>
					<button
						type="button"
						class="inline-flex min-h-6 items-center rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
						onclick={() => (fields.reportsToId = suggested.id)}>Use {suggested.name}</button
					>
				{:else}
					<span>An explicit manager, separate from the team's lead.</span>
				{/if}
			</div>
		</div>
	</fieldset>

	<fieldset class="space-y-6 border-t border-line-subtle pt-6">
		<legend class="float-left mb-4 w-full text-base/7 font-semibold text-fg">How it runs</legend>
		<p class="clear-left mt-0! text-sm/6 text-fg-muted">
			Chosen apart from who the agent is: changing these later doesn't change its identity.
		</p>
		<div>
			<Label for="hire-profile" class="text-sm/6 font-medium text-fg">Execution profile</Label>
			<select
				id="hire-profile"
				class="mt-2 {selectClass}"
				bind:value={fields.executionProfileId}
				onchange={chooseProfile}
			>
				{#if !profile}<option value="" disabled>Choose an execution profile</option>{/if}
				{#each choices.profiles as p (p.id)}<option value={p.id}>{p.name} · {p.driver}</option
					>{/each}
			</select>
		</div>
		<div>
			<Label for="hire-model" class="text-sm/6 font-medium text-fg">Model</Label>
			<select
				id="hire-model"
				class="mt-2 {selectClass} disabled:cursor-not-allowed disabled:text-fg-muted disabled:outline-line"
				disabled={!profile}
				aria-describedby="hire-model-help"
				bind:value={fields.model}
			>
				{#if !fields.model}<option value="" disabled>Choose a model</option>{/if}
				{#each profile?.models ?? [] as m (m)}<option value={m}>{m}</option>{/each}
			</select>
			{#if !profile}
				<p id="hire-model-help" class={helpClass}>Choose an execution profile first.</p>
			{/if}
		</div>
		<div>
			<Label for="hire-office" class="text-sm/6 font-medium text-fg">Office</Label>
			<select
				id="hire-office"
				class="mt-2 {selectClass}"
				aria-describedby="hire-office-help"
				bind:value={fields.officeId}
			>
				{#if !office}<option value="" disabled>Choose an office</option>{/if}
				{#each choices.offices as o (o.id)}<option value={o.id}>{o.name} · {o.instance}</option
					>{/each}
			</select>
			<p id="hire-office-help" class={helpClass}>
				{#if runtime}
					<span class={runtime.check === 'ready' ? 'text-success-text' : 'text-fg-secondary'}
						>{runtime.text}.</span
					>
					{runtime.check === 'ready' ? '' : 'You can still hire; Review says what it means.'}
				{:else}
					Where the office runs it. The office checks the runtime there.
				{/if}
			</p>
		</div>
	</fieldset>

	<div class="flex flex-wrap items-center gap-x-4 gap-y-3 border-t border-line-subtle pt-6">
		<Button variant="outline" class={mobileButton} onclick={onback}>Back</Button>
		<Button
			class="min-w-24 {mobileButton}"
			disabled={!!blocked}
			aria-describedby={blocked ? 'hire-proposal-blocked' : undefined}
			onclick={next}>Next</Button
		>
		{#if blocked}
			<p
				id="hire-proposal-blocked"
				class="basis-full text-xs/5 text-fg-muted min-[640px]:basis-auto"
			>
				{blocked}
			</p>
		{/if}
	</div>
</div>
