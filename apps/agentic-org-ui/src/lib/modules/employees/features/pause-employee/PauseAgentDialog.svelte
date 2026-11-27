<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/proto';
	import { setAgentStatus } from '$lib/modules/organization/actions';
	import { teamsOfMember } from '$lib/modules/organization/model';
	import type { OrgSession } from '$lib/modules/organization/session';
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
	const shown = $derived(agent);
	const teams = $derived(shown ? teamsOfMember(snapshot, shown.id).map((t) => t.name) : []);

	async function confirm() {
		if (!shown || busy) return;
		busy = true;
		try {
			await setAgentStatus(session, shown.id, 'paused', shown.version);
			open = false;
			ondone({ name: shown.name });
		} catch (e) {
			const failure = toAppError(e);
			error =
				failure.code === 'conflict'
					? `${shown.name} changed at HQ while you were looking. Here's the latest.`
					: failure.message;
			ondone({ stale: true });
		} finally {
			busy = false;
		}
	}
</script>

<Dialog
	bind:open
	onOpenChange={(next) => {
		if (next) error = undefined;
		else if (busy) open = true;
	}}
	onOpenChangeComplete={(next) => {
		if (!next) error = undefined;
	}}
>
	<DialogContent>
		<DialogHeader>
			<DialogTitle text={`Pause ${shown?.name ?? ''}?`} />
			<DialogDescription class="space-y-1 [overflow-wrap:anywhere]">
				<span class="block"
					>{shown?.name} won't be started or woken, and new tasks and messages wait until you resume.</span
				>
				{#if teams.length > 0}<span class="block">{shown?.name} stays in {teams.join(', ')}.</span
					>{/if}
			</DialogDescription>
			{#if error}<p class="text-sm/6 text-danger [overflow-wrap:anywhere]" role="alert">
					{error}
				</p>{/if}
		</DialogHeader>
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}
				>Cancel</Button
			>
			<Button type="button" disabled={busy} onclick={confirm} class="[overflow-wrap:anywhere]"
				>Pause {shown?.name}</Button
			>
		</DialogFooter>
	</DialogContent>
</Dialog>
