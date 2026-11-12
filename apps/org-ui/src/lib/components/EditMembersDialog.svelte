<script lang="ts">
	import { untrack } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { setTeamLead, setTeamMembers, wouldClearLead } from '$lib/org/actions';
	import { byName, kindWord, leadOf, membersOfTeam, nonOwners, otherTeamsOf } from '$lib/org/model';
	import type { OrgSession } from '$lib/org/session';
	import type { OrgSnapshot, Team } from '$lib/proto';

	let {
		open = $bindable(false),
		team,
		snapshot,
		session,
		ondone
	}: {
		open: boolean;
		team: Team | undefined;
		snapshot: OrgSnapshot;
		session: OrgSession;
		/** Saved, or partly saved (`problems` says what didn't go through): reload either way. */
		ondone: (result: { name: string; changed: boolean; problems: string[] }) => void;
	} = $props();

	const chosen = new SvelteSet<string>();
	let lead = $state('');
	let search = $state('');
	let busy = $state(false);
	let problems = $state<string[]>([]);
	// Kept while the dialog closes, so its title doesn't flash empty.
	let shown = $state<Team | undefined>();

	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!team) return;
				shown = team;
				chosen.clear();
				membersOfTeam(snapshot, team.id).forEach((m) => chosen.add(m.id));
				lead = leadOf(snapshot, team)?.id ?? '';
				search = '';
				problems = [];
			});
		}
	});

	const candidates = $derived(nonOwners(snapshot).toSorted(byName));
	const shownCandidates = $derived(candidates.filter((m) => m.name.toLowerCase().includes(search.trim().toLowerCase())));
	const leadChoices = $derived(candidates.filter((m) => chosen.has(m.id)));
	const nameOf = (id: string) => candidates.find((m) => m.id === id)?.name ?? id;
	// TEAM-6: unchecking the lead clears the lead; say so before it happens.
	const clearsLead = $derived(!!shown && wouldClearLead(snapshot, shown, [...chosen]));
	const leaderName = $derived(shown?.lead_member_id ? nameOf(shown.lead_member_id) : '');

	function toggle(id: string, on: boolean) {
		if (on) chosen.add(id);
		else {
			chosen.delete(id);
			if (lead === id) lead = '';
		}
	}

	async function save() {
		if (!shown || busy) return;
		busy = true;
		problems = [];
		const team = shown;
		const desired = [...chosen];
		try {
			const { steps } = await setTeamMembers(session, team.id, desired, nameOf);
			const failed = steps.filter((s) => s.error);
			const current = leadOf({ ...snapshot, ...(await session.snapshot()) }, team)?.id ?? '';
			const wanted = lead && chosen.has(lead) ? lead : '';
			if (wanted !== current) {
				try {
					await setTeamLead(session, team.id, wanted || undefined);
				} catch (e) {
					failed.push({ what: wanted ? `Set ${nameOf(wanted)} as the lead` : 'Clear the lead', error: toAppError(e) });
				}
			}
			const messages = failed.map((s) => `Couldn't ${s.what.charAt(0).toLowerCase()}${s.what.slice(1)}: ${s.error!.message}`);
			if (messages.length === 0) {
				open = false;
				ondone({ name: team.name, changed: steps.length > 0 || wanted !== current, problems: [] });
			} else {
				// The dialog stays open, names what failed and keeps the rest (spec §7.6).
				problems = messages;
				ondone({ name: team.name, changed: true, problems: messages });
			}
		} catch (e) {
			problems = [toAppError(e).message];
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent class="sm:max-w-lg">
		<DialogHeader>
			<DialogTitle text={`Members of ${shown?.name ?? ''}`} />
			<DialogDescription>Check everyone who should be in the team. Someone can be in any number of teams.</DialogDescription>
		</DialogHeader>

		<fieldset class="space-y-2" disabled={busy}>
			<legend class="sr-only">Agents and people</legend>
			<Input type="search" bind:value={search} placeholder="Search agents and people…" aria-label="Search agents and people" autocomplete="off" />
			<ul class="max-h-60 divide-y divide-white/5 overflow-y-auto rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Agents and people">
				{#each shownCandidates as m (m.id)}
					{@const also = shown ? otherTeamsOf(snapshot, m.id, shown.id) : []}
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
			<Label for="members-lead">Lead</Label>
			<select id="members-lead" bind:value={lead} disabled={busy || leadChoices.length === 0} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400 disabled:opacity-60">
				<option value="">No lead</option>
				{#each leadChoices as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
			</select>
			<p class="text-xs/5 text-gray-400">The lead also leads the team's projects.</p>
		</div>

		{#if clearsLead}
			<p class="flex gap-x-2 rounded-lg bg-amber-500/10 px-3 py-2 text-sm/6 text-amber-100 outline-1 -outline-offset-1 outline-amber-400/20 [overflow-wrap:anywhere]" data-testid="clears-lead">
				<TriangleAlert class="mt-1 size-4 shrink-0" aria-hidden="true" />
				<span>{leaderName} leads {shown?.name}. Removing {leaderName} also clears {shown?.name}'s lead.</span>
			</p>
		{/if}
		{#if problems.length > 0}
			<ul class="space-y-1 text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">
				{#each problems as problem (problem)}<li>{problem}</li>{/each}
			</ul>
		{/if}

		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" disabled={busy} onclick={save} variant={clearsLead ? 'destructive' : 'default'}>{clearsLead ? 'Remove and clear lead' : 'Save'}</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
