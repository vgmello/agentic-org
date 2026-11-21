<!--
  @component
  Hire, Review (our-org-in-the-app §7, STAFF-1, STAFF-7): what will be created, as the office's dry run: the employee
  (name, handle), its optional role, title and focus, the required home team, optional reports to, the separate
  execution profile and model with what the office observed about the runtime, the office and its instance, the
  home folder, and who can reach it. Rows for features that don't exist yet (the cost estimate, How it runs) are left
  out. A runtime that isn't ready warns and the primary becomes Hire anyway. A refusal (no access, team full, handle
  taken) stays inline with the input kept.
-->
<script lang="ts">
	import Callout from '$lib/components/Callout.svelte';
	import DescriptionList, { type DescriptionItem } from '$lib/components/DescriptionList.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/proto';
	import { employeesApi } from '../../api';
	import { runtimeText, runtimeWarning } from '../../hire';
	import type { HireChoices, HireProposal, HireTeam } from '../../types';

	let {
		organizationId,
		organizationName,
		proposal,
		choices,
		teams,
		onback,
		onconfirm
	}: {
		organizationId: string;
		organizationName: string;
		proposal: HireProposal;
		choices: HireChoices;
		teams: HireTeam[];
		onback: () => void;
		/** Confirms the proposal; rejects with HQ's refusal. */
		onconfirm: () => Promise<void>;
	} = $props();

	const preview = new Resource(() => employeesApi.preview(organizationId, proposal));
	let busy = $state(false);
	let refusal = $state<string | undefined>();

	const warning = $derived(
		preview.data
			? runtimeWarning(
					preview.data.driver,
					preview.data.runtime,
					preview.data.office.instance,
					proposal.name
				)
			: undefined
	);

	const rows = $derived.by((): DescriptionItem[] => {
		const p = preview.data;
		if (!p) return [];
		const profile = choices.profiles.find((x) => x.id === proposal.executionProfileId);
		const role = choices.roles.find((r) => r.id === proposal.roleId)?.name ?? proposal.roleText;
		const boss = choices.people.find((m) => m.id === proposal.reportsToId);
		return [
			{ term: 'Name', value: proposal.name },
			{ term: 'Handle', value: proposal.handle, mono: true },
			{ term: 'Role', value: role ?? 'No role' },
			{ term: 'Title', value: proposal.title ?? 'No title' },
			{ term: 'Focus', value: proposal.focus ?? 'No focus' },
			{
				term: 'Home team',
				value: teams.find((t) => t.id === proposal.homeTeamId)?.name ?? proposal.homeTeamId
			},
			{ term: 'Reports to', value: boss?.name ?? 'No one recorded' },
			{
				term: 'Execution',
				value: `${profile?.name ?? proposal.executionProfileId} · ${proposal.model} · ${runtimeText(p.driver, p.runtime, p.office.instance)}`
			},
			{ term: 'Office', value: `${p.office.name} · ${p.office.instance}` },
			{ term: 'Home folder', value: p.homeFolder, mono: true }
		];
	});

	async function confirm() {
		if (busy) return;
		busy = true;
		refusal = undefined;
		try {
			await onconfirm();
		} catch (e) {
			refusal = toAppError(e).message;
			busy = false;
			setTimeout(() => document.getElementById('hire-refusal')?.focus(), 0);
		}
	}
</script>

{#if preview.state === 'loading'}
	<SkeletonRows label="what will be created" rows={6} lines={1} />
{:else if preview.state === 'error' || preview.state === 'unavailable'}
	<ErrorState
		level={4}
		title="Couldn't prepare the review"
		message={preview.message}
		onretry={() => preview.reload()}
		retrying={preview.reloading}
	/>
	<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8">
		<Button variant="outline" class={mobileButton} onclick={onback}>Back</Button>
	</div>
{:else}
	<div class="max-w-3xl space-y-6">
		<p class="px-4 text-sm/6 text-fg-secondary min-[640px]:px-6 min-[1024px]:px-8">
			Nothing has been created yet. Confirming creates exactly this.
		</p>
		{#if warning}
			<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8">
				<Callout>
					<p class="text-sm/6 text-fg">{warning}</p>
				</Callout>
			</div>
		{/if}
		<DescriptionList items={rows} class="border-y border-line-subtle" />
		<p class="px-4 text-sm/6 text-fg-secondary min-[640px]:px-6 min-[1024px]:px-8">
			{proposal.name} can be reached by everyone in {organizationName}.
		</p>
		{#if refusal}
			<p
				id="hire-refusal"
				tabindex="-1"
				role="alert"
				class="mx-4 rounded-md border border-danger/30 px-4 py-3 text-sm/6 text-danger-text outline-none min-[640px]:mx-6 min-[1024px]:mx-8 [overflow-wrap:anywhere]"
			>
				{refusal}
			</p>
		{/if}
		<div
			class="flex flex-wrap items-center gap-3 border-t border-line-subtle px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<Button variant="outline" class={mobileButton} disabled={busy} onclick={onback}>Back</Button>
			<Button class="min-w-32 {mobileButton}" aria-busy={busy} onclick={confirm}
				>{busy ? 'Hiring…' : warning ? 'Hire anyway' : `Hire ${proposal.name}`}</Button
			>
		</div>
	</div>
{/if}
