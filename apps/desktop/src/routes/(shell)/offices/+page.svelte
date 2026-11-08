<script lang="ts">
	import { invalidate } from '$app/navigation';
	import Plus from '@lucide/svelte/icons/plus';
	import { onMount } from 'svelte';
	import AddOfficeSheet from '$lib/components/AddOfficeSheet.svelte';
	import OfficeDetailsSheet from '$lib/components/OfficeDetailsSheet.svelte';
	import OfficeStatusBadge from '$lib/components/OfficeStatusBadge.svelte';
	import {
		AlertDialog,
		AlertDialogAction,
		AlertDialogCancel,
		AlertDialogContent,
		AlertDialogDescription,
		AlertDialogFooter,
		AlertDialogHeader,
		AlertDialogTitle
	} from '$lib/components/ui/alert-dialog';
	import { Button, buttonVariants } from '$lib/components/ui/button';
	import { officesApi } from '$lib/offices/api';
	import { agentsIn, isHq, orderOffices, removeSummary, secondaryText, unplacedAgents } from '$lib/offices/model';
	import { toAppError } from '$lib/office/api';
	import type { OfficeWithExtras } from '$lib/proto';

	let { data } = $props();

	const offices = $derived(orderOffices(data.offices));
	const here = $derived(data.connection.office);
	// Adding and removing offices happens on HQ's Mac (spec §6, A5): every office on this Mac is HQ's Mac in phase 1.
	const canManage = $derived(here.role === 'hq');
	const unplaced = $derived(unplacedAgents(data.snapshot, data.offices));
	const organization = $derived(data.connection.organization.name);

	let now = $state(0);
	onMount(() => (now = Date.now()));

	let adding = $state(false);
	let detailsId = $state<string | undefined>();
	// The open sheet shows the latest data for its office, not the copy captured when it was clicked.
	const detailsFor = $derived(data.offices.find((o) => o.id === detailsId));
	let detailsOpen = $state(false);
	let removing = $state<OfficeWithExtras | undefined>();
	let removingName = $state(''); // kept while the dialog closes, so its title doesn't flash empty
	let removeBusy = $state(false);
	let removeError = $state<string | undefined>();
	let highlighted = $state<string | undefined>();

	const reload = () => invalidate('app:offices');

	async function confirmRemove() {
		if (!removing || removeBusy) return;
		removeBusy = true;
		try {
			await officesApi.remove(removing.id);
			removing = undefined;
			detailsOpen = false;
		} catch (e) {
			removeError = toAppError(e).message;
		} finally {
			removeBusy = false;
			await reload(); // whatever happened, show how things are now
		}
	}
	const summary = $derived(removing ? removeSummary(removing, organization, data.snapshot) : []);

	function open(o: OfficeWithExtras) {
		detailsId = o.id;
		detailsOpen = true;
	}
	async function added() {
		const before = new Set(data.offices.map((o) => o.id));
		await reload();
		highlighted = data.offices.find((o) => !before.has(o.id))?.id;
		setTimeout(() => (highlighted = undefined), 2500);
	}
</script>

<svelte:head><title>Offices · Agentic Org</title></svelte:head>

<div class="space-y-5 px-8 py-6">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<p class="max-w-xl text-sm/6 text-gray-400">
			Where {organization}'s agents run. HQ keeps the records and approves new offices; every other office keeps a copy.
		</p>
		<div class="text-right">
			<Button disabled={!canManage} aria-describedby={canManage ? undefined : 'add-reason'} onclick={() => (adding = true)}><Plus aria-hidden="true" /> Add office</Button>
			{#if !canManage}<p id="add-reason" class="mt-1 text-xs/5 text-gray-400">Add offices from HQ's Mac ({offices.find(isHq)?.name ?? 'HQ'}).</p>{/if}
		</div>
	</div>

	<ul class="divide-y divide-white/5 overflow-hidden rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Offices">
		{#each offices as office (office.id)}
			{@const agents = agentsIn(data.snapshot, office.id).length}
			<li data-testid="office-row" data-office={office.id} class="flex items-center gap-x-4 px-4 py-3 {highlighted === office.id ? 'bg-indigo-500/15' : ''}">
				<span class="flex h-6 w-10 shrink-0 items-center justify-center rounded text-xs font-semibold {isHq(office) ? 'bg-indigo-500/20 text-indigo-200' : 'text-transparent'}" aria-hidden={!isHq(office)}>
					{isHq(office) ? 'HQ' : ''}
				</span>
				<button type="button" class="min-w-0 flex-1 text-left focus-visible:outline-2 focus-visible:outline-indigo-400" onclick={() => open(office)}>
					<span class="block truncate text-sm/6 font-semibold text-white">{office.name}</span>
					<span class="block truncate text-xs/5 text-gray-400">{secondaryText(office, now || Date.now()) + (here.id === office.id ? " (you're here)" : '')}</span>
				</button>
				<OfficeStatusBadge status={office.status} />
				<span class="w-20 text-right text-xs/5 text-gray-400">{agents} {agents === 1 ? 'agent' : 'agents'}</span>
			</li>
		{/each}
	</ul>

	{#if offices.length === 1}
		<p class="text-sm/6 text-gray-400" data-testid="add-hint">Add an office to run agents on another Mac, or to keep a copy of the records there.</p>
	{/if}

	{#if unplaced.length > 0}
		<p class="rounded-lg bg-amber-500/10 px-4 py-3 text-sm/6 text-amber-100 outline-1 -outline-offset-1 outline-amber-400/20" data-testid="unplaced-note">
			{unplaced.length} {unplaced.length === 1 ? 'agent is' : 'agents are'} not placed in an office: {unplaced.map((a) => a.name).join(', ')}.
		</p>
	{/if}
	<p class="text-xs/5 text-gray-500">Shown from this Mac.</p>
</div>

<AddOfficeSheet bind:open={adding} offices={data.offices} onchanged={added} />
<OfficeDetailsSheet bind:open={detailsOpen} office={detailsFor} snapshot={data.snapshot} now={now || Date.now()} canRemove={canManage} onremove={(o) => ((removing = o), (removingName = o.name), (removeError = undefined))} />

<AlertDialog open={!!removing} onOpenChange={(o) => !o && (removing = undefined)}>
	<AlertDialogContent>
		<AlertDialogHeader>
			<AlertDialogTitle class="[overflow-wrap:anywhere]">Remove {removingName} from {organization}?</AlertDialogTitle>
			<AlertDialogDescription class="[overflow-wrap:anywhere]">
				{#each summary as line (line)}<span class="block">{line}</span>{/each}
			</AlertDialogDescription>
			{#if removeError}<p class="text-sm/6 text-red-400" role="alert">{removeError}</p>{/if}
		</AlertDialogHeader>
		<AlertDialogFooter>
			<AlertDialogCancel>Cancel</AlertDialogCancel>
			<AlertDialogAction class={buttonVariants({ variant: 'destructive' })} disabled={removeBusy} onclick={confirmRemove}>Remove office</AlertDialogAction>
		</AlertDialogFooter>
	</AlertDialogContent>
</AlertDialog>
