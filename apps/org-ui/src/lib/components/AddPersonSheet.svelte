<script lang="ts">
	import { untrack } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import Info from '@lucide/svelte/icons/info';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { addPerson } from '$lib/org/actions';
	import { byName, owner } from '$lib/org/model';
	import { checkPersonName } from '$lib/org/people-edit';
	import type { OrgSession } from '$lib/org/session';
	import { teamOptions } from '$lib/org/team-edit';
	import type { AppError, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		snapshot,
		session,
		organization,
		presetTeamId = undefined,
		presetName = '',
		ondone
	}: {
		open: boolean;
		snapshot: OrgSnapshot;
		session: OrgSession;
		/** The organization's name, for "… is already in Acme". */
		organization: string;
		/** A team to tick (Add person to Checkout…). */
		presetTeamId?: string;
		presetName?: string;
		ondone: (result: { memberId: string; name: string; problems: string[] }) => void;
	} = $props();

	let name = $state('');
	let contact = $state('');
	let manager = $state('');
	const teamIds = new SvelteSet<string>();
	let failure = $state<AppError | undefined>();
	let busy = $state(false);
	let touched = $state(false);

	$effect(() => {
		if (open) {
			untrack(() => {
				name = presetName;
				contact = '';
				manager = '';
				teamIds.clear();
				if (presetTeamId) teamIds.add(presetTeamId);
				failure = undefined;
				busy = false;
				touched = false;
			});
		}
	});

	// The clash is answered as they type (PEOPLE-4); Add stays disabled until the name is free.
	const check = $derived(checkPersonName(snapshot, name, organization));
	const nameError = $derived(!check.ok && (name.trim() !== '' || touched) ? check.message : undefined);
	const teams = $derived(teamOptions(snapshot));
	const managers = $derived(snapshot.members.filter((m) => !m.owner).toSorted(byName));
	const ownerName = $derived(owner(snapshot)?.name ?? 'you');
	const teamName = (id: string) => snapshot.teams.find((t) => t.id === id)?.name ?? id;

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		touched = true;
		if (busy || !check.ok) return;
		const added = check.name;
		busy = true;
		failure = undefined;
		try {
			const result = await addPerson(session, { name: added, contact: contact.trim() || undefined, teamIds: [...teamIds], managerId: manager || undefined }, teamName);
			if (result.created.error) {
				failure = result.created.error;
				return;
			}
			open = false;
			ondone({ memberId: result.memberId, name: added, problems: result.steps.filter((s) => s.error).map((s) => `Couldn't ${s.what.charAt(0).toLowerCase()}${s.what.slice(1)}: ${s.error!.message}`) });
		} catch (error) {
			failure = toAppError(error);
		} finally {
			busy = false;
		}
	}
</script>

<Sheet bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<SheetContent side="right" class="w-full max-w-md overflow-y-auto">
		<SheetHeader>
			<SheetTitle>Add person</SheetTitle>
			<SheetDescription>People can be in teams and have agents report to them.</SheetDescription>
		</SheetHeader>

		<form class="flex flex-1 flex-col gap-y-5 px-4" onsubmit={submit} novalidate>
			<div class="space-y-2">
				<Label for="person-name">Name</Label>
				<Input id="person-name" bind:value={name} disabled={busy} autocomplete="off" aria-invalid={nameError ? true : undefined} aria-describedby={nameError ? 'person-name-error' : 'person-name-help'} />
				{#if nameError}
					<p id="person-name-error" class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{nameError}</p>
				{:else}
					<p id="person-name-help" class="text-xs/5 text-gray-400">Agents address people by name, so it must be different from everyone else's.</p>
				{/if}
			</div>

			<div class="space-y-2">
				<Label for="person-contact">Contact <span class="font-normal text-gray-400">(optional)</span></Label>
				<Input id="person-contact" bind:value={contact} disabled={busy} autocomplete="off" placeholder="sam@example.com" aria-describedby="person-contact-help" />
				<p id="person-contact-help" class="text-xs/5 text-gray-400">How to reach them outside Agentic Org, e.g. an email.</p>
			</div>

			<fieldset class="space-y-2" disabled={busy}>
				<legend class="text-sm/6 font-medium text-white">Teams <span class="font-normal text-gray-400">(optional)</span></legend>
				<ul class="max-h-40 divide-y divide-white/5 overflow-y-auto rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Teams">
					{#each teams as option (option.team.id)}
						<li>
							<label class="flex cursor-pointer items-center gap-x-3 py-1.5 pr-3 text-sm/6" style="padding-left: {0.75 + option.depth * 1.25}rem">
								<Checkbox checked={teamIds.has(option.team.id)} onCheckedChange={(on) => (on === true ? teamIds.add(option.team.id) : teamIds.delete(option.team.id))} aria-label={option.path} />
								<span class="min-w-0 text-white [overflow-wrap:anywhere]">{option.team.name}</span>
							</label>
						</li>
					{:else}
						<li class="px-3 py-2 text-sm/6 text-gray-400">There are no teams yet. They will show under Not in a team.</li>
					{/each}
				</ul>
			</fieldset>

			<div class="space-y-2">
				<Label for="person-manager">Reports to</Label>
				<select id="person-manager" bind:value={manager} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					<option value="">{ownerName} (you)</option>
					{#each managers as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
				</select>
			</div>

			<p class="flex gap-x-2 rounded-lg bg-sky-500/10 px-3 py-2 text-sm/6 text-sky-100 outline-1 -outline-offset-1 outline-sky-400/20">
				<Info class="mt-1 size-4 shrink-0" aria-hidden="true" />
				<span>Once added, any agent in {organization} can message them, ask them or assign them work.</span>
			</p>

			{#if failure}
				<Alert variant="destructive">
					<CircleAlert aria-hidden="true" />
					<AlertTitle class="[overflow-wrap:anywhere]">Couldn't add the person</AlertTitle>
					<AlertDescription class="[overflow-wrap:anywhere]">{failure.message}</AlertDescription>
				</Alert>
			{/if}

			<SheetFooter class="mt-auto px-0">
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || !check.ok}>Add person</Button>
			</SheetFooter>
		</form>
	</SheetContent>
</Sheet>
