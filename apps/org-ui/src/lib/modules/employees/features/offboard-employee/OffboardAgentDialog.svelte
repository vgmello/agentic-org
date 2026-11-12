<script lang="ts">
	import { untrack } from 'svelte';
	import LogOut from '@lucide/svelte/icons/log-out';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import { offboardAgent } from '$lib/org/actions';
	import { offboardSummary } from '$lib/org/agents';
	import type { OrgSession } from '$lib/org/session';
	import type { Member, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		agent,
		snapshot,
		session,
		ondone
	}: {
		open: boolean;
		agent: Member | undefined;
		snapshot: OrgSnapshot;
		session: OrgSession;
		ondone: (result: { name: string } | { stale: true }) => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	let shown = $state<Member | undefined>();
	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!agent) return;
				shown = agent;
				error = undefined;
			});
		}
	});
	const lines = $derived(shown ? offboardSummary(snapshot, shown) : []);

	// After a stale edit (HQ changed the agent meanwhile) the page reloads: take the latest as the baseline and the
	// version to send, but leave anything the owner typed alone, so "Here's the latest" is true and a retry can work.
	$effect(() => {
		const latest = agent;
		untrack(() => {
			if (open && shown && latest && latest.id === shown.id && latest.version !== shown.version) shown = latest;
		});
	});

	async function confirm() {
		if (!shown || busy) return;
		busy = true;
		try {
			await offboardAgent(session, shown.id, shown.version);
			open = false;
			ondone({ name: shown.name });
		} catch (e) {
			const failure = toAppError(e);
			// Already changed elsewhere (spec §9): refresh, and nothing is applied twice.
			error = failure.code === 'conflict' || failure.code === 'invalid_request' ? `${shown.name} changed at HQ while you were looking. Here's the latest.` : failure.message;
			ondone({ stale: true });
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<DialogHeader>
			<div class="mb-2 flex size-12 items-center justify-center rounded-full bg-red-500/15 text-red-400"><LogOut class="size-6" aria-hidden="true" /></div>
			<DialogTitle text={`Offboard ${shown?.name ?? ''}?`} />
			<DialogDescription class="space-y-1 [overflow-wrap:anywhere]">
				{#each lines as line (line)}<span class="block">{line}</span>{/each}
			</DialogDescription>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
		</DialogHeader>
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" variant="destructive" disabled={busy} onclick={confirm} class="[overflow-wrap:anywhere]">Offboard {shown?.name}</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
