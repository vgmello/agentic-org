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
		opener,
		fallback,
		ondone
	}: {
		open: boolean;
		target: { projectId: string; projectName: string; teamName: string; teamId: string } | undefined;
		session: OrgSession;
		/** The Remove button that opened this: Cancel and Escape give focus back to it. Passing it (even `null`) also makes Confirm wait for `ondone` before closing. */
		opener?: HTMLElement | null;
		/** Where focus goes when that button is gone (the team was removed and the page reloaded). */
		fallback?: () => HTMLElement | null;
		/** Done (or refused): the page reloads. Awaited, so the dialog closes once what is behind it is up to date. */
		ondone: (result: { message?: string }) => void | Promise<void>;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	let shown = $state<typeof target>();
	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!target) return;
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
			const done = ondone({ message: `Removed ${shown.projectName} from ${shown.teamName}.` });
			// With an opener (the project page) the dialog closes once the page behind it is up to date, so focus can go to a row that exists.
			if (opener !== undefined) await done;
			open = false;
		} catch (e) {
			error = toAppError(e).message;
			// A refusal must not hold Cancel and Escape until the reload is done.
			void ondone({});
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent {opener} {fallback}>
		<DialogHeader>
			<DialogTitle text={`Remove ${shown?.projectName ?? ''} from ${shown?.teamName ?? ''}?`} />
			<DialogDescription class="[overflow-wrap:anywhere]">It will have no team and show under Unassigned projects.</DialogDescription>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
		</DialogHeader>
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
			<Button type="button" variant="destructive" disabled={busy} onclick={confirm}>Remove</Button>
		</DialogFooter>
	</DialogContent>
</Dialog>
