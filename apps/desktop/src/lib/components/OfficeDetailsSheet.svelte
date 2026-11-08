<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import OfficeStatusBadge from './OfficeStatusBadge.svelte';
	import { agentsIn, ago, isHq } from '$lib/offices/model';
	import type { OfficeWithExtras, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		office,
		snapshot,
		now,
		canRemove,
		onremove
	}: {
		open: boolean;
		office: OfficeWithExtras | undefined;
		snapshot: OrgSnapshot;
		now: number;
		canRemove: boolean;
		onremove: (office: OfficeWithExtras) => void;
	} = $props();

	const agents = $derived(office ? agentsIn(snapshot, office.id) : []);
</script>

<Sheet bind:open>
	<SheetContent side="right" class="w-full max-w-md">
		{#if office}
			<SheetHeader>
				<SheetTitle class="[overflow-wrap:anywhere]">{office.name}</SheetTitle>
				<SheetDescription>{isHq(office) ? 'HQ keeps the records and approves new offices.' : 'An office keeps a copy of the records.'}</SheetDescription>
			</SheetHeader>
			<dl class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-3 px-4 text-sm/6" data-testid="office-details">
				<dt class="text-gray-400">Role</dt>
				<dd class="text-white">{isHq(office) ? 'HQ' : 'Office'}</dd>
				<dt class="text-gray-400">Where</dt>
				<dd class="text-white">{office.host}</dd>
				<dt class="text-gray-400">Status</dt>
				<dd><OfficeStatusBadge status={office.status} /></dd>
				{#if office.stopped_reason}
					<dt class="text-gray-400">Why it stopped</dt>
					<dd class="text-white">{office.stopped_reason}</dd>
				{/if}
				{#if office.last_seen}
					<dt class="text-gray-400">Last seen</dt>
					<dd class="text-white">{ago(office.last_seen, now)}</dd>
				{/if}
				<dt class="text-gray-400">Sync</dt>
				<dd class="text-white">{office.sync_behind ? `Catching up: ${office.sync_behind} changes` : 'Up to date'}</dd>
				<dt class="text-gray-400">Version</dt>
				<dd class="text-white">{office.app_version ?? 'This app'}</dd>
				<dt class="text-gray-400">Agents here</dt>
				<dd class="text-white">{agents.length === 0 ? 'None' : agents.map((a) => a.name).join(', ')}</dd>
			</dl>
			<SheetFooter class="mt-auto">
				{#if !isHq(office)}
					<Button variant="destructive" disabled={!canRemove} onclick={() => onremove(office)}>Remove office…</Button>
					{#if !canRemove}<p class="text-xs/5 text-gray-400">Remove offices from HQ's Mac.</p>{/if}
				{/if}
				<Button variant="outline" onclick={() => (open = false)}>Close</Button>
			</SheetFooter>
		{/if}
	</SheetContent>
</Sheet>
