<!--
  @component
  All overlays, sheets, and confirmation dialogs owned by the Offices page, along with their opener/neighbour
  focus restoration state.
-->
<script lang="ts">
	import { invalidate } from '$app/navigation';
	import { toast } from 'svelte-sonner';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { focusHeading, neighbourFocus } from '$lib/components/focus';
	import { OffboardAgentDialog } from '$lib/modules/employees/ui';
	import { agentsIn, isHq } from '$lib/modules/offices/model';
	import type { OrgSession } from '$lib/modules/organization/session';
	import { toAppError } from '$lib/proto';
	import type { Member, OfficeInfo, OrgSnapshot } from '$lib/proto';
	import { officesApi } from '../../api';
	import type {
		Invitation,
		Issued,
		JoinRequest,
		OfficesOverview,
		OutboxItem,
		PendingAdd
	} from '../../types';
	import AddOfficeDialog from '../add-office/AddOfficeDialog.svelte';
	import IssuedDialog from '../add-office/IssuedDialog.svelte';
	import ApproveJoinDialog from '../approve-join/ApproveJoinDialog.svelte';
	import OfficeDetailsSheet from '../details/OfficeDetailsSheet.svelte';
	import RemoveOfficeDialog from '../remove-office/RemoveOfficeDialog.svelte';
	import PlaceAgentDialog from '../unplaced/PlaceAgentDialog.svelte';
	import type { OfficesLive } from './live.svelte';

	let {
		orgId,
		organization,
		hereId,
		now,
		current,
		placeable,
		overview,
		canAddOnInstance,
		snapshot,
		countError,
		sample,
		session,
		live,
		detailsOf,
		rowButton,
		reload,
		renewingId = $bindable<string | undefined>(undefined)
	}: {
		orgId: string;
		organization: string;
		hereId: string;
		now: number;
		current: OfficeInfo[];
		placeable: { id: string; name: string }[];
		overview: OfficesOverview | undefined;
		canAddOnInstance: boolean;
		snapshot: OrgSnapshot | undefined;
		countError: string | undefined;
		sample: boolean;
		session: OrgSession;
		live: OfficesLive;
		detailsOf: (id: string) => OfficesOverview['details'][string] | undefined;
		rowButton: (officeId: string) => HTMLElement | null;
		reload: () => Promise<void>;
		renewingId?: string | undefined;
	} = $props();

	let detailsId = $state<string | undefined>();
	// The open sheet shows the latest data for its office, not the copy captured when it was clicked.
	const detailsFor = $derived(current.find((o) => o.id === detailsId));
	let detailsOpen = $state(false);

	// ---- overlays: one at a time, and whatever opened it gets focus back when it closes ------------------------------
	// When the opener's row is gone by then (approved, declined, renewed, removed), focus goes to an explicit target
	// set by the action, else to the opener's neighbour taken when it opened, else to the page heading: never the body.
	let lastOpener: HTMLElement | undefined;
	let lastNeighbour: (() => Promise<void>) | undefined;
	let focusAfterClose: (() => void) | undefined;
	/** An approve or decline whose list is reloading; it answers where focus goes. */
	let approveChange: Promise<() => void> | undefined;
	const pageHeading = () => (focusHeading('offices-heading'), undefined);
	const via = (opener: HTMLElement | undefined) => {
		lastOpener = opener;
		lastNeighbour = opener ? neighbourFocus(opener, pageHeading) : undefined;
	};
	function restoreOpener(event: Event) {
		const opener = lastOpener;
		const neighbour = lastNeighbour;
		const next = focusAfterClose;
		lastOpener = lastNeighbour = focusAfterClose = undefined;
		if (!next && !opener?.isConnected && !neighbour) return;
		event.preventDefault();
		if (next) setTimeout(next, 0);
		else if (opener?.isConnected) opener.focus({ preventScroll: true });
		else void neighbour?.();
	}
	/** Focus the first control of the row named `name`: the office's row, else its invitation, else the heading. */
	const focusRowNamed = (name: string) => () => {
		const quoted = CSS.escape(name);
		const target =
			document.querySelector<HTMLElement>(
				`[data-testid=office-row][data-office-name="${quoted}"] button[aria-haspopup]`
			) ??
			document.querySelector<HTMLElement>(
				`[data-testid=invitation-row][data-office-name="${quoted}"] button`
			);
		if (target) target.focus();
		else pageHeading();
	};

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

	async function issue(invitation: Invitation) {
		issued = await officesApi.newInvitation(orgId, invitation.id);
		issuedOpen = true;
		await reload();
		// The renewed invitation is a new row: focus it when the shown-once dialog closes.
		focusAfterClose = focusRowNamed(invitation.officeName);
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
		lastOpener = lastNeighbour = focusAfterClose = undefined;
		setTimeout(() => (document.getElementById('unplaced') ?? rowButton(hereId))?.focus(), 0);
	}

	export function openDetails(id: string, opener: HTMLElement) {
		via(opener);
		detailsId = id;
		detailsOpen = true;
	}
	export function openAdd(opener: HTMLElement, pending?: PendingAdd) {
		via(opener);
		resumeAdd = pending;
		addOpen = true;
	}
	export function openApprove(request: JoinRequest, opener: HTMLElement) {
		via(opener);
		approving = request;
		approveOpen = true;
	}
	export function openStop(add: PendingAdd, opener: HTMLElement) {
		via(opener);
		stopping = add;
		stopOpen = true;
	}
	export function openCancelInvite(invitation: Invitation, opener: HTMLElement) {
		via(opener);
		cancelling = invitation;
		cancelInviteOpen = true;
	}
	export function renew(invitation: Invitation, opener: HTMLElement) {
		if (renewingId) return;
		via(opener);
		renewing = invitation;
		const stillWorks = invitation.state === 'invited' && Date.parse(invitation.expiresAt) > now;
		if (stillWorks) return void (renewOpen = true);
		renewingId = invitation.id;
		void issue(invitation)
			.catch((e) => toast.error(toAppError(e).message))
			.finally(() => (renewingId = undefined));
	}
	export function openPlace(agent: Member, opener: HTMLElement) {
		via(opener);
		placing = agent;
		placeOpen = true;
	}
	export function openOffboard(agent: Member, opener: HTMLElement) {
		via(opener);
		offboarding = agent;
		offboardOpen = true;
	}
	export function openDiscard(item: OutboxItem, opener: HTMLElement) {
		via(opener);
		discarding = item;
		discardOpen = true;
	}
</script>

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
	here={detailsFor?.id === hereId}
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
	onchanged={() => {
		// The request's row goes: its office (approved) or its invitation (declined, cancelled) takes focus once listed.
		const name = approving?.officeName;
		const change = reload().then(() => (name ? focusRowNamed(name) : pageHeading));
		approveChange = change;
		return change.then(() => undefined);
	}}
	onCloseAutoFocus={(e) => {
		const change = approveChange;
		approveChange = undefined;
		if (!change) return restoreOpener(e);
		e.preventDefault();
		lastOpener = lastNeighbour = focusAfterClose = undefined;
		void change.then((focus) => setTimeout(focus, 0));
	}}
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
		const focusNext = neighbourFocus(lastOpener, pageHeading);
		await officesApi.cancelInvitation(orgId, cancelling.id);
		await reload();
		focusAfterClose = () => void focusNext();
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
	agents={removing && snapshot ? agentsIn(snapshot, removing.id).length : undefined}
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
	snapshot={snapshot ?? { position: { epoch: 0, seq: 0 }, teams: [], members: [] }}
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
		const focusNext = neighbourFocus(lastOpener, pageHeading);
		await officesApi.discardOutbox(orgId, discarding.id);
		await reload();
		focusAfterClose = () => void focusNext();
	}}
/>
