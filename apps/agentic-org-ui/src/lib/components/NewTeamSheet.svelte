<script lang="ts">
	import { untrack } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { createTeam } from '$lib/org/actions';
	import { byName, kindWord, nonOwners, otherTeamsOf } from '$lib/org/model';
	import type { OrgSession } from '$lib/org/session';
	import { checkTeamName, teamOptions } from '$lib/org/team-edit';
	import type { AppError, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		snapshot,
		session,
		ownerTeamId = undefined,
		ondone
	}: {
		open: boolean;
		snapshot: OrgSnapshot;
		session: OrgSession;
		/** The team to prefill "Part of" with (Add team under…). */
		ownerTeamId?: string;
		/** The team was created; `problems` lists anything that didn't go through (members, lead). */
		ondone: (result: { teamId: string; name: string; problems: string[] }) => void;
	} = $props();

	let name = $state('');
	let owner = $state('');
	let search = $state('');
	const chosen = new SvelteSet<string>();
	let lead = $state('');
	let nameError = $state<string | undefined>();
	let failure = $state<AppError | undefined>();
	let busy = $state(false);

	const candidates = $derived(nonOwners(snapshot).toSorted(byName));
	const shown = $derived(candidates.filter((m) => m.name.toLowerCase().includes(search.trim().toLowerCase())));
	const leadChoices = $derived(candidates.filter((m) => chosen.has(m.id)));
	const parents = $derived(teamOptions(snapshot));

	// Each time the sheet opens it starts empty (only `open` triggers this: the chart reloading behind it must not).
	$effect(() => {
		if (open) {
			untrack(() => {
				name = '';
				owner = ownerTeamId ?? '';
				search = '';
				chosen.clear();
				lead = '';
				nameError = undefined;
				failure = undefined;
			});
		}
	});

	function toggle(id: string, on: boolean) {
		if (on) chosen.add(id);
		else {
			chosen.delete(id);
			if (lead === id) lead = '';
		}
	}

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (busy) return;
		const checked = checkTeamName(snapshot, name, owner || undefined);
		if (!checked.ok) {
			nameError = checked.message;
			return;
		}
		busy = true;
		failure = undefined;
		const nameOf = (id: string) => candidates.find((m) => m.id === id)?.name ?? id;
		try {
			const result = await createTeam(session, { name: checked.name, ownerTeamId: owner || undefined, memberIds: [...chosen], leadId: lead || undefined }, nameOf);
			if (result.created.error) {
				const error = result.created.error;
				// HQ's refusal of the name is fixed in the field; anything else is shown above the buttons.
				if (error.code === 'invalid_name' || error.code === 'already_exists') nameError = error.message;
				else failure = error;
				return;
			}
			open = false;
			ondone({
				teamId: result.teamId,
				name: checked.name,
				problems: result.steps.filter((s) => s.error).map((s) => `Couldn't ${s.what.charAt(0).toLowerCase()}${s.what.slice(1)}: ${s.error!.message}`)
			});
		} catch (error) {
			failure = toAppError(error);
		} finally {
			busy = false;
		}
	}
</script>

<!-- A click outside or Escape doesn't dismiss the sheet while the team is being created. -->
<Sheet bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<SheetContent side="right" class="w-full max-w-md overflow-y-auto">
		<SheetHeader>
			<SheetTitle>New team</SheetTitle>
			<SheetDescription>A team groups agents and people. It can be part of another team.</SheetDescription>
		</SheetHeader>

		<form class="flex flex-1 flex-col gap-y-5 px-4" onsubmit={submit} novalidate>
			<div class="space-y-2">
				<Label for="team-name">Name</Label>
				<Input
					id="team-name"
					bind:value={name}
					disabled={busy}
					autocomplete="off"
					aria-invalid={nameError ? true : undefined}
					aria-describedby={nameError ? 'team-name-error' : undefined}
					oninput={() => (nameError = undefined)}
				/>
				{#if nameError}<p id="team-name-error" class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{nameError}</p>{/if}
			</div>

			<div class="space-y-2">
				<Label for="team-owner">Part of</Label>
				<select id="team-owner" bind:value={owner} disabled={busy} onchange={() => (nameError = undefined)} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					<option value="">None: top level</option>
					{#each parents as option (option.team.id)}<option value={option.team.id}>{option.path}</option>{/each}
				</select>
			</div>

			<fieldset class="space-y-2" disabled={busy}>
				<legend class="text-sm/6 font-medium text-white">Members <span class="font-normal text-gray-400">(optional)</span></legend>
				<Input type="search" bind:value={search} placeholder="Search agents and people…" aria-label="Search agents and people" autocomplete="off" />
				<ul class="max-h-56 divide-y divide-white/5 overflow-y-auto rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Agents and people">
					{#each shown as m (m.id)}
						{@const also = otherTeamsOf(snapshot, m.id, '')}
						<li>
							<label class="flex cursor-pointer items-start gap-x-3 px-3 py-2 text-sm/6">
								<Checkbox class="mt-1" checked={chosen.has(m.id)} onCheckedChange={(on) => toggle(m.id, on === true)} aria-label="{m.name}, {kindWord(m.kind).toLowerCase()}" />
								<span class="min-w-0">
									<span class="block text-white [overflow-wrap:anywhere]">{m.name} <span class="text-xs text-gray-400">{kindWord(m.kind)}</span></span>
									{#if also.length > 0}<span class="block text-xs/5 text-gray-400 [overflow-wrap:anywhere]">Also in: {also.map((t) => t.name).join(', ')}</span>{/if}
								</span>
							</label>
						</li>
					{:else}
						<li class="px-3 py-2 text-sm/6 text-gray-400">{search ? 'No one matches.' : 'No agents or people yet.'}</li>
					{/each}
				</ul>
			</fieldset>

			<div class="space-y-2">
				<Label for="team-lead">Lead</Label>
				<select id="team-lead" bind:value={lead} disabled={busy || leadChoices.length === 0} aria-describedby="team-lead-help" class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400 disabled:opacity-60">
					<option value="">No lead</option>
					{#each leadChoices as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
				</select>
				<p id="team-lead-help" class="text-xs/5 text-gray-400">
					{leadChoices.length === 0 ? 'Choose members first: the lead is one of them. ' : ''}The lead also leads the team's projects.
				</p>
			</div>

			{#if failure}
				<Alert variant="destructive">
					<CircleAlert aria-hidden="true" />
					<AlertTitle class="[overflow-wrap:anywhere]">Couldn't create the team</AlertTitle>
					<AlertDescription class="[overflow-wrap:anywhere]">{failure.message}</AlertDescription>
				</Alert>
			{/if}

			<SheetFooter class="mt-auto px-0">
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy}>Create team</Button>
			</SheetFooter>
		</form>
	</SheetContent>
</Sheet>
