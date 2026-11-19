<!--
  @component
  Organization settings › Offices (offices-and-teams-ux §6): every office of the organization with its live status, where
  it runs and its agents; join requests, invitations and offices being added; agents left unplaced; and what offices
  could not deliver. Add office, approve, remove, rename, place and the outbox go through `officesApi`; the connected
  office's own status, restart and repair go through the office API (§3.3).
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { invalidate } from '$app/navigation';
	import Plus from '@lucide/svelte/icons/plus';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import { toast } from 'svelte-sonner';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import DividedRow from '$lib/components/DividedRow.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { neighbourFocus } from '$lib/components/focus';
	import { Resource } from '$lib/components/resource.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { OffboardAgentDialog } from '$lib/modules/employees';
	import { toAppError } from '$lib/office/api';
	import { agentsIn, isHq, orderOffices, statusInfo, unplacedAgents } from '$lib/offices/model';
	import { orgApi } from '$lib/org/api';
	import { sessionFor } from '$lib/org/session';
	import type { Connection, Member, OfficeInfo } from '$lib/proto';
	import { officesApi } from '../../api';
	import { layerOffices, satelliteWhere } from '../../layer';
	import { withPlacements } from '../../placements';
	import type { Invitation, Issued, JoinRequest, OutboxItem, PendingAdd } from '../../types';
	import AddOfficeDialog from '../add-office/AddOfficeDialog.svelte';
	import InvitationRows from '../add-office/InvitationRows.svelte';
	import IssuedDialog from '../add-office/IssuedDialog.svelte';
	import PendingAddRows from '../add-office/PendingAddRows.svelte';
	import ApproveJoinDialog from '../approve-join/ApproveJoinDialog.svelte';
	import JoinRequestRow from '../approve-join/JoinRequestRow.svelte';
	import OfficeDetailsSheet from '../details/OfficeDetailsSheet.svelte';
	import OutboxList from '../outbox/OutboxList.svelte';
	import RemoveOfficeDialog from '../remove-office/RemoveOfficeDialog.svelte';
	import PlaceAgentDialog from '../unplaced/PlaceAgentDialog.svelte';
	import UnplacedAgents from '../unplaced/UnplacedAgents.svelte';
	import { compactRow, listBleed } from './compact-row';
	import type { Counts } from './counts';
	import { OfficesLive } from './live.svelte';
	import OfficeRow from './OfficeRow.svelte';

	let {
		connection,
		saved,
		counts: pendingCounts
	}: {
		connection: Connection;
		/** The offices saved with the connection: shown at once, before the host lists them. */
		saved: OfficeInfo[];
		/** The agent counts, read beside the list (`loadOfficeCounts`). */
		counts: Promise<Counts>;
	} = $props();

	const orgId = $derived(connection.organization.id);
	const session = $derived(sessionFor(orgApi, orgId));
	const organization = $derived(connection.organization.name);
	const here = $derived(connection.office);

	// ---- what joined, who wants to join and what is queued: layered beside the connection's own offices ----------
	const extra = new Resource(() => officesApi.overview(orgId));
	const overview = $derived(extra.data);
	const reload = () => extra.reload();
	let now = $state(Date.now());
	onMount(() => {
		const tick = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(tick);
	});
	const satelliteIds = $derived(new Set<string>(overview?.satellites.map((o) => o.id)));
	const detailsOf = (id: string) => overview?.details[id];

	// ---- the live status of the connected office (§3.3) -----------------------------------------------------------
	const live: OfficesLive = new OfficesLive({
		organizationId: () => orgId,
		saved: () => saved,
		hereId: () => here.id,
		current: (): OfficeInfo[] => current,
		joined: () => satelliteIds,
		onback: () => rowButton(here.id)?.focus()
	});
	const current: OfficeInfo[] = $derived(layerOffices(live.listed, overview));
	const offices = $derived(orderOffices(current));
	live.watch();
	onMount(() => live.start());

	/** An office that joined, as its row words it: where it is and when it was last seen. */
	const shownOffice = (o: OfficeInfo): OfficeInfo =>
		satelliteIds.has(o.id)
			? { ...o, host: satelliteWhere(o, detailsOf(o.id), now), endpoint: undefined }
			: o;

	// ---- the agent counts arrive after the list: `undefined` while they are being counted ---------------------------
	let counts = $state<Counts | undefined>();
	let organizationOfCounts = '';
	let retryingCounts = $state(false);
	$effect(() => {
		const pending = pendingCounts;
		// The last answer stays only within one organization: a new one starts uncounted (no old agents under Unplaced).
		if (organizationOfCounts !== orgId) {
			organizationOfCounts = orgId;
			counts = undefined;
		}
		// The last answer stays until the next arrives, so Retry (and its focus) isn't removed while it works.
		pending.then((r) => {
			if (pending !== pendingCounts) return;
			counts = r;
			retryingCounts = false;
		});
	});
	function retryCounts() {
		retryingCounts = true;
		void invalidate('app:offices');
	}
	const rawSnapshot = $derived(counts && 'snapshot' in counts ? counts.snapshot : undefined);
	const snapshot = $derived(
		rawSnapshot ? withPlacements(rawSnapshot, overview?.placements ?? {}) : undefined
	);
	const sample = $derived(!!counts && 'snapshot' in counts && counts.sample);
	const countError = $derived(counts && 'error' in counts ? counts.error : undefined);
	const unplaced = $derived(snapshot ? unplacedAgents(snapshot, current) : []);

	// ---- details ----------------------------------------------------------------------------------------------------
	let detailsId = $state<string | undefined>();
	// The open sheet shows the latest data for its office, not the copy captured when it was clicked.
	const detailsFor = $derived(current.find((o) => o.id === detailsId));
	let detailsOpen = $state(false);
	const rowButton = (officeId: string) =>
		document.querySelector<HTMLElement>(`[data-office="${officeId}"] button[aria-haspopup]`);

	// ---- overlays: one at a time, and whatever opened it gets focus back when it closes ------------------------------
	let lastOpener: HTMLElement | undefined;
	const via = (opener: HTMLElement | undefined) => (lastOpener = opener);
	function restoreOpener(event: Event) {
		const opener = lastOpener;
		lastOpener = undefined;
		if (!opener?.isConnected) return;
		event.preventDefault();
		opener.focus({ preventScroll: true });
	}

	/** After a confirmed removal of a row, its neighbour takes focus once the dialog has let go of it. */
	let focusAfterClose: (() => Promise<void>) | undefined;
	function restoreOrNeighbour(event: Event) {
		const next = focusAfterClose;
		focusAfterClose = undefined;
		if (!next) return restoreOpener(event);
		event.preventDefault();
		lastOpener = undefined;
		void next();
	}

	function openDetails(id: string, opener: HTMLElement) {
		via(opener);
		detailsId = id;
		detailsOpen = true;
	}

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
	let removed = false;
	let placeOpen = $state(false);
	let placing = $state<Member | undefined>();
	let offboardOpen = $state(false);
	let offboarding = $state<Member | undefined>();
	let discardOpen = $state(false);
	let discarding = $state<OutboxItem | undefined>();
	let retryingItem = $state<string | undefined>();

	const canAddOnInstance = $derived(overview?.viewer.isAdministrator ?? false);
	const placeable = $derived(
		offices.filter((o) => o.status !== 'pending').map((o) => ({ id: o.id, name: o.name }))
	);
	const pendingRows = $derived(
		overview ? overview.pendingAdds.length + overview.invitations.length : 0
	);
	const updating = $derived(
		offices.filter((o) => o.status === 'update_needed' && satelliteIds.has(o.id))
	);
	const hqAlone = $derived(
		!!overview && offices.length === 1 && pendingRows === 0 && overview.joinRequests.length === 0
	);

	async function issue(invitation: Invitation) {
		issued = await officesApi.newInvitation(orgId, invitation.id);
		issuedOpen = true;
		await reload();
	}
	function renew(invitation: Invitation, opener: HTMLElement) {
		via(opener);
		renewing = invitation;
		const stillWorks = invitation.state === 'invited' && Date.parse(invitation.expiresAt) > now;
		if (stillWorks) renewOpen = true;
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
	function removeFromSheet() {
		const office = detailsFor;
		if (!office) return;
		removing = office;
		detailsOpen = false;
		removeOpen = true;
	}
	/** After a removal, focus the agents it left unplaced, or the connected office's row when it left none. */
	function afterRemove(event: Event) {
		if (!removed) return restoreOpener(event);
		event.preventDefault();
		removed = false;
		lastOpener = undefined;
		setTimeout(() => (document.getElementById('unplaced') ?? rowButton(here.id))?.focus(), 0);
	}
</script>

<!-- The outer width governs both header gutters and the full-width divided rows. -->
<div class="@container">
	<PageHeader
		class={compactRow.gutters}
		title="Offices"
		count={offices.length}
		countLabel={offices.length === 1 ? 'office' : 'offices'}
		description="Where {organization}'s agents run. HQ keeps the records and approves new offices; other offices keep a copy."
	>
		{#snippet actions()}
			{#if extra.state !== 'unavailable'}
				<Button
					class="max-[639px]:min-h-11"
					disabled={!overview}
					aria-describedby={extra.state === 'error' ? 'overview-error' : undefined}
					onclick={(e) => (via(e.currentTarget), (resumeAdd = undefined), (addOpen = true))}
					><Plus aria-hidden="true" /> Add office</Button
				>
			{/if}
		{/snippet}
	</PageHeader>

	<div class="{compactRow.gutters} space-y-5 px-4 py-6 sm:px-6 lg:px-8">
		{#if sample}
			<!-- TEMPORARY: agent counts and placement come from the invented organization until the real snapshot is wired in. -->
			<p
				class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs/5 text-fg-muted"
				data-testid="sample-data-note"
			>
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
					class="max-[639px]:min-h-11"
					aria-busy={extra.reloading}
					aria-disabled={extra.reloading}
					onclick={() => !extra.reloading && extra.reload()}
					>{extra.reloading ? 'Retrying…' : 'Retry'}</Button
				>
			</p>
		{:else if extra.state === 'unavailable'}
			<!-- Production: the web API doesn't serve these flows yet (app-shell-ux §0.1): said, not a dead control. -->
			<p class="text-xs/5 text-fg-muted" data-testid="offices-unavailable">
				Adding, approving and removing offices isn't built yet. It's coming in this release.
			</p>
		{/if}

		{#if overview && overview.joinRequests.length > 0}
			<ul class={listBleed} aria-label="Join requests">
				{#each overview.joinRequests as request (request.id)}
					<JoinRequestRow
						{request}
						{now}
						onreview={(button) => (via(button), (approving = request), (approveOpen = true))}
					/>
				{/each}
			</ul>
		{/if}

		{#each updating as stale (stale.id)}
			<p class="text-xs/5 text-fg-muted" data-testid="version-note">
				<button
					type="button"
					class="font-medium text-link underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:-my-3 max-[639px]:min-h-11 max-[639px]:align-middle"
					aria-haspopup="dialog"
					onclick={(e) => openDetails(stale.id, e.currentTarget)}>{stale.name}</button
				> runs an older version of Agentic Org, so new features wait until it updates.
			</p>
		{/each}

		<!-- Said once when the answer changes: the badge and count are hidden from screen readers, the row's label has them. -->
		<p class="sr-only" role="status" data-testid="live-status">
			{offices.find((o) => o.id === here.id)?.name ?? 'Office'}: {statusInfo(live.statusOf(here.id))
				.label}{live.labelWhyOf(here.id) ? `: ${live.labelWhyOf(here.id)}` : ''}
		</p>

		<ul class={listBleed} aria-label="Offices">
			{#each offices as office (office.id)}
				{@const status = live.statusOf(office.id)}
				<OfficeRow
					{office}
					shown={shownOffice(office)}
					agents={snapshot ? agentsIn(snapshot, office.id).length : undefined}
					countError={!!countError}
					here={here.id === office.id}
					{status}
					why={live.whyOf(office.id)}
					retry={status === 'unreachable' || (office.id === here.id && live.retryingProbe)
						? { busy: live.check === 'checking' }
						: undefined}
					onopen={(button) => openDetails(office.id, button)}
					onretry={() => live.retry()}
				/>
			{:else}
				<DividedRow
					class="{compactRow.gutters} py-12 text-center text-sm/6 text-fg-muted"
					data-testid="no-offices">No offices to show.</DividedRow
				>
			{/each}
			{#if extra.state === 'loading'}
				<!-- The offices that joined are still being read: a row shaped like theirs, with a Loading label. -->
				<DividedRow
					class="{compactRow.gutters} flex items-center gap-x-4"
					data-testid="offices-loading"
				>
					<span class="sr-only" role="status">Loading the other offices…</span>
					<span class="min-w-0 flex-1 space-y-3" aria-hidden="true">
						<span
							class="block h-4 w-32 max-w-full animate-pulse rounded bg-surface-overlay motion-reduce:animate-none"
						></span>
						<span
							class="block h-3 w-48 max-w-full animate-pulse rounded bg-surface-raised motion-reduce:animate-none"
						></span>
					</span>
					<span
						class="h-6 w-16 shrink-0 animate-pulse rounded-full bg-surface-raised motion-reduce:animate-none"
						aria-hidden="true"
					></span>
				</DividedRow>
			{/if}
		</ul>

		{#if overview && pendingRows > 0}
			<ul class={listBleed} aria-label="Invitations and offices being added">
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

		{#if hqAlone}
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
					class="aria-disabled:cursor-not-allowed aria-disabled:opacity-60 max-[639px]:min-h-11"
					aria-disabled={retryingCounts}
					aria-busy={retryingCounts}
					onclick={() => !retryingCounts && retryCounts()}
					>{retryingCounts ? 'Retrying…' : 'Retry'}</Button
				>
			</p>
		{/if}

		{#if unplaced.length > 0}
			<UnplacedAgents
				agents={unplaced}
				bleed={compactRow.bleed}
				gutters={compactRow.gutters}
				canplace={extra.state !== 'unavailable'}
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
	bind:open={
		() => detailsOpen && !!detailsFor, // the office went away: its sheet closes
		(open) => (detailsOpen = open)
	}
	office={detailsFor}
	status={detailsFor ? live.statusOf(detailsFor.id) : 'status_unknown'}
	why={detailsFor ? live.whyOf(detailsFor.id) : undefined}
	capabilities={live.capabilities}
	onrestarted={live.refresh}
	onCloseAutoFocus={(e) => (removeOpen ? e.preventDefault() : restoreOpener(e))}
	{snapshot}
	counting={!snapshot && !countError}
	{sample}
	here={detailsFor?.id === here.id}
	details={detailsFor ? detailsOf(detailsFor.id) : undefined}
	onrename={detailsFor && overview
		? async (name) => {
				await officesApi.renameOffice(orgId, detailsFor.id, name);
				await reload();
			}
		: undefined}
	onremove={detailsFor && !isHq(detailsFor) && overview ? removeFromSheet : undefined}
/>

<AddOfficeDialog
	bind:open={addOpen}
	organizationId={orgId}
	{canAddOnInstance}
	resume={resumeAdd}
	onchanged={reload}
	onCloseAutoFocus={(e) => {
		// Closing doesn't cancel a wait for running agents: its pending row shows in the list (§6.2).
		void reload();
		restoreOpener(e);
	}}
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
	onCloseAutoFocus={restoreOrNeighbour}
	onconfirm={async () => {
		if (!cancelling) return;
		const focusNext = neighbourFocus(lastOpener);
		await officesApi.cancelInvitation(orgId, cancelling.id);
		await reload();
		focusAfterClose = focusNext;
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
		? live.statusOf(removing.id) === 'offline' || live.statusOf(removing.id) === 'unreachable'
		: false}
	onchanged={async () => {
		await reload();
		removed = true;
		toast.success(`${removing?.name ?? 'The office'} was removed.`);
	}}
	onCloseAutoFocus={afterRemove}
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
	onCloseAutoFocus={restoreOrNeighbour}
	onconfirm={async () => {
		if (!discarding) return;
		const focusNext = neighbourFocus(lastOpener);
		await officesApi.discardOutbox(orgId, discarding.id);
		await reload();
		focusAfterClose = focusNext;
	}}
/>
