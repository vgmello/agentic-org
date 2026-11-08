<script lang="ts">
	import { onMount } from 'svelte';
	import { createProbe } from '$lib/offices/probe';
	import { invalidate } from '$app/navigation';
	import Plus from '@lucide/svelte/icons/plus';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import OfficeDetailsSheet from '$lib/components/OfficeDetailsSheet.svelte';
	import OfficeStatusBadge from '$lib/components/OfficeStatusBadge.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { officeApi } from '$lib/office/api';
	import { agentsIn, displayStatusFor, isHq, orderOffices, rowLabel, secondaryText, statusInfo, unplacedAgents, type LiveCheck } from '$lib/offices/model';
	import type { Counts } from './+page';

	let { data } = $props();

	const offices = $derived(orderOffices(data.offices));
	const here = $derived(data.connection.office);
	// The agent counts arrive after the list: `undefined` while they are being counted.
	let counts = $state<Counts | undefined>();
	let retrying = $state(false);
	// True from the first failure until the office answers again: the Retry button stays while it works.
	let retryingProbe = $state(false);
	$effect(() => {
		if (check === 'down') retryingProbe = true;
		else if (check === 'up') retryingProbe = false;
	});
	$effect(() => {
		const pending = data.counts;
		// The last answer stays until the next arrives, so Retry (and its focus) isn't removed while it works.
		pending.then((r) => {
			if (pending !== data.counts) return;
			counts = r;
			retrying = false;
		});
	});
	function retry() {
		retrying = true;
		invalidate('app:offices');
	}
	const snapshot = $derived(counts && 'snapshot' in counts ? counts.snapshot : undefined);
	const sample = $derived(!!counts && 'snapshot' in counts && counts.sample);
	const countError = $derived(counts && 'error' in counts ? counts.error : undefined);
	const unplaced = $derived(snapshot ? unplacedAgents(snapshot, data.offices) : []);
	const organization = $derived(data.connection.organization.name);

	// The one live signal there is: whether the office this app is connected to answers. Checked now, every
	// half minute and on Retry; a background check keeps the last answer instead of flashing "Checking…".
	let check = $state<LiveCheck>('checking');
	const probe = createProbe(officeApi, (next) => {
		check = next;
		// A Retry that worked removes the button the keyboard was on: move focus to the row instead of dropping it.
		if (next === 'up' && document.activeElement?.matches('[data-testid=retry-office]')) rowButton(here.id)?.focus();
	});
	const rowButton = (officeId: string) => document.querySelector<HTMLElement>(`[data-office="${officeId}"] button[aria-haspopup]`);
	onMount(() => {
		void probe.run('explicit');
		const again = setInterval(() => void probe.run('background'), 30_000);
		return () => clearInterval(again);
	});
	// Another organization's office is another thing to verify: don't carry the last answer over.
	let lastOffice: string | undefined;
	$effect(() => {
		const id = here.id;
		if (lastOffice !== undefined && id !== lastOffice) {
			retryingProbe = false;
			void probe.run('explicit');
		}
		lastOffice = id;
	});
	const statusOf = (officeId: string) => displayStatusFor(officeId, here.id, check);

	let detailsId = $state<string | undefined>();
	// The open sheet shows the latest data for its office, not the copy captured when it was clicked.
	const detailsFor = $derived(data.offices.find((o) => o.id === detailsId));
	let detailsOpen = $state(false);
	$effect(() => {
		if (detailsOpen && !detailsFor) detailsOpen = false; // the office went away: close its sheet
	});

	function open(id: string) {
		detailsId = id;
		detailsOpen = true;
	}
</script>

<svelte:head><title>Offices · Agentic Org</title></svelte:head>

<div class="space-y-5 px-8 py-6">
	<div class="flex flex-wrap items-start justify-between gap-3">
		<div class="max-w-xl space-y-2">
			<p class="text-sm/6 text-gray-400">
				Where {organization}'s agents run. HQ keeps the records and approves new offices; other offices keep a copy.
			</p>
			{#if sample}
				<!-- TEMPORARY: agent counts and placement come from the invented organization until the real snapshot is wired in. -->
				<p class="flex items-center gap-x-2 text-xs/5 text-gray-400" data-testid="sample-data-note">
					<Badge variant="secondary">Sample data</Badge> The agents below are invented, not yours.
				</p>
			{/if}
		</div>
		<div class="text-right">
			<!-- Phase 1 has HQ only; more offices arrive with the local satellite. -->
			<Button disabled aria-describedby="add-reason"><Plus aria-hidden="true" /> Add office</Button>
			<p id="add-reason" class="mt-1 text-xs/5 text-gray-400">Available in a later release.</p>
		</div>
	</div>

	<!-- Said once when the answer changes: the badge and count are hidden from screen readers, the row's label has them. -->
	<p class="sr-only" role="status" data-testid="live-status">{offices.find((o) => o.id === here.id)?.name ?? 'Office'}: {statusInfo(statusOf(here.id)).label}</p>

	<ul class="divide-y divide-white/5 overflow-hidden rounded-xl bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Offices">
		{#each offices as office (office.id)}
			{@const agents = snapshot ? agentsIn(snapshot, office.id).length : undefined}
			<li data-testid="office-row" data-office={office.id} class="flex items-center gap-x-4 px-4 py-3">
				<span class="flex h-6 w-10 shrink-0 items-center justify-center rounded text-xs font-semibold {isHq(office) ? 'bg-indigo-500/20 text-indigo-200' : 'text-transparent'}" aria-hidden="true">
					{isHq(office) ? 'HQ' : ''}
				</span>
				<button
					type="button"
					class="min-w-0 flex-1 text-left focus-visible:outline-2 focus-visible:outline-indigo-400"
					aria-label={rowLabel(office, agents, here.id === office.id, statusOf(office.id))}
					aria-haspopup="dialog"
					onclick={() => open(office.id)}
				>
					<span class="block truncate text-sm/6 font-semibold text-white" title={office.name}>{office.name}</span>
					<span class="block truncate text-xs/5 text-gray-400">{statusOf(office.id) === 'unreachable' ? "Can't reach this office" : secondaryText(office) + (here.id === office.id ? " (you're here)" : '')}</span>
				</button>
				<!-- The button's label already says the status and the count: not read twice. -->
				<span aria-hidden="true"><OfficeStatusBadge status={statusOf(office.id)} /></span>
				{#if statusOf(office.id) === 'unreachable' || (office.id === here.id && retryingProbe)}
					<!-- Stays mounted while it works (aria-disabled, not disabled), so keyboard focus isn't lost. -->
					<Button
						variant="outline"
						size="xs"
						data-testid="retry-office"
						class="aria-disabled:cursor-not-allowed aria-disabled:opacity-60"
						aria-label="{check === 'checking' ? 'Retrying…' : 'Retry'} reaching {office.name}"
						aria-disabled={check === 'checking'}
						aria-busy={check === 'checking'}
						onclick={() => check !== 'checking' && probe.run('explicit')}>{check === 'checking' ? 'Retrying…' : 'Retry'}</Button>
				{/if}
				<span class="w-20 text-right text-xs/5 text-gray-400" data-testid="agent-count" aria-hidden="true">
					{#if agents !== undefined}{agents} {agents === 1 ? 'agent' : 'agents'}
					{:else if countError}—
					{:else}<span class="inline-block h-4 w-14 animate-pulse rounded bg-white/10 align-middle motion-reduce:animate-none" data-testid="count-skeleton"></span>{/if}
				</span>
			</li>
		{:else}
			<li class="px-4 py-3 text-sm/6 text-gray-400" data-testid="no-offices">No offices to show.</li>
		{/each}
	</ul>

	{#if countError}
		<p class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg bg-red-500/10 px-4 py-3 text-sm/6 text-red-100 outline-1 -outline-offset-1 outline-red-400/20" role="alert" data-testid="count-error">
			<CircleAlert class="size-4 shrink-0" aria-hidden="true" />
			<span class="[overflow-wrap:anywhere]">Can't count agents right now: {countError}</span>
			<Button variant="outline" size="xs" class="aria-disabled:cursor-not-allowed aria-disabled:opacity-60" aria-disabled={retrying} aria-busy={retrying} onclick={() => !retrying && retry()}>{retrying ? 'Retrying…' : 'Retry'}</Button>
		</p>
	{/if}

	{#if unplaced.length > 0}
		<section aria-labelledby="unplaced" class="rounded-xl bg-amber-500/10 p-4 outline-1 -outline-offset-1 outline-amber-400/20" data-testid="unplaced-note">
			<h2 id="unplaced" class="text-sm/6 font-semibold text-amber-100">Unplaced · {unplaced.length} {unplaced.length === 1 ? 'agent' : 'agents'}</h2>
			<p class="text-xs/5 text-amber-100/80">These agents aren't placed in an office, so they don't run anywhere.</p>
			<ul class="mt-2 space-y-1">
				{#each unplaced as agent (agent.id)}
					<li class="flex items-center justify-between gap-x-3 text-sm/6 text-amber-50">
						<span class="min-w-0 [overflow-wrap:anywhere]">{agent.name}</span>
						<!-- Placing an agent is its own flow, which comes later: shown, not hidden, so nobody looks for it. -->
						<Button variant="outline" size="xs" disabled aria-label="Place… {agent.name}" aria-describedby="place-reason">Place…</Button>
					</li>
				{/each}
			</ul>
			<p id="place-reason" class="mt-2 text-xs/5 text-amber-100/80">Placing agents is available in a later release.</p>
		</section>
	{/if}
	<p class="text-xs/5 text-gray-500">Shown from this Mac.</p>
</div>

<OfficeDetailsSheet bind:open={detailsOpen} office={detailsFor} status={detailsFor ? statusOf(detailsFor.id) : 'status_unknown'} {snapshot} counting={!snapshot && !countError} {sample} here={detailsFor?.id === here.id} />
