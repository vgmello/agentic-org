<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import OfficeRepairLocation from '$lib/offices/OfficeRepairLocation.svelte';
	import { createProbe } from '$lib/offices/probe';
	import { invalidate } from '$app/navigation';
	import Plus from '@lucide/svelte/icons/plus';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import DividedRow from '$lib/components/DividedRow.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import OfficeDetailsSheet from '$lib/components/OfficeDetailsSheet.svelte';
	import OfficeStatusBadge from '$lib/components/OfficeStatusBadge.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { toast } from 'svelte-sonner';
	import { OffboardAgentDialog } from '$lib/modules/employees';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { neighbourFocus } from '$lib/components/focus';
	import { Resource } from '$lib/components/resource.svelte';
	import { officesApi } from '$lib/modules/offices/api';
	import { layerOffices, satelliteWhere } from '$lib/modules/offices/layer';
	import { withPlacements } from '$lib/modules/offices/placements';
	import AddOfficeDialog from '$lib/modules/offices/features/add-office/AddOfficeDialog.svelte';
	import InvitationRows from '$lib/modules/offices/features/add-office/InvitationRows.svelte';
	import PendingAddRows from '$lib/modules/offices/features/add-office/PendingAddRows.svelte';
	import IssuedDialog from '$lib/modules/offices/features/add-office/IssuedDialog.svelte';
	import ApproveJoinDialog from '$lib/modules/offices/features/approve-join/ApproveJoinDialog.svelte';
	import JoinRequestRow from '$lib/modules/offices/features/approve-join/JoinRequestRow.svelte';
	import OutboxList from '$lib/modules/offices/features/outbox/OutboxList.svelte';
	import RemoveOfficeDialog from '$lib/modules/offices/features/remove-office/RemoveOfficeDialog.svelte';
	import PlaceAgentDialog from '$lib/modules/offices/features/unplaced/PlaceAgentDialog.svelte';
	import UnplacedAgents from '$lib/modules/offices/features/unplaced/UnplacedAgents.svelte';
	import { orgApi } from '$lib/org/api';
	import { sessionFor } from '$lib/org/session';
	import type {
		Issued,
		Invitation,
		JoinRequest,
		OutboxItem,
		PendingAdd
	} from '$lib/modules/offices/types';
	import type { Member } from '$lib/proto';
	import { officeApi, toAppError } from '$lib/office/api';
	import {
		agentsIn,
		applyStatusUpdate,
		displayStatusFor,
		isHq,
		orderOffices,
		repairReasonCopy,
		rowLabel,
		sampleRestartDeadlines,
		secondaryText,
		statusInfo,
		unplacedAgents,
		whyForLabel,
		whyStopped,
		type LiveCheck,
		type RestartDeadlineSample
	} from '$lib/offices/model';
	import type { OfficeInfo } from '$lib/proto';
	import type { Counts } from './+page';

	// 21rem reserves 4rem for 6ch of name, 9.375rem for the widest pill (7rem label +
	// 2.375rem icon/gap/padding), 1.25rem chevron, 2rem gaps and 4rem gutters, rounded up.
	// Scale with the list font so enlarged text cannot leave a tiny name or an icon-only pill.
	const compactRow = {
		gutters: '@max-[21rem]:px-[min(1rem,5cqi)]',
		name: '@max-[21rem]:w-full @max-[21rem]:flex-none',
		gap: '@max-[21rem]:gap-x-2',
		chevron: '@max-[21rem]:ml-0',
		bleed: '@max-[21rem]:-mx-[min(1rem,5cqi)]'
	};

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
	// What joined, who wants to join and what is queued: layered beside the connection's own offices (the host lists no others yet).
	const extra = new Resource(() => officesApi.overview(data.connection.organization.id));
	const overview = $derived(extra.data);
	const current = $derived(layerOffices(reported ?? data.offices, overview));
	let now = $state(Date.now());
	$effect(() => {
		const tick = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(tick);
	});
	const satelliteIds = $derived(new Set(overview?.satellites.map((o) => o.id)));
	const detailsOf = (id: string) => overview?.details[id];
	/** An office that joined, as its row words it: where it is and when it was last seen. */
	const shownOffice = (o: OfficeInfo): OfficeInfo =>
		satelliteIds.has(o.id)
			? { ...o, host: satelliteWhere(o, detailsOf(o.id), now), endpoint: undefined }
			: o;
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
	const rawSnapshot = $derived(counts && 'snapshot' in counts ? counts.snapshot : undefined);
	const snapshot = $derived(
		rawSnapshot ? withPlacements(rawSnapshot, overview?.placements ?? {}) : undefined
	);
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
			if (
				id === data.connection.organization.id &&
				revision === statusRevision &&
				request > successfulRefresh
			) {
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
		if (next === 'up' && document.activeElement?.matches('[data-testid=retry-office]'))
			rowButton(here.id)?.focus();
	});
	const rowButton = (officeId: string) =>
		document.querySelector<HTMLElement>(`[data-office="${officeId}"] button[aria-haspopup]`);
	// A new deadline gets an immediate sample, which anchors its stable live-status announcement.
	let deadlineSamples = $state.raw<ReadonlyMap<string, RestartDeadlineSample>>(new Map());
	let countdownClock = $state(Date.now());
	// Capture the new deadline before rendering its visible and accessible text.
	$effect.pre(() => {
		deadlineSamples = sampleRestartDeadlines(
			current,
			untrack(() => deadlineSamples),
			Date.now()
		);
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
			const delay = Math.min(
				...deadlines
					.filter((deadline) => deadline > now)
					.map((deadline) => ((deadline - now) % 10_000) + 1)
			);
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
		return whyStopped(
			office,
			new Date(Math.max(countdownClock, deadlineObservedAt(office) ?? countdownClock))
		);
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
	// An office that joined is reported by HQ (not by this connection), so its saved status is its status.
	const statusOf = (officeId: string) =>
		satelliteIds.has(officeId)
			? (current.find((o) => o.id === officeId)?.status ?? 'unknown')
			: displayStatusFor(
					officeId,
					here.id,
					check,
					current.find((o) => o.id === officeId)?.status,
					capabilities
				);

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

	// ---- add, approve, remove, rename, place and the outboxes (offices-and-teams-ux §6) ----------------------------------
	const orgId = $derived(data.connection.organization.id);
	const session = $derived(sessionFor(orgApi, orgId));
	const reload = async () => {
		await extra.reload();
	};
	/** One overlay at a time: whatever opened it gets focus back when it closes. */
	let lastOpener: HTMLElement | undefined;
	function restoreOpener(event: Event) {
		const opener = lastOpener;
		lastOpener = undefined;
		if (!opener?.isConnected) return;
		event.preventDefault();
		opener.focus({ preventScroll: true });
	}
	const via = (opener: HTMLElement | undefined) => (lastOpener = opener);

	let addOpen = $state(false);
	let resumeAdd = $state<PendingAdd | undefined>();
	let approveOpen = $state(false);
	let approving = $state<JoinRequest | undefined>();
	let cancelInviteOpen = $state(false);
	let cancelling = $state<Invitation | undefined>();
	let renewOpen = $state(false);
	let renewing = $state<Invitation | undefined>();
	let issuedOpen = $state(false);
	let issued = $state<Issued | undefined>();
	let stopOpen = $state(false);
	let stopping = $state<PendingAdd | undefined>();
	let removeOpen = $state(false);
	let removing = $state<OfficeInfo | undefined>();
	let placeOpen = $state(false);
	let placing = $state<Member | undefined>();
	let offboardOpen = $state(false);
	let offboarding = $state<Member | undefined>();
	let discardOpen = $state(false);
	let discarding = $state<OutboxItem | undefined>();
	let retryingItem = $state<string | undefined>();

	const canAddOnInstance = $derived(overview?.viewer.isAdministrator ?? false);
	const placeable = $derived(
		orderOffices(current)
			.filter((o) => o.status !== 'pending')
			.map((o) => ({ id: o.id, name: o.name }))
	);

	async function issue(invitation: Invitation) {
		issued = await officesApi.newInvitation(orgId, invitation.id);
		issuedOpen = true;
		await reload();
	}
	function renew(invitation: Invitation, opener: HTMLElement) {
		via(opener);
		renewing = invitation;
		const live = invitation.state === 'invited' && Date.parse(invitation.expiresAt) > now;
		if (live) renewOpen = true;
		else void issue(invitation).catch((e) => toast.error(toAppError(e).message));
	}
	async function removeInvitation(invitation: Invitation, opener: HTMLElement) {
		const focusNext = neighbourFocus(opener);
		try {
			await officesApi.removeInvitation(orgId, invitation.id);
			await reload();
			toast.success(`Removed the invitation for ${invitation.officeName}.`);
			await focusNext();
		} catch (e) {
			toast.error(toAppError(e).message);
		}
	}
	async function retryItem(item: OutboxItem) {
		retryingItem = item.id;
		try {
			await officesApi.retryOutbox(orgId, item.id);
			await reload();
			toast.success('Sent again.');
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			retryingItem = undefined;
		}
	}
	function removeOfficeFromSheet() {
		const office = detailsFor;
		if (!office) return;
		removing = office;
		detailsOpen = false;
		removeOpen = true;
	}
	const unplacedHeading = () => document.getElementById('unplaced');

	function restoreDetailsFocus(event: Event) {
		const opener = detailsOpener;
		detailsOpener = undefined;
		if (!opener?.isConnected) return;
		event.preventDefault();
		opener.focus({ preventScroll: true });
	}
</script>

<svelte:head><title>Offices · Organization settings · Agentic Org</title></svelte:head>

<PageHeader
	title="Offices"
	count={offices.length}
	countLabel={offices.length === 1 ? 'office' : 'offices'}
	description="Where {organization}'s agents run. HQ keeps the records and approves new offices; other offices keep a copy."
>
	{#snippet actions()}
		<Button
			disabled={!overview}
			aria-describedby={extra.state === 'error' ? 'overview-error' : undefined}
			onclick={(e) => (via(e.currentTarget), (resumeAdd = undefined), (addOpen = true))}
			><Plus aria-hidden="true" /> Add office</Button
		>
	{/snippet}
</PageHeader>

<!-- The outer width governs both header gutters and the full-width divided rows. -->
<div class="@container">
	<div class="{compactRow.gutters} space-y-5 px-4 py-6 sm:px-6 lg:px-8">
		{#if sample}
			<!-- TEMPORARY: agent counts and placement come from the invented organization until the real snapshot is wired in. -->
			<p class="flex items-center gap-x-2 text-xs/5 text-fg-muted" data-testid="sample-data-note">
				<Badge variant="secondary">Sample data</Badge> The agents below are invented, not yours.
			</p>
		{/if}

		{#if extra.state === 'error'}
			<p
				id="overview-error"
				class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg bg-danger-emphasis/10 px-4 py-3 text-sm/6 text-danger-text-stronger outline-1 -outline-offset-1 outline-danger/20"
				role="alert"
				data-testid="overview-error"
			>
				<CircleAlert class="size-4 shrink-0" aria-hidden="true" />
				<span class="[overflow-wrap:anywhere]"
					>Can't read join requests, invitations and offices that joined: {extra.message}</span
				>
				<Button
					variant="outline"
					size="xs"
					aria-busy={extra.reloading}
					aria-disabled={extra.reloading}
					onclick={() => !extra.reloading && extra.reload()}
					>{extra.reloading ? 'Retrying…' : 'Retry'}</Button
				>
			</p>
		{/if}

		{#if overview && overview.joinRequests.length > 0}
			<ul class="{compactRow.bleed} -mx-4 sm:-mx-6 lg:-mx-8" aria-label="Join requests">
				{#each overview.joinRequests as request (request.id)}
					<JoinRequestRow
						{request}
						{now}
						onreview={(button) => (via(button), (approving = request), (approveOpen = true))}
					/>
				{/each}
			</ul>
		{/if}

		<!-- Said once when the answer changes: the badge and count are hidden from screen readers, the row's label has them. -->
		<p class="sr-only" role="status" data-testid="live-status">
			{offices.find((o) => o.id === here.id)?.name ?? 'Office'}: {statusInfo(statusOf(here.id))
				.label}{labelWhyOf(here.id) ? `: ${labelWhyOf(here.id)}` : ''}
		</p>

		<ul class="{compactRow.bleed} -mx-4 sm:-mx-6 lg:-mx-8" aria-label="Offices">
			{#each offices as office (office.id)}
				{@const agents = snapshot ? agentsIn(snapshot, office.id).length : undefined}
				<DividedRow
					data-testid="office-row"
					data-office={office.id}
					class="{compactRow.gutters} {compactRow.gap} flex flex-wrap items-center justify-end gap-x-4 gap-y-2 hover:bg-tint/[0.025]"
				>
					<button
						type="button"
						class="{compactRow.name} min-w-[min(6ch,100%)] flex-1 basis-[6ch] text-left outline-none [&:focus-visible>.absolute]:outline-2 [&:focus-visible>.absolute]:outline-solid [&:focus-visible>.absolute]:-outline-offset-2 [&:focus-visible>.absolute]:outline-focus max-[639px]:order-first max-[639px]:w-full max-[639px]:flex-none"
						aria-label={rowLabel(
							shownOffice(office),
							agents,
							here.id === office.id,
							statusOf(office.id),
							whyOf(office.id)
						)}
						aria-haspopup="dialog"
						onclick={(event) => open(office.id, event.currentTarget)}
					>
						<span class="block truncate text-sm/6 font-semibold text-fg" title={office.name}
							>{office.name}{#if isHq(office) && office.name !== 'HQ'}<span
									class="font-normal text-fg-muted">{' · HQ'}</span
								>{/if}</span
						>
						<span class="mt-3 flex flex-wrap items-baseline gap-x-2 text-xs/5 text-fg-muted">
							<span
								class="max-w-full [overflow-wrap:anywhere]"
								title={whyOf(office.id)}
								data-testid="office-location"
							>
								{#if office.status_reason === 'unsupported_version' && whyOf(office.id)}
									{repairReasonCopy.before}<OfficeRepairLocation {office} />{repairReasonCopy.after}
								{:else}{statusOf(office.id) === 'unreachable'
										? "Can't reach this office"
										: (whyOf(office.id) ??
											secondaryText(shownOffice(office)) +
												(here.id === office.id ? " (you're here)" : ''))}{/if}
							</span>
							<span data-testid="agent-count" aria-hidden="true">
								{#if agents !== undefined}{agents} {agents === 1 ? 'agent' : 'agents'}
								{:else if countError}—
								{:else}<span
										class="inline-block h-4 w-14 animate-pulse rounded bg-surface-overlay align-middle motion-reduce:animate-none"
										data-testid="count-skeleton"
									></span>{/if}
							</span>
						</span>
						<span class="absolute inset-0" aria-hidden="true"></span>
					</button>
					<!-- The button's label already says the status and the count: not read twice. -->
					<span
						class="ml-auto inline-flex max-w-full flex-none flex-wrap items-center justify-end gap-x-2 gap-y-2"
						data-testid="office-actions"
					>
						<span class="contents" aria-hidden="true"
							><OfficeStatusBadge status={statusOf(office.id)} /></span
						>
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
								onclick={() => check !== 'checking' && probe.run('explicit')}
								>{check === 'checking' ? 'Retrying…' : 'Retry'}</Button
							>
						{/if}
						<ChevronRight
							class="{compactRow.chevron} ml-2 size-5 shrink-0 text-fg-muted"
							aria-hidden="true"
						/>
					</span>
				</DividedRow>
			{:else}
				<DividedRow
					class="{compactRow.gutters} py-12 text-center text-sm/6 text-fg-muted"
					data-testid="no-offices">No offices to show.</DividedRow
				>
			{/each}
		</ul>

		{#if overview && (overview.pendingAdds.length > 0 || overview.invitations.length > 0)}
			<ul
				class="{compactRow.bleed} -mx-4 sm:-mx-6 lg:-mx-8"
				aria-label="Invitations and offices being added"
			>
				<PendingAddRows
					pending={overview.pendingAdds}
					onopen={(add, button) => (via(button), (resumeAdd = add), (addOpen = true))}
					oncancel={(add, button) => (via(button), (stopping = add), (stopOpen = true))}
				/>
				<InvitationRows
					invitations={overview.invitations}
					{now}
					oncancel={(invitation, button) => (
						via(button),
						(cancelling = invitation),
						(cancelInviteOpen = true)
					)}
					onrenew={renew}
					onremove={removeInvitation}
				/>
			</ul>
		{/if}

		{#each offices.filter((o) => o.status === 'update_needed' && satelliteIds.has(o.id)) as stale (stale.id)}
			<p class="text-xs/5 text-fg-muted" data-testid="version-note">
				{stale.name} runs an older version of Agentic Org, so new features wait until it updates.
			</p>
		{/each}
		{#if overview && offices.length === 1 && overview.invitations.length === 0 && overview.pendingAdds.length === 0 && overview.joinRequests.length === 0}
			<p class="text-xs/5 text-fg-muted" data-testid="hq-only-hint">
				Add an office to run agents on another machine, or to keep a copy of the records there.
			</p>
		{/if}

		{#if countError}
			<p
				class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg bg-danger-emphasis/10 px-4 py-3 text-sm/6 text-danger-text-stronger outline-1 -outline-offset-1 outline-danger/20"
				role="alert"
				data-testid="count-error"
			>
				<CircleAlert class="size-4 shrink-0" aria-hidden="true" />
				<span class="[overflow-wrap:anywhere]">Can't count agents right now: {countError}</span>
				<Button
					variant="outline"
					size="xs"
					class="aria-disabled:cursor-not-allowed aria-disabled:opacity-60"
					aria-disabled={retrying}
					aria-busy={retrying}
					onclick={() => !retrying && retry()}>{retrying ? 'Retrying…' : 'Retry'}</Button
				>
			</p>
		{/if}

		{#if unplaced.length > 0}
			<UnplacedAgents
				agents={unplaced}
				bleed={compactRow.bleed}
				gutters={compactRow.gutters}
				onplace={(agent, button) => (via(button), (placing = agent), (placeOpen = true))}
				onoffboard={(agent, button) => (via(button), (offboarding = agent), (offboardOpen = true))}
			/>
		{/if}

		{#if overview && overview.outbox.length > 0}
			<OutboxList
				items={overview.outbox}
				{now}
				bleed={compactRow.bleed}
				gutters={compactRow.gutters}
				retrying={retryingItem}
				onretry={(item) => retryItem(item)}
				ondiscard={(item, button) => (via(button), (discarding = item), (discardOpen = true))}
			/>
		{/if}
		<p class="text-xs/5 text-fg-muted">Shown from HQ. Statuses include last-seen information.</p>
	</div>
</div>

<OfficeDetailsSheet
	bind:open={detailsOpen}
	office={detailsFor}
	status={detailsFor ? statusOf(detailsFor.id) : 'status_unknown'}
	why={detailsFor ? whyOf(detailsFor.id) : undefined}
	{capabilities}
	onrestarted={refreshOffices}
	onCloseAutoFocus={restoreDetailsFocus}
	{snapshot}
	counting={!snapshot && !countError}
	{sample}
	here={detailsFor?.id === here.id}
	details={detailsFor ? detailsOf(detailsFor.id) : undefined}
	onrename={detailsFor
		? async (name) => {
				await officesApi.renameOffice(orgId, detailsFor.id, name);
				await reload();
			}
		: undefined}
	onremove={detailsFor && !isHq(detailsFor) && overview ? removeOfficeFromSheet : undefined}
/>

<AddOfficeDialog
	bind:open={addOpen}
	organizationId={orgId}
	{canAddOnInstance}
	resume={resumeAdd}
	onchanged={reload}
	onCloseAutoFocus={restoreOpener}
/>
<ApproveJoinDialog
	bind:open={approveOpen}
	request={approving}
	organizationId={orgId}
	organizationName={organization}
	onchanged={reload}
	onCloseAutoFocus={restoreOpener}
/>
<ConfirmDialog
	bind:open={cancelInviteOpen}
	title={`Cancel the invitation for ${cancelling?.officeName ?? ''}?`}
	description="Its link stops working. Nothing is added."
	confirmLabel="Cancel invitation"
	busyLabel="Cancelling…"
	cancelLabel="Keep invitation"
	onCloseAutoFocus={restoreOpener}
	onconfirm={async () => {
		if (!cancelling) return;
		const focusNext = neighbourFocus(lastOpener);
		await officesApi.cancelInvitation(orgId, cancelling.id);
		await reload();
		lastOpener = undefined;
		await focusNext();
	}}
/>
<ConfirmDialog
	bind:open={renewOpen}
	title={`Make a new invitation for ${renewing?.officeName ?? ''}?`}
	description="The previous invitation stops working. If a computer already used it, its request is cancelled."
	confirmLabel="New invitation"
	busyLabel="Creating…"
	cancelLabel="Keep the current one"
	onCloseAutoFocus={(e) => (issuedOpen ? e.preventDefault() : restoreOpener(e))}
	onconfirm={async () => {
		if (renewing) await issue(renewing);
	}}
/>
<IssuedDialog bind:open={issuedOpen} {issued} onCloseAutoFocus={restoreOpener} />
<ConfirmDialog
	bind:open={stopOpen}
	title={`Stop adding ${stopping?.name ?? ''}?`}
	description="It wasn't added yet."
	confirmLabel="Stop adding"
	busyLabel="Stopping…"
	cancelLabel="Keep waiting"
	onCloseAutoFocus={restoreOpener}
	onconfirm={async () => {
		if (!stopping) return;
		const cancelled = await officesApi.cancelAdd(orgId, stopping.id);
		if (!cancelled)
			throw {
				source: 'app',
				code: 'too_late',
				message: `Too late to cancel: ${stopping.name} is being added now.`
			};
		await reload();
		toast.success(`${stopping.name} wasn't added.`);
	}}
/>
<RemoveOfficeDialog
	bind:open={removeOpen}
	office={removing}
	organizationId={orgId}
	organizationName={organization}
	agents={removing && snapshot ? agentsIn(snapshot, removing.id).length : 0}
	tasks={removing ? (detailsOf(removing.id)?.tasks ?? 0) : 0}
	offline={removing
		? statusOf(removing.id) === 'offline' || statusOf(removing.id) === 'unreachable'
		: false}
	onchanged={async () => {
		await reload();
		toast.success(`${removing?.name ?? 'The office'} was removed.`);
		await new Promise((r) => setTimeout(r, 0));
		unplacedHeading()?.focus();
	}}
	onCloseAutoFocus={(e) => (e.preventDefault(), rowButton(here.id)?.focus())}
/>
<PlaceAgentDialog
	bind:open={placeOpen}
	agent={placing}
	offices={placeable}
	organizationId={orgId}
	onchanged={reload}
	onCloseAutoFocus={restoreOpener}
/>
<OffboardAgentDialog
	bind:open={offboardOpen}
	agent={offboarding}
	snapshot={snapshot ?? { position: { epoch: 0, seq: 0 }, teams: [], members: [], memberships: [] }}
	{session}
	ondone={async (result) => {
		if ('name' in result) toast.success(`${result.name} was offboarded.`);
		await invalidate('app:offices');
	}}
/>
<ConfirmDialog
	bind:open={discardOpen}
	title="Discard this command?"
	description={discarding
		? `${discarding.description} won't be sent to ${discarding.officeName}.`
		: ''}
	confirmLabel="Discard"
	busyLabel="Discarding…"
	cancelLabel="Keep it"
	destructive
	onCloseAutoFocus={restoreOpener}
	onconfirm={async () => {
		if (!discarding) return;
		const focusNext = neighbourFocus(lastOpener);
		await officesApi.discardOutbox(orgId, discarding.id);
		await reload();
		lastOpener = undefined;
		await focusNext();
	}}
/>
