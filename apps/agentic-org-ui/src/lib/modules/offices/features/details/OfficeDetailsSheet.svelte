<script lang="ts">
	import { onDestroy } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetFooter,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import OfficeStatusBadge from '../status/OfficeStatusBadge.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { toast } from 'svelte-sonner';
	import pkg from '../../../../../../package.json';
	import { officeApi } from '$lib/modules/offices/office-api';
	import OfficeRepairLocation from './OfficeRepairLocation.svelte';
	import {
		agentsIn,
		copyDetails,
		repairReasonCopy,
		restartAction,
		restartOutcome,
		roleInfo,
		secondaryText,
		statusInfo,
		type DisplayStatus
	} from '$lib/modules/offices/model';
	import { toAppError, type OfficeInfo, type OrgSnapshot } from '$lib/proto';
	import { agoText } from '../../time';
	import { syncText } from '../../layer';
	import RenameOffice from '../rename-office/RenameOffice.svelte';
	import type { SatelliteDetails } from '../../types';

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
		onrestarted,
		onCloseAutoFocus,
		details,
		onrename,
		onremove
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
		/** The list owns the row that opened this sheet. */
		onCloseAutoFocus?: (event: Event) => void;
		/** What HQ knows about an office that joined (sync, version, what it follows). */
		details?: SatelliteDetails;
		/** Rename this office (offices-and-teams-ux §6.1). Rejects with the reason. */
		onrename?: (name: string) => Promise<void>;
		/** Remove this office… (not offered for HQ): the list opens its own confirmation. */
		onremove?: () => void;
	} = $props();

	const agents = $derived(office && snapshot ? agentsIn(snapshot, office.id) : []);

	// Restarting (spec §3.3): the spinner stays until the office is no longer stopped, or half a minute has passed.
	let restartingOfficeId = $state<string | undefined>();
	let restartNoteEntry = $state<{ key: string; text: string } | undefined>();
	let unsupportedState = $state<
		{ officeId: string | undefined; canRestart: boolean | undefined } | undefined
	>();
	let sheetContent = $state<HTMLDivElement | null>(null);
	const repairId = $props.id();
	const repairCommand = 'agentic-org doctor';
	let repairOpenFor = $state<string | undefined>();
	let commandCopiedFor = $state<string | undefined>();
	// Each restart run has a number; a run that is no longer the newest stops and leaves the state to the newer one.
	let run = 0;
	const action = $derived(office ? restartAction(office, capabilities) : {});
	// Keyed on the id and on a boolean, not on the objects: the page re-lists the offices every check, and a new object for the
	// same office must not cancel the run that is waiting for it.
	const officeId = $derived(office?.id);
	const canRestart = $derived(capabilities?.includes('office.restart'));
	const reportedStatus = $derived(office?.status);
	const reportedReason = $derived(office?.status_reason);
	const statusKey = $derived(
		`${officeId ?? ''}|${status}|${reportedStatus ?? ''}|${reportedReason ?? ''}`
	);
	const restarting = $derived(Boolean(officeId && restartingOfficeId === officeId));
	const restartNote = $derived(
		restartNoteEntry?.key === statusKey ? restartNoteEntry.text : undefined
	);
	const unsupported = $derived(
		Boolean(
			unsupportedState &&
			unsupportedState.officeId === officeId &&
			unsupportedState.canRestart === canRestart
		)
	);
	const repairEligible = $derived(
		open && Boolean(why) && office?.status_reason === 'unsupported_version'
	);
	const repairOpen = $derived(Boolean(repairEligible && officeId && repairOpenFor === officeId));
	const commandCopied = $derived(Boolean(repairOpen && officeId && commandCopiedFor === officeId));
	onDestroy(() => {
		run++;
	});
	const isOfficeBack = (id: string) =>
		office?.id === id && office.status !== 'offline' && office.status !== 'unavailable';

	async function pollUntilOnline(id: string, live: () => boolean): Promise<void> {
		for (let i = 0; i < 15 && live(); i++) {
			await onrestarted?.();
			if (!live() || isOfficeBack(id)) break;
			await new Promise((r) => setTimeout(r, 2000));
		}
	}

	async function restart() {
		if (!office || restarting) return;
		const mine = ++run;
		const id = office.id;
		const startedStatusKey = statusKey;
		const startedCanRestart = canRestart;
		restartingOfficeId = id;
		restartNoteEntry = undefined;
		const live = () => mine === run && office?.id === id;
		const clearRestarting = () => {
			if (restartingOfficeId === id) restartingOfficeId = undefined;
		};
		try {
			await officeApi.restartOffice(id);
		} catch (e) {
			if (!live()) return;
			const err = toAppError(e);
			const outcome = restartOutcome(err.source === 'api' ? err.code : 'unknown');
			if (outcome.kind === 'unsupported') {
				unsupportedState = { officeId: id, canRestart: startedCanRestart };
			}
			if (outcome.kind === 'refresh') await onrestarted?.();
			if (outcome.kind !== 'wait') {
				if (!live()) return;
				if (statusKey === startedStatusKey && outcome.kind !== 'unsupported' && outcome.message) {
					restartNoteEntry = { key: startedStatusKey, text: outcome.message };
				}
				clearRestarting();
				return;
			}
		}
		await pollUntilOnline(id, live);
		if (!live()) return;
		const back = isOfficeBack(id);
		clearRestarting();
		// The button goes with the stop; keep the keyboard somewhere sensible.
		if (back) sheetContent?.querySelector<HTMLButtonElement>('[data-slot="sheet-close"]')?.focus();
	}

	async function copy() {
		if (!office) return;
		try {
			await navigator.clipboard.writeText(
				copyDetails(office, statusInfo(status).label, new Date(), pkg.version)
			);
			toast.success('Copied the details.');
		} catch {
			toast.error("Couldn't copy. Select the details and copy them yourself.");
		}
	}

	function toggleRepair(event: MouseEvent) {
		// WebKit doesn't focus a button on a pointer click.
		if (event.currentTarget instanceof HTMLElement)
			event.currentTarget.focus({ preventScroll: true });
		repairOpenFor = repairOpen ? undefined : officeId;
		commandCopiedFor = undefined;
	}

	async function copyRepairCommand() {
		try {
			await navigator.clipboard.writeText(repairCommand);
			if (repairOpen) commandCopiedFor = officeId;
		} catch {
			toast.error("Couldn't copy. Select the command and copy it yourself.");
		}
	}
</script>

<Sheet bind:open>
	<SheetContent
		bind:ref={sheetContent}
		{onCloseAutoFocus}
		side="right"
		class="w-full max-w-md overflow-y-auto max-[640px]:max-w-full max-[640px]:data-[side=right]:w-full"
	>
		{#if office}
			<SheetHeader class="pr-14">
				<SheetTitle class="[overflow-wrap:anywhere]">{office.name}</SheetTitle>
				<SheetDescription>{roleInfo(office).description}</SheetDescription>
			</SheetHeader>
			<dl
				class="grid grid-cols-1 gap-x-6 gap-y-2 px-4 text-sm/6 min-[480px]:grid-cols-[auto_1fr] min-[480px]:gap-y-3"
				data-testid="office-details"
			>
				<dt class="text-fg-muted">Name</dt>
				<dd class="min-w-0 text-fg">
					{#if onrename}<RenameOffice name={office.name} onsave={onrename} />{:else}<span
							class="[overflow-wrap:anywhere]">{office.name}</span
						>{/if}
				</dd>
				<dt class="text-fg-muted">Role</dt>
				<dd class="text-fg">{roleInfo(office).label}</dd>
				<dt class="text-fg-muted">Where</dt>
				<dd class="min-w-0 text-fg [overflow-wrap:anywhere]">
					{secondaryText(office) + (here ? " (you're here)" : '')}
				</dd>
				<dt class="text-fg-muted">Status</dt>
				<dd><OfficeStatusBadge {status} /></dd>
				{#if why}
					<dt class="text-fg-muted">Why</dt>
					<dd class="min-w-0 text-fg [overflow-wrap:anywhere]">
						<p data-testid="office-why">
							{#if office.status_reason === 'unsupported_version'}{repairReasonCopy.before}<OfficeRepairLocation
									{office}
								/>{repairReasonCopy.after}{:else}{why}{/if}
						</p>
						{#if office.status_reason === 'unsupported_version'}
							<Button
								variant="outline"
								class="mt-2 h-auto min-h-9 whitespace-normal py-2 text-left"
								aria-expanded={repairOpen}
								aria-controls={repairId}
								onclick={toggleRepair}
							>
								{repairOpen ? 'Hide repair instructions' : 'Show local CLI repair instructions'}
							</Button>
							<div
								id={repairId}
								hidden={!repairOpen}
								class="mt-3 flex flex-col gap-2 text-fg-secondary"
							>
								<p>On <OfficeRepairLocation {office} />, run:</p>
								<pre
									class="overflow-x-auto rounded-md bg-surface-sunken px-3 py-2 text-xs text-fg ring-1 ring-line ring-inset"><code
										>{repairCommand}</code
									></pre>
								<Button variant="outline" size="sm" onclick={copyRepairCommand}
									>{commandCopied ? 'Copied' : 'Copy command'}</Button
								>
								<p>
									It checks the Agentic Org instance on that machine and says what to repair. Follow
									its steps there. This page updates on its own when the office is back.
								</p>
								<p>
									If <OfficeRepairLocation {office} /> runs more than one instance, add
									<code>--name &lt;instance&gt;</code>.
								</p>
							</div>
						{/if}
					</dd>
				{/if}
				{#if details}
					<dt class="text-fg-muted">Last seen</dt>
					<dd class="text-fg tabular-nums" data-testid="office-last-seen">
						{details.lastSeenAt ? agoText(details.lastSeenAt, Date.now()) : 'Not known'}
					</dd>
					<dt class="text-fg-muted">Sync</dt>
					<dd class="text-fg tabular-nums" data-testid="office-sync">{syncText(details)}</dd>
					{#if details.version}
						<dt class="text-fg-muted">Version</dt>
						<dd class="text-fg tabular-nums">Agentic Org {details.version}</dd>
					{/if}
					<dt class="text-fg-muted">Follows</dt>
					<dd class="min-w-0 text-fg [overflow-wrap:anywhere]">
						{details.follows.length === 0 ? 'No projects yet' : details.follows.join(', ')}
					</dd>
				{/if}
				<dt class="text-fg-muted">Agents here</dt>
				<dd class="min-w-0 text-fg [overflow-wrap:anywhere]">
					{#if !snapshot}{counting ? 'Counting…' : "Couldn't be counted"}{:else}{agents.length === 0
							? 'None'
							: agents.map((a) => a.name).join(', ')}{/if}
					{#if sample && agents.length > 0}<Badge
							variant="secondary"
							data-testid="sample-data-note-sheet">Sample data</Badge
						>{/if}
				</dd>
			</dl>
			<p class="sr-only" role="status">{restarting ? 'Restarting…' : ''}</p>
			{#if why && (action.note || unsupported)}
				<p class="px-4 text-sm/6 text-fg-secondary" data-testid="restart-note">
					{action.note ?? 'Update Agentic Org to restart offices from here'}
				</p>
			{/if}
			{#if restartNote}<p
					class="px-4 text-sm/6 text-danger-text"
					role="alert"
					data-testid="restart-error"
				>
					{restartNote}
				</p>{/if}
			{#if onremove}
				<div class="px-4">
					<Button
						variant="outline"
						class="text-danger-text max-[639px]:min-h-11"
						onclick={onremove}
						data-testid="remove-office">Remove office…</Button
					>
				</div>
			{/if}
			{#if why}
				<SheetFooter class="mt-auto">
					{#if action.label && !unsupported}
						<Button
							aria-disabled={restarting}
							aria-busy={restarting}
							class="aria-disabled:opacity-50"
							onclick={restart}
							data-testid="restart-office">{restarting ? 'Restarting…' : action.label}</Button
						>
					{/if}
					<Button variant="outline" onclick={copy}>Copy details</Button>
				</SheetFooter>
			{/if}
		{/if}
	</SheetContent>
</Sheet>
