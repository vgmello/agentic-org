<script lang="ts">
	import { untrack } from 'svelte';
	import { SvelteMap } from 'svelte/reactivity';
	import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { toAppError } from '$lib/office/api';
	import { ledTeamsLeft, setMemberTeams, teamTitlesOf } from '$lib/org/actions';
	import type { OrgSession } from '$lib/org/session';
	import { teamOptions } from '$lib/org/team-edit';
	import type { Member, OrgSnapshot } from '$lib/proto';

	/** TEAM-6, from the member's side: a checklist of the team tree with an optional title in each checked team. */
	let {
		open = $bindable(false),
		member,
		snapshot,
		session,
		ondone
	}: {
		open: boolean;
		member: Member | undefined;
		snapshot: OrgSnapshot;
		session: OrgSession;
		ondone: (result: { name: string; changed: boolean; problems: string[] }) => void;
	} = $props();

	// team id -> title ('' for none). A team is in the map exactly when it is checked.
	const wanted = new SvelteMap<string, string>();
	let busy = $state(false);
	let problems = $state<string[]>([]);
	let shown = $state<Member | undefined>();

	$effect(() => {
		if (open && member) {
			untrack(() => {
				shown = member;
				wanted.clear();
				teamTitlesOf(snapshot, member.id).forEach((v, teamId) => wanted.set(teamId, v.title ?? ''));
				problems = [];
			});
		}
	});

	const teams = $derived(teamOptions(snapshot));
	const teamName = (id: string) => snapshot.teams.find((t) => t.id === id)?.name ?? id;
	// Unchecking a team the member leads clears its lead: say so before it happens (TEAM-6).
	const leaving = $derived(shown ? ledTeamsLeft(snapshot, shown.id, new Set(wanted.keys())) : []);

	async function save() {
		if (!shown || busy) return;
		busy = true;
		problems = [];
		const who = shown;
		try {
			const { steps } = await setMemberTeams(session, who.id, new Map([...wanted].map(([id, title]) => [id, title] as const)), teamName);
			const failed = steps.filter((s) => s.error);
			if (failed.length === 0) {
				open = false;
				ondone({ name: who.name, changed: steps.length > 0, problems: [] });
			} else {
				// The dialog stays open, names what failed and keeps the rest (spec §7.6).
				problems = failed.map((s) => `Couldn't ${s.what.charAt(0).toLowerCase()}${s.what.slice(1)}: ${s.error!.message}`);
				ondone({ name: who.name, changed: true, problems });
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
			<DialogTitle class="[overflow-wrap:anywhere]">Teams for {shown?.name}</DialogTitle>
			<DialogDescription>Check each team they are in, and give them a title there if you like.</DialogDescription>
		</DialogHeader>

		<fieldset class="space-y-1" disabled={busy}>
			<legend class="sr-only">Teams</legend>
			<ul class="max-h-64 divide-y divide-white/5 overflow-y-auto rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Teams">
				{#each teams as option (option.team.id)}
					<li class="flex flex-wrap items-center gap-x-3 gap-y-1 py-1.5 pr-3" style="padding-left: {0.75 + option.depth * 1.25}rem">
						<label class="flex min-w-0 cursor-pointer items-center gap-x-3 text-sm/6">
							<Checkbox checked={wanted.has(option.team.id)} onCheckedChange={(on) => (on === true ? wanted.set(option.team.id, wanted.get(option.team.id) ?? '') : wanted.delete(option.team.id))} aria-label={option.path} />
							<span class="min-w-0 text-white [overflow-wrap:anywhere]">{option.team.name}</span>
						</label>
						{#if wanted.has(option.team.id)}
							<Input class="h-7 max-w-44 text-xs" value={wanted.get(option.team.id) ?? ''} oninput={(e) => wanted.set(option.team.id, e.currentTarget.value)} placeholder="Title (optional)" aria-label="Title in {option.team.name}" autocomplete="off" />
						{/if}
					</li>
				{:else}
					<li class="px-3 py-2 text-sm/6 text-gray-400">There are no teams yet.</li>
				{/each}
			</ul>
		</fieldset>

		{#if leaving.length > 0}
			<p class="flex gap-x-2 rounded-lg bg-amber-500/10 px-3 py-2 text-sm/6 text-amber-100 outline-1 -outline-offset-1 outline-amber-400/20 [overflow-wrap:anywhere]" data-testid="clears-lead">
				<TriangleAlert class="mt-1 size-4 shrink-0" aria-hidden="true" />
				<span>{#each leaving as t, i (t.id)}{i > 0 ? ' ' : ''}{shown?.name} leads {t.name}. Removing {shown?.name} also clears {t.name}'s lead.{/each}</span>
			</p>
		{/if}
		{#if problems.length > 0}
			<ul class="space-y-1 text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">
				{#each problems as problem (problem)}<li>{problem}</li>{/each}
			</ul>
		{/if}

		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" disabled={busy} onclick={save} variant={leaving.length > 0 ? 'destructive' : 'default'}>{leaving.length > 0 ? 'Remove and clear lead' : 'Save'}</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
