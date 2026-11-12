<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import OfficeRepairLocation from '$lib/offices/OfficeRepairLocation.svelte';
	import { createProbe } from '$lib/offices/probe';
	import { invalidate } from '$app/navigation';
	import Plus from '@lucide/svelte/icons/plus';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import DividedRow from '$lib/components/DividedRow.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import OfficeDetailsSheet from '$lib/components/OfficeDetailsSheet.svelte';
	import OfficeStatusBadge from '$lib/components/OfficeStatusBadge.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { officeApi } from '$lib/office/api';
	import { agentsIn, applyStatusUpdate, displayStatusFor, isHq, orderOffices, repairReasonCopy, rowLabel, sampleRestartDeadlines, secondaryText, statusInfo, unplacedAgents, whyForLabel, whyStopped, type LiveCheck, type RestartDeadlineSample } from '$lib/offices/model';
	import type { OfficeInfo } from '$lib/proto';
	import type { Counts } from './+page';

	// Cap gutters only in a very narrow list; viewport breakpoints can leave little room at enlarged text.
	const rowGutters = '@max-[300px]:px-[min(1rem,5cqi)]';

	let { data } = $props();

	// The offices as the host last listed them; until it has, the ones saved with the connection.
	let reported = $state<OfficeInfo[] | undefined>();
	let reportedFor = '';
	// A relevant push or organization change invalidates pending reads. Only successful reads supersede older reads.
	let statusRevision = 0;
	let refreshSequence = 0;
	let successfulRefresh = 0;
	$effect(() => {
		if (reportedFor !== data.connection.organization.id) {
			reportedFor = data.connection.organization.id;
			statusRevision += 1;
			reported = undefined;
		}
	});
	const current = $derived(reported ?? data.offices);
	const offices = $derived(orderOffices(current));
	const here = $derived(data.connection.office);
	// The agent counts arrive after the list: `undefined` while they are being counted.
	let counts = $state<Counts | undefined>();
	let organizationOfCounts = '';
	let retrying = $state(false);
	// True from the first failure until the office answers again: the Retry button stays while it works.
	let retryingProbe = $state(false);
	$effect(() => {
		if (check === 'down') retryingProbe = true;
		else if (check === 'up') retryingProbe = false;
	});
	$effect(() => {
		const pending = data.counts;
		// The last answer stays only within one organization: a new one starts uncounted (no old agents under Unplaced).
		if (organizationOfCounts !== data.connection.organization.id) {
			organizationOfCounts = data.connection.organization.id;
			counts = undefined;
		}
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
	const unplaced = $derived(snapshot ? unplacedAgents(snapshot, current) : []);
	const organization = $derived(data.connection.organization.name);

	// The one live signal there is: whether the office this app is connected to answers. Checked now, every
	// half minute and on Retry; a background check keeps the last answer instead of flashing "Checking…".
	let check = $state<LiveCheck>('checking');
	// What the host said it can do in its last hello (`office.status_changed`, `office.restart`).
	let capabilities = $state<string[] | undefined>();
	/**
	 * The statuses the host reports are not the ones saved when the organization was opened: list them again. Read-only: it
	 * remembers nothing and does not change which organization is open (an unavailable HQ is listed too).
	 */
	async function refreshOffices() {
		const id = data.connection.organization.id;
		const revision = statusRevision;
		const request = ++refreshSequence;
		try {
			const found = (await officeApi.peekOrganizations()).find((o) => o.id === id);
			if (id === data.connection.organization.id && revision === statusRevision && request > successfulRefresh) {
				// A successful empty result also fences older reads, preserving the absence it reported.
				successfulRefresh = request;
				if (found) reported = found.offices;
			}
		} catch {
			// The next tick tries again; the probe already says whether the host answers.
		}
	}
	const probe = createProbe(officeApi, (next, caps) => {
		check = next;
		if (next === 'up') {
			capabilities = caps;
			void refreshOffices();
		}
		// A Retry that worked removes the button the keyboard was on: move focus to the row instead of dropping it.
		if (next === 'up' && document.activeElement?.matches('[data-testid=retry-office]')) rowButton(here.id)?.focus();
	});
	const rowButton = (officeId: string) => document.querySelector<HTMLElement>(`[data-office="${officeId}"] button[aria-haspopup]`);
// A new deadline gets an immediate sample, which anchors its stable live-status announcement.
let deadlineSamples = $state.raw<ReadonlyMap<string, RestartDeadlineSample>>(new Map());
let countdownClock = $state(Date.now());
// Capture the new deadline before rendering its visible and accessible text.
$effect.pre(() => {
	deadlineSamples = sampleRestartDeadlines(current, untrack(() => deadlineSamples), Date.now());
});
	onMount(() => {
		// The host says when an office's status changes: show it now instead of at the next check.
		let stopListening: (() => void) | undefined;
		let leaving = false;
		void officeApi
			.onOfficeStatus((update) => {
				const before = reported ?? data.offices;
				const next = applyStatusUpdate(before, data.connection.organization.id, update);
				// An unrelated organization's single-office push returns the same list.
				if (next !== before) statusRevision += 1;
				reported = next;
			})
			.then((stop) => {
				if (leaving) stop();
				else stopListening = stop;
			})
			.catch(() => {
				// No live updates then: the check every half minute still lists the statuses.
			});
		void probe.run('explicit');
		const again = setInterval(() => void probe.run('background'), 30_000);
		return () => (clearInterval(again), (leaving = true), stopListening?.());
	});
	$effect(() => {
		const deadlines = Array.from(deadlineSamples.values(), ({ deadline }) => Date.parse(deadline));
		let timer: ReturnType<typeof setTimeout> | undefined;
		function tick() {
			const now = Date.now();
			countdownClock = now;
			const delay = Math.min(...deadlines
				.filter((deadline) => deadline > now)
				.map((deadline) => (deadline - now) % 10_000 + 1));
			// Cross the exact deadline boundary by 1 ms before displaying the next lower step.
			if (Number.isFinite(delay)) timer = setTimeout(tick, delay);
		}
		tick();
		return () => clearTimeout(timer);
	});
	// Why a stopped office stopped: only for an office whose status is shown as reported by the host.
const deadlineObservedAt = (office: OfficeInfo) => {
	const sample = deadlineSamples.get(office.id);
	return sample && sample.deadline === office.next_restart_at ? sample.observedAt : undefined;
};
	const labelWhyOf = (officeId: string) => {
		const shown = statusOf(officeId);
		const office = current.find((o) => o.id === officeId);
	return office && (shown === 'offline' || shown === 'unavailable')
		? whyForLabel(office, new Date(deadlineObservedAt(office) ?? Date.now()))
		: undefined;
	};
	const whyOf = (officeId: string) => {
		const shown = statusOf(officeId);
		const office = current.find((o) => o.id === officeId);
	if (!office || (shown !== 'offline' && shown !== 'unavailable')) return undefined;
	return whyStopped(office, new Date(Math.max(countdownClock, deadlineObservedAt(office) ?? countdownClock)));
	};
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
	const statusOf = (officeId: string) => displayStatusFor(officeId, here.id, check, current.find((o) => o.id === officeId)?.status, capabilities);

	let detailsId = $state<string | undefined>();
	// The open sheet shows the latest data for its office, not the copy captured when it was clicked.
	const detailsFor = $derived(current.find((o) => o.id === detailsId));
	let detailsOpen = $state(false);
	let detailsOpener: HTMLButtonElement | undefined;
	$effect(() => {
		if (detailsOpen && !detailsFor) detailsOpen = false; // the office went away: close its sheet
	});

	function open(id: string, opener: HTMLButtonElement) {
		detailsOpener = opener;
		detailsId = id;
		detailsOpen = true;
	}

	function restoreDetailsFocus(event: Event) {
		const opener = detailsOpener;
		detailsOpener = undefined;
		if (!opener?.isConnected) return;
		event.preventDefault();
		opener.focus({ preventScroll: true });
	}
</script>

<svelte:head><title>Offices · Agentic Org</title></svelte:head>

<div class="space-y-5 px-4 py-6 sm:px-6 lg:px-8">
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
			<Button variant="outline" disabled aria-describedby="add-reason"><Plus aria-hidden="true" /> Add office</Button>
			<p id="add-reason" class="mt-1 text-xs/5 text-gray-400">Available in a later release.</p>
		</div>
	</div>

	<!-- Said once when the answer changes: the badge and count are hidden from screen readers, the row's label has them. -->
	<p class="sr-only" role="status" data-testid="live-status">{offices.find((o) => o.id === here.id)?.name ?? 'Office'}: {statusInfo(statusOf(here.id)).label}{labelWhyOf(here.id) ? `: ${labelWhyOf(here.id)}` : ''}</p>

	<ul class="@container -mx-4 sm:-mx-6 lg:-mx-8" aria-label="Offices">
		{#each offices as office (office.id)}
			{@const agents = snapshot ? agentsIn(snapshot, office.id).length : undefined}
			<DividedRow data-testid="office-row" data-office={office.id} class="{rowGutters} flex flex-wrap items-center gap-x-4 gap-y-2 hover:bg-white/[0.025]">
				<button
					type="button"
					class="min-w-0 flex-1 basis-32 text-left [&:focus-visible>.absolute]:outline-2 [&:focus-visible>.absolute]:outline-solid [&:focus-visible>.absolute]:-outline-offset-2 [&:focus-visible>.absolute]:outline-indigo-400 max-[639px]:order-first max-[639px]:w-full max-[639px]:flex-none"
					aria-label={rowLabel(office, agents, here.id === office.id, statusOf(office.id), whyOf(office.id))}
					aria-haspopup="dialog"
					onclick={(event) => open(office.id, event.currentTarget)}
				>
					<span class="block truncate text-sm/6 font-semibold text-white" title={office.name}>{office.name}{#if isHq(office) && office.name !== 'HQ'}<span class="font-normal text-gray-400">{' · HQ'}</span>{/if}</span>
					<span class="mt-3 inline-block max-w-full text-xs/5 text-gray-400 [overflow-wrap:anywhere]" title={whyOf(office.id)}>
						{#if office.status_reason === 'unsupported_version' && whyOf(office.id)}
							{repairReasonCopy.before}<OfficeRepairLocation {office} />{repairReasonCopy.after}
						{:else}{statusOf(office.id) === 'unreachable' ? "Can't reach this office" : (whyOf(office.id) ?? secondaryText(office) + (here.id === office.id ? " (you're here)" : ''))}{/if}
					</span>
					<span class="mx-2 text-xs/5 text-gray-400" aria-hidden="true">·</span>
					<span class="text-xs/5 text-gray-400" data-testid="agent-count" aria-hidden="true">
						{#if agents !== undefined}{agents} {agents === 1 ? 'agent' : 'agents'}
						{:else if countError}—
						{:else}<span class="inline-block h-4 w-14 animate-pulse rounded bg-white/10 align-middle motion-reduce:animate-none" data-testid="count-skeleton"></span>{/if}
					</span>
					<span class="absolute inset-0" aria-hidden="true"></span>
				</button>
				<!-- The button's label already says the status and the count: not read twice. -->
				<span class="inline-flex min-w-0 max-w-full items-center gap-x-4 @max-[300px]:flex-wrap @max-[300px]:justify-end @max-[300px]:gap-y-2" aria-hidden="true"><OfficeStatusBadge status={statusOf(office.id)} /><ChevronRight class="size-5 shrink-0 text-gray-400" aria-hidden="true" /></span>
				{#if statusOf(office.id) === 'unreachable' || (office.id === here.id && retryingProbe)}
					<!-- Stays mounted while it works (aria-disabled, not disabled), so keyboard focus isn't lost. -->
					<Button
						variant="outline"
						size="xs"
						data-testid="retry-office"
						class="relative z-10 aria-disabled:cursor-not-allowed aria-disabled:opacity-60"
						aria-label="{check === 'checking' ? 'Retrying…' : 'Retry'} reaching {office.name}"
						aria-disabled={check === 'checking'}
						aria-busy={check === 'checking'}
						onclick={() => check !== 'checking' && probe.run('explicit')}>{check === 'checking' ? 'Retrying…' : 'Retry'}</Button>
				{/if}
			</DividedRow>
		{:else}
			<DividedRow class="{rowGutters} py-12 text-center text-sm/6 text-gray-400" data-testid="no-offices">No offices to show.</DividedRow>
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
		<section aria-labelledby="unplaced" class="space-y-3" data-testid="unplaced-note">
			<h2 id="unplaced" class="text-xs/6 font-semibold text-gray-400">Unplaced · {unplaced.length} {unplaced.length === 1 ? 'agent' : 'agents'}</h2>
			<Callout data-testid="unplaced-explanation">
				{#snippet icon()}<CircleAlert class="size-4 text-amber-400" aria-hidden="true" />{/snippet}
				<p class="text-sm/6">These agents aren't placed in an office, so they don't run anywhere.</p>
			</Callout>
			<ul class="@container -mx-4 sm:-mx-6 lg:-mx-8">
				{#each unplaced as agent (agent.id)}
					<DividedRow class="{rowGutters} flex items-center justify-between gap-x-3 text-sm/6 text-white">
						<span class="min-w-0 [overflow-wrap:anywhere]">{agent.name}</span>
						<!-- Placing an agent is its own flow, which comes later: shown, not hidden, so nobody looks for it. -->
						<Button variant="outline" size="xs" disabled aria-label="Place… {agent.name}" aria-describedby="place-reason">Place…</Button>
					</DividedRow>
				{/each}
			</ul>
			<p id="place-reason" class="text-xs/5 text-gray-400">Placing agents is available in a later release.</p>
		</section>
	{/if}
	<p class="text-xs/5 text-gray-400">Shown from HQ. Statuses include last-seen information.</p>
</div>

<OfficeDetailsSheet bind:open={detailsOpen} office={detailsFor} status={detailsFor ? statusOf(detailsFor.id) : 'status_unknown'} why={detailsFor ? whyOf(detailsFor.id) : undefined} {capabilities} onrestarted={refreshOffices} onCloseAutoFocus={restoreDetailsFocus} {snapshot} counting={!snapshot && !countError} {sample} here={detailsFor?.id === here.id} />
