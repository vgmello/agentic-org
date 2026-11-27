<script lang="ts">
	import LogOut from '@lucide/svelte/icons/log-out';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { isBuiltInAgent, toAppError } from '$lib/proto';
	import { offboardAgent } from '$lib/modules/organization/actions';
	import { offboardSummary } from '$lib/modules/organization/agents';
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
	let lastAgent = $state<Member | undefined>();
	// While open, reflect live agent updates (e.g. version bump on stale reload); while closing, keep lastAgent so the title does not flash empty.
	const shown = $derived(open ? (agent ?? lastAgent) : lastAgent);
	const lines = $derived(shown ? offboardSummary(snapshot, shown) : []);
	const isBuiltIn = $derived(isBuiltInAgent(shown));

	function resetOnOpen() {
		if (agent) lastAgent = agent;
		error = undefined;
	}

	async function confirm() {
		if (!shown || busy) return;
		if (isBuiltIn) {
			error = 'Built-in agents cannot be offboarded.';
			return;
		}
		busy = true;
		try {
			await offboardAgent(session, shown.id, shown.version);
			open = false;
			ondone({ name: shown.name });
		} catch (e) {
			const failure = toAppError(e);
			// Already changed elsewhere (spec §9): refresh, and nothing is applied twice.
			error =
				failure.code === 'conflict' || failure.code === 'invalid_request'
					? `${shown.name} changed at HQ while you were looking. Here's the latest.`
					: failure.message;
			ondone({ stale: true });
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent onOpenAutoFocus={resetOnOpen}>
		<DialogHeader>
			<div
				class="mb-2 flex size-12 items-center justify-center rounded-full bg-danger-emphasis/15 text-danger"
			>
				<LogOut class="size-6" aria-hidden="true" />
			</div>
			<DialogTitle text={`Offboard ${shown?.name ?? ''}?`} />
			<DialogDescription class="space-y-1 [overflow-wrap:anywhere]">
				{#each lines as line (line)}<span class="block">{line}</span>{/each}
			</DialogDescription>
			{#if error}<p class="text-sm/6 text-danger [overflow-wrap:anywhere]" role="alert">
					{error}
				</p>{/if}
		</DialogHeader>
		<DialogFooter>
			<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}
				>Cancel</Button
			>
			<Button
				type="button"
				variant="destructive"
				disabled={busy || isBuiltIn}
				onclick={confirm}
				class="[overflow-wrap:anywhere]">Offboard {shown?.name}</Button
			>
		</DialogFooter>
	</DialogContent>
</Dialog>
