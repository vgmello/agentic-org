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
	import { removeProjectTeam } from '$lib/modules/organization/actions';
	import type { OrgSession } from '$lib/modules/organization/session';

	/** Removing a project's LAST team asks first: it will have no team and show under Unassigned projects. */
	let {
		open = $bindable(false),
		target,
		opener,
		fallback,
		session,
		ondone
	}: {
		open: boolean;
		target:
			{ projectId: string; projectName: string; teamName: string; teamId: string } | undefined;
		opener?: HTMLElement | null;
		fallback?: () => HTMLElement | null;
		session: OrgSession;
		ondone: (result: { message?: string }) => void | Promise<void>;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	let lastTarget = $state<typeof target>();
	const shown = $derived(open ? (target ?? lastTarget) : lastTarget);
	let confirmed = false;

	function resetOnOpen() {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (target) lastTarget = target;
		error = undefined;
		confirmed = false;
	}

	async function confirm() {
		if (!shown || busy) return;
		busy = true;
		try {
			await removeProjectTeam(session, shown.projectId, shown.teamId);
			confirmed = true;
			open = false;
			await ondone({ message: `Removed ${shown.projectName} from ${shown.teamName}.` });
		} catch (e) {
			error = toAppError(e).message;
			await ondone({});
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent
		{opener}
		{fallback}
		onOpenAutoFocus={resetOnOpen}
		onCloseAutoFocus={(e) => {
			// When confirmed, `ondone` reloads the row and moves focus to Assign team; don't focus the gone chip first.
			if (confirmed) e.preventDefault();
		}}
	>
		<DialogHeader>
			<DialogTitle text={`Remove ${shown?.projectName ?? ''} from ${shown?.teamName ?? ''}?`} />
			<DialogDescription class="[overflow-wrap:anywhere]"
				>It will have no team and show under Unassigned projects.</DialogDescription
			>
			{#if error}<p class="text-sm/6 text-danger [overflow-wrap:anywhere]" role="alert">
					{error}
				</p>{/if}
		</DialogHeader>
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}
				>Cancel</Button
			>
			<Button type="button" variant="destructive" disabled={busy} onclick={confirm}>Remove</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
