<!--
  @component
  Archive a team (offices-and-teams-ux §7.10): its ID and history stay. HQ refuses while child teams or home
  employees remain and names them; the dialog then shows that reason and keeps the team.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/office/api';
	import { teamsApi } from '../../api';
	import type { TeamRow } from '../../types';

	let {
		open = $bindable(false),
		team,
		organizationId,
		ondone
	}: {
		open: boolean;
		team: TeamRow | undefined;
		organizationId: string;
		ondone: (result: { name: string }) => void;
	} = $props();

	let busy = $state(false);
	let error = $state<string | undefined>();
	let shown = $state<TeamRow | undefined>();
	$effect(() => {
		if (open) untrack(() => ((shown = team), (error = undefined)));
	});

	async function confirm() {
		if (!shown || busy) return;
		busy = true;
		try {
			await teamsApi.archive(organizationId, shown.id);
			open = false;
			ondone({ name: shown.name });
		} catch (e) {
			error = toAppError(e).message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<DialogHeader>
			<DialogTitle text={`Archive ${shown?.name ?? ''}?`} />
			<DialogDescription class="space-y-2">
				<span class="block"
					>Its ID, reference and history stay. Archived teams take no new work and are listed apart.</span
				>
				<span class="block"
					>A team can be archived once it has no child teams and no one has it as their home team.</span
				>
			</DialogDescription>
		</DialogHeader>
		{#if error}<p class="text-sm/6 text-red-300 [overflow-wrap:anywhere]" role="alert">
				{error}
			</p>{/if}
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}
				>Cancel</Button
			>
			<Button type="button" variant="destructive" aria-busy={busy} disabled={busy} onclick={confirm}
				>{busy ? 'Archiving…' : `Archive ${shown?.name ?? ''}`}</Button
			>
		</DialogFooter>
	</DialogContent>
</Dialog>
