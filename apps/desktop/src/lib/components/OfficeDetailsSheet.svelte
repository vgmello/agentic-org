<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import OfficeStatusBadge from './OfficeStatusBadge.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { agentsIn, roleInfo, secondaryText, type DisplayStatus } from '$lib/offices/model';
	import type { OfficeInfo, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		office,
		snapshot,
		status,
		counting = false,
		sample = false,
		here = false
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
	} = $props();

	const agents = $derived(office && snapshot ? agentsIn(snapshot, office.id) : []);
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
				<dt class="text-gray-400">Agents here</dt>
				<dd class="min-w-0 text-white [overflow-wrap:anywhere]">
					{#if !snapshot}{counting ? 'Counting…' : "Couldn't be counted"}{:else}{agents.length === 0 ? 'None' : agents.map((a) => a.name).join(', ')}{/if}
					{#if sample && agents.length > 0}<Badge variant="secondary" data-testid="sample-data-note-sheet">Sample data</Badge>{/if}
				</dd>
			</dl>
			<SheetFooter class="mt-auto">
				<Button variant="outline" onclick={() => (open = false)}>Close</Button>
			</SheetFooter>
		{/if}
	</SheetContent>
</Sheet>
