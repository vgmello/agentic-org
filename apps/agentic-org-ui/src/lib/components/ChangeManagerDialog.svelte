<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import { changeManager } from '$lib/org/actions';
	import { kindWord } from '$lib/org/model';
	import { managerOf, managerOptions } from '$lib/org/reporting';
	import type { OrgSession } from '$lib/org/session';
	import type { Member, OrgSnapshot } from '$lib/proto';

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
		/** Changed (with a way to undo it), or HQ refused / the lines changed meanwhile (`stale`): reload either way. */
		ondone: (result: { name: string; to: string; undo: () => Promise<void> } | { stale: true }) => void;
	} = $props();

	let target = $state<string | undefined>();
	let error = $state<string | undefined>();
	let busy = $state(false);
	let shown = $state<Member | undefined>();

	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!member) return;
				shown = member;
				target = undefined;
				error = undefined;
			});
		}
	});

	const options = $derived(shown ? managerOptions(snapshot, shown.id) : []);
	const current = $derived(shown ? managerOf(snapshot, shown) : undefined);
	const label = (m: Member) => (m.owner ? `${m.name} (you)` : m.name);

	async function save() {
		if (!shown || target === undefined || busy) return;
		busy = true;
		const name = shown.name;
		const chosen = options.find((o) => o.member.id === target)?.member;
		try {
			// The owner's own id is stored as "no manager", so reporting to the owner clears the field.
			const { undo } = await changeManager(session, shown.id, chosen && !chosen.owner ? chosen.id : undefined);
			open = false;
			ondone({ name, to: chosen ? label(chosen) : 'you', undo });
		} catch (e) {
			// HQ refused (REPORT-4): nothing changed, and the current lines are shown again.
			error = `Couldn't change ${name}'s manager: ${toAppError(e).message} Nothing changed.`;
			ondone({ stale: true });
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle text={`Change manager of ${shown?.name ?? ''}`} />
			<DialogDescription>Reporting lines are separate from teams. {shown?.name} reports to {current ? label(current) : 'you'} now.</DialogDescription>
		</DialogHeader>

		<fieldset class="max-h-64 space-y-1 overflow-y-auto" disabled={busy}>
			<legend class="sr-only">New manager</legend>
			{#each options as { member: m, disabled, reason } (m.id)}
				{@const isCurrent = (current?.id ?? snapshot.members.find((x) => x.owner)?.id) === m.id}
				<label class="flex items-start gap-x-3 rounded-md px-2 py-1.5 text-sm/6 hover:bg-white/5 has-disabled:opacity-60">
					<input type="radio" name="manager" value={m.id} class="mt-1" checked={target === m.id} disabled={disabled || isCurrent} onchange={() => (target = m.id)} />
					<span class="min-w-0">
						<span class="block text-white [overflow-wrap:anywhere]">{label(m)} <span class="text-xs text-gray-400">{kindWord(m.kind)}{isCurrent ? ' · current' : ''}</span></span>
						{#if disabled}<span class="block text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{reason}</span>{/if}
					</span>
				</label>
			{/each}
		</fieldset>

		{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}

		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" disabled={busy || target === undefined} onclick={save}>Change manager</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
