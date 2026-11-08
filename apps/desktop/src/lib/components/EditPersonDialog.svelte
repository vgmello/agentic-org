<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { updatePerson } from '$lib/org/actions';
	import { checkPersonName } from '$lib/org/people-edit';
	import type { OrgSession } from '$lib/org/session';
	import type { Member, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		person,
		snapshot,
		session,
		organization,
		ondone
	}: {
		open: boolean;
		person: Member | undefined;
		snapshot: OrgSnapshot;
		session: OrgSession;
		organization: string;
		ondone: (result: { name: string; stale?: boolean }) => void;
	} = $props();

	let name = $state('');
	let contact = $state('');
	let error = $state<string | undefined>();
	let busy = $state(false);
	let shown = $state<Member | undefined>();

	$effect(() => {
		if (open && person) {
			untrack(() => {
				shown = person;
				name = person.name;
				contact = person.contact ?? '';
				error = undefined;
			});
		}
	});

	const check = $derived(shown ? checkPersonName(snapshot, name, organization, shown.id) : ({ ok: false, message: '' } as const));
	const nameError = $derived(!check.ok && name.trim() !== '' ? check.message : undefined);
	const renamed = $derived(!!shown && check.ok && check.name !== shown.name);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (!shown || busy || !check.ok) return;
		const change: { name?: string; contact?: string } = {};
		if (check.name !== shown.name) change.name = check.name;
		if (contact.trim() !== (shown.contact ?? '')) change.contact = contact;
		if (Object.keys(change).length === 0) {
			open = false;
			return;
		}
		busy = true;
		try {
			await updatePerson(session, shown.id, change);
			open = false;
			ondone({ name: check.name });
		} catch (e) {
			const failure = toAppError(e);
			if (failure.code === 'conflict') {
				// Stale edit (spec §7.10): show the latest and keep what was typed.
				error = `${shown.name} changed at HQ while you were editing. Here's the latest.`;
				ondone({ name: shown.name, stale: true });
			} else error = failure.message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<form class="grid gap-6" onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle class="[overflow-wrap:anywhere]">Edit {shown?.name}</DialogTitle>
				<DialogDescription>Agents address a person by their name.</DialogDescription>
			</DialogHeader>
			<div class="space-y-2">
				<Label for="edit-person-name">Name</Label>
				<Input id="edit-person-name" bind:value={name} disabled={busy} autocomplete="off" aria-invalid={nameError ? true : undefined} aria-describedby="edit-person-name-help" />
				{#if nameError}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{nameError}</p>{/if}
				{#if renamed}<p id="edit-person-name-help" class="text-xs/5 text-gray-300" data-testid="rename-note">Agents will address them as {check.ok ? check.name : ''} from now on.</p>{/if}
			</div>
			<div class="space-y-2">
				<Label for="edit-person-contact">Contact <span class="font-normal text-gray-400">(clear it to remove)</span></Label>
				<Input id="edit-person-contact" bind:value={contact} disabled={busy} autocomplete="off" />
			</div>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || !check.ok}>Save</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
