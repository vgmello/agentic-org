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
	import DividedRow from '$lib/components/DividedRow.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { neighbourFocus, openerFrom, returnFocus } from '$lib/components/focus';
	import { Resource } from '$lib/components/resource.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/proto';
	import { agentsIn, orderOffices, statusInfo, unplacedAgents } from '$lib/modules/offices/model';
	import { orgApi } from '$lib/modules/organization/api';
	import { sessionFor } from '$lib/modules/organization/session';
	import type { Connection, OfficeInfo } from '$lib/proto';
	import { officesApi } from '../../api';
	import { layerOffices, satelliteWhere } from '../../layer';
	import { withPlacements } from '../../placements';
	import type { Invitation, OutboxItem } from '../../types';
	import InvitationRows from '../add-office/InvitationRows.svelte';
	import PendingAddRows from '../add-office/PendingAddRows.svelte';
	import JoinRequestRow from '../approve-join/JoinRequestRow.svelte';
	import OutboxList from '../outbox/OutboxList.svelte';
	import UnplacedAgents from '../unplaced/UnplacedAgents.svelte';
	import { possessive, statusChangeText } from '../../copy';
	import { compactRow, listBleed } from './compact-row';
	import type { Counts } from './counts';
	import { OfficesLive } from './live.svelte';
	import OfficesDialogs from './OfficesDialogs.svelte';
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
	const extra = new Resource((signal) => officesApi.overview(orgId, signal));
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
	let countRetryOpener: HTMLElement | null = null;
	$effect(() => {
		let active = true;
		const pending = pendingCounts;
		// The last answer stays only within one organization: a new one starts uncounted (no old agents under Unplaced).
		if (organizationOfCounts !== orgId) {
			organizationOfCounts = orgId;
			counts = undefined;
		}
		// The last answer stays until the next arrives, so Retry (and its focus) isn't removed while it works.
		pending.then((r) => {
			if (!active || pending !== pendingCounts) return;
			const opener = countRetryOpener;
			countRetryOpener = null;
			counts = r;
			retryingCounts = false;
			if (!opener) return;
			if ('error' in r) void returnFocus(opener);
			else
				void returnFocus(null, {
					within: opener,
					fallback: () => {
						const heading = document.getElementById('offices-heading');
						if (heading) heading.tabIndex = -1;
						return heading;
					}
				});
		});
		return () => {
			active = false;
		};
	});
	function retryCounts(event: MouseEvent) {
		countRetryOpener = openerFrom(event);
		countRetryOpener?.focus({ preventScroll: true });
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

	// ---- status changes of every office, said once in a polite live region (OFC-13, §6.1) ---------------------------
	let changeNote = $state('');
	// Bookkeeping only, never rendered: a plain map.
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- non-reactive bookkeeping map never rendered
	const lastStatus = new Map<string, string>();
	$effect(() => {
		for (const office of offices) {
			const status = live.statusOf(office.id);
			const before = lastStatus.get(office.id);
			lastStatus.set(office.id, status);
			if (before === undefined || before === status || office.id === here.id) continue;
			const text = statusChangeText(office.name, status, live.labelWhyOf(office.id));
			if (text) changeNote = text;
		}
	});

	// ---- details ----------------------------------------------------------------------------------------------------
	const rowButton = (officeId: string) =>
		document.querySelector<HTMLElement>(`[data-office="${officeId}"] button[aria-haspopup]`);

	let dialogs = $state<OfficesDialogs | undefined>();
	let retryingItem = $state<string | undefined>();
	let renewingId = $state<string | undefined>();

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
</script>

<!-- The outer width governs both header gutters and the full-width divided rows. -->
<div class="@container">
	<PageHeader
		class={compactRow.gutters}
		title="Offices"
		count={offices.length}
		countLabel={offices.length === 1 ? 'office' : 'offices'}
		id="offices-heading"
		description="Where {possessive(
			organization
		)} agents run. HQ keeps the records and approves new offices; every other office keeps a copy."
	>
		{#snippet actions()}
			{#if extra.state !== 'unavailable' && extra.state !== 'unsupported'}
				<Button
					class="max-[639px]:min-h-11"
					disabled={!overview}
					aria-describedby={extra.state === 'error' ? 'overview-error' : undefined}
					onclick={(e) => dialogs?.openAdd(e.currentTarget)}
					><Plus aria-hidden="true" /> Add office</Button
				>
			{/if}
		{/snippet}
	</PageHeader>

	<div class="{compactRow.gutters} flex flex-col gap-5 px-4 py-6 sm:px-6 lg:px-8">
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
		{:else if extra.state === 'unsupported'}
			<p class="text-xs/5 text-fg-muted" data-testid="resource-unsupported">
				Not available in this version.{extra.message ? ` ${extra.message}` : ''}
			</p>
		{/if}

		{#if overview && overview.joinRequests.length > 0}
			<ul class={listBleed} aria-label="Join requests">
				{#each overview.joinRequests as request (request.id)}
					<JoinRequestRow
						{request}
						{now}
						onreview={(button) => dialogs?.openApprove(request, button)}
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
					onclick={(e) => dialogs?.openDetails(stale.id, e.currentTarget)}>{stale.name}</button
				> runs an older version of Agentic Org, so new features wait until it updates.
			</p>
		{/each}

		<!-- Said once when the answer changes: the badge and count are hidden from screen readers, the row's label has them. -->
		<p class="sr-only" role="status" data-testid="office-changes">{changeNote}</p>
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
					onopen={(button) => dialogs?.openDetails(office.id, button)}
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
					<span class="flex min-w-0 flex-1 flex-col gap-3" aria-hidden="true">
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
					onopen={(add, button) => dialogs?.openAdd(button, add)}
					oncancel={(add, button) => dialogs?.openStop(add, button)}
				/>
				<InvitationRows
					invitations={overview.invitations}
					{now}
					oncancel={(invitation, button) => dialogs?.openCancelInvite(invitation, button)}
					onrenew={(invitation, button) => dialogs?.renew(invitation, button)}
					renewing={renewingId}
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
					onclick={(e) => !retryingCounts && retryCounts(e)}
					>{retryingCounts ? 'Retrying…' : 'Retry'}</Button
				>
			</p>
		{/if}

		{#if unplaced.length > 0}
			<UnplacedAgents
				agents={unplaced}
				bleed={compactRow.bleed}
				gutters={compactRow.gutters}
				canplace={extra.state !== 'unavailable' && extra.state !== 'unsupported'}
				onplace={(agent, button) => dialogs?.openPlace(agent, button)}
				onoffboard={(agent, button) => dialogs?.openOffboard(agent, button)}
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
				ondiscard={(item, button) => dialogs?.openDiscard(item, button)}
			/>
		{/if}
		<p class="text-xs/5 text-fg-muted">Shown from HQ. Statuses include last-seen information.</p>
	</div>
</div>

<OfficesDialogs
	bind:this={dialogs}
	bind:renewingId
	{orgId}
	{organization}
	hereId={here.id}
	{now}
	{current}
	{placeable}
	{overview}
	{canAddOnInstance}
	{snapshot}
	{countError}
	{sample}
	{session}
	{live}
	{detailsOf}
	{rowButton}
	{reload}
/>
