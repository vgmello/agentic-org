<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import { removeProjectTeam } from '$lib/org/actions';
	import type { OrgSession } from '$lib/org/session';

	/** Removing a project's LAST team asks first: it will have no team and show under Unassigned projects. */
	let {
		open = $bindable(false),
		target,
		session,
		ondone
	}: {
		open: boolean;
		target: { projectId: string; projectName: string; teamName: string; teamId: string } | undefined;
		session: OrgSession;
		ondone: (result: { message?: string }) => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	let shown = $state<typeof target>();
	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!(target)) return;
				shown = target;
				error = undefined;
			});
		}
	});

	async function confirm() {
		if (!shown || busy) return;
		busy = true;
		try {
			await removeProjectTeam(session, shown.projectId, shown.teamId);
			open = false;
			ondone({ message: `Removed ${shown.projectName} from ${shown.teamName}.` });
		} catch (e) {
			error = toAppError(e).message;
			ondone({});
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle class="[overflow-wrap:anywhere]">Remove {shown?.projectName} from {shown?.teamName}?</DialogTitle>
			<DialogDescription class="[overflow-wrap:anywhere]">It will have no team and show under Unassigned projects.</DialogDescription>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
		</DialogHeader>
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" variant="destructive" disabled={busy} onclick={confirm}>Remove</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
