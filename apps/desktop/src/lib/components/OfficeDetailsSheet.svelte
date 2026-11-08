<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import OfficeStatusBadge from './OfficeStatusBadge.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { toast } from 'svelte-sonner';
	import pkg from '../../../package.json';
	import { officeApi, toAppError } from '$lib/office/api';
	import { agentsIn, copyDetails, restartAction, restartOutcome, roleInfo, secondaryText, statusInfo, type DisplayStatus } from '$lib/offices/model';
	import type { OfficeInfo, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		office,
		snapshot,
		status,
		counting = false,
		sample = false,
		here = false,
		why,
		capabilities,
		onrestarted
	}: {
		open: boolean;
		office: OfficeInfo | undefined;
		/** The status to show: what the app has verified, not what was saved. */
		status: DisplayStatus;
		/** Undefined while the agents are still being counted, or when they couldn't be. */
		snapshot: OrgSnapshot | undefined;
		counting?: boolean;
		/** The agents come from the invented sample organization: say so where they are listed. */
		sample?: boolean;
		/** This is the office the app is connected to. */
		here?: boolean;
		/** Why the office is stopped, when the host says it is (spec §3.3). */
		why?: string;
		/** What the host's last hello listed; absent when not known yet. */
		capabilities?: string[];
		/** The host accepted a restart: reload the offices' statuses. */
		onrestarted?: () => Promise<void> | void;
	} = $props();

	const agents = $derived(office && snapshot ? agentsIn(snapshot, office.id) : []);

	// Restarting (spec §3.3): the spinner stays until the office is no longer stopped, or half a minute has passed.
	let restarting = $state(false);
	let restartNote = $state<string | undefined>();
	let unsupported = $state(false);
	const action = $derived(office ? restartAction(office, capabilities) : {});
	$effect(() => {
		// A different office, or a closed sheet, starts clean.
		void office?.id;
		restarting = false;
		restartNote = undefined;
	});
	async function restart() {
		if (!office || restarting) return;
		restarting = true;
		restartNote = undefined;
		const id = office.id;
		try {
			await officeApi.restartOffice(id);
		} catch (e) {
			const err = toAppError(e);
			const outcome = restartOutcome(err.source === 'api' ? err.code : 'unknown');
			if (outcome.kind === 'unsupported') unsupported = true;
			if (outcome.kind === 'refresh') await onrestarted?.();
			if (outcome.kind !== 'wait') {
				restartNote = outcome.message;
				restarting = false;
				return;
			}
		}
		// Keep the spinner until the host says it is no longer stopped (the live status), for up to 30 s.
		for (let i = 0; i < 15 && restarting; i++) {
			await onrestarted?.();
			if (office?.id !== id || (office.status !== 'offline' && office.status !== 'unavailable')) break;
			await new Promise((r) => setTimeout(r, 2000));
		}
		restarting = false;
	}

	async function copy() {
		if (!office) return;
		try {
			await navigator.clipboard.writeText(copyDetails(office, statusInfo(status).label, new Date(), pkg.version));
			toast.success('Copied the details.');
		} catch {
			toast.error("Couldn't copy. Select the details and copy them yourself.");
		}
	}
</script>

<Sheet bind:open>
	<SheetContent side="right" class="w-full max-w-md">
		{#if office}
			<SheetHeader>
				<SheetTitle class="[overflow-wrap:anywhere]">{office.name}</SheetTitle>
				<SheetDescription>{roleInfo(office).description}</SheetDescription>
			</SheetHeader>
			<dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-3 px-4 text-sm/6" data-testid="office-details">
				<dt class="text-gray-400">Role</dt>
				<dd class="text-white">{roleInfo(office).label}</dd>
				<dt class="text-gray-400">Where</dt>
				<dd class="text-white">{secondaryText(office) + (here ? " (you're here)" : '')}</dd>
				<dt class="text-gray-400">Status</dt>
				<dd><OfficeStatusBadge {status} /></dd>
				{#if why}
					<dt class="text-gray-400">Why</dt>
					<dd class="text-white [overflow-wrap:anywhere]" data-testid="office-why">{why}</dd>
				{/if}
				<dt class="text-gray-400">Agents here</dt>
				<dd class="min-w-0 text-white [overflow-wrap:anywhere]">
					{#if !snapshot}{counting ? 'Counting…' : "Couldn't be counted"}{:else}{agents.length === 0 ? 'None' : agents.map((a) => a.name).join(', ')}{/if}
					{#if sample && agents.length > 0}<Badge variant="secondary" data-testid="sample-data-note-sheet">Sample data</Badge>{/if}
				</dd>
			</dl>
			{#if why && (action.note || unsupported)}
				<p class="px-4 text-sm/6 text-gray-300" data-testid="restart-note">{action.note ?? 'Update Agentic Org to restart offices from here'}</p>
			{/if}
			{#if restartNote}<p class="px-4 text-sm/6 text-red-300" role="alert" data-testid="restart-error">{restartNote}</p>{/if}
			<SheetFooter class="mt-auto">
				{#if why && action.label && !unsupported}
					<Button disabled={restarting} onclick={restart} data-testid="restart-office">{restarting ? 'Restarting…' : action.label}</Button>
				{/if}
				{#if why}<Button variant="outline" onclick={copy}>Copy details</Button>{/if}
				<Button variant="outline" onclick={() => (open = false)}>Close</Button>
			</SheetFooter>
		{/if}
	</SheetContent>
</Sheet>
