<script lang="ts">
	import { untrack } from 'svelte';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import { removePerson } from '$lib/org/actions';
	import { removePersonSummary } from '$lib/org/people-edit';
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
		ondone: (result: { name: string } | { stale: true }) => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	let shown = $state<Member | undefined>();
	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!(person)) return;
				shown = person;
				error = undefined;
			});
		}
	});
	const lines = $derived(shown ? removePersonSummary(snapshot, shown, organization) : []);

	async function confirm() {
		if (!shown || busy) return;
		busy = true;
		try {
			await removePerson(session, shown.id);
			open = false;
			ondone({ name: shown.name });
		} catch (e) {
			const failure = toAppError(e);
			error = failure.code === 'conflict' ? `${shown.name} changed at HQ while you were looking. Here's the latest.` : failure.message;
			if (failure.code === 'conflict' || failure.code === 'not_found') ondone({ stale: true });
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<DialogHeader>
			<div class="mb-2 flex size-12 items-center justify-center rounded-full bg-red-500/15 text-red-400"><Trash2 class="size-6" aria-hidden="true" /></div>
			<DialogTitle class="[overflow-wrap:anywhere]">Remove {shown?.name} from {organization}?</DialogTitle>
			<DialogDescription class="space-y-1 [overflow-wrap:anywhere]">
				{#each lines as line (line)}<span class="block">{line}</span>{/each}
			</DialogDescription>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
		</DialogHeader>
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" variant="destructive" disabled={busy} onclick={confirm} class="[overflow-wrap:anywhere]">Remove {shown?.name}</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
