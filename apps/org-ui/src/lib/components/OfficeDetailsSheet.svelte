<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import OfficeStatusBadge from './OfficeStatusBadge.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { toast } from 'svelte-sonner';
	import pkg from '../../../package.json';
	import { officeApi, toAppError } from '$lib/office/api';
	import { agentsIn, copyDetails, repairLocation, restartAction, restartOutcome, roleInfo, secondaryText, statusInfo, type DisplayStatus } from '$lib/offices/model';
	import type { OfficeInfo, OrgSnapshot } from '$lib/proto';

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
		onCloseAutoFocus
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
	} = $props();

	const agents = $derived(office && snapshot ? agentsIn(snapshot, office.id) : []);

	// Restarting (spec §3.3): the spinner stays until the office is no longer stopped, or half a minute has passed.
	let restarting = $state(false);
	let restartNote = $state<string | undefined>();
	let unsupported = $state(false);
	let sheetContent = $state<HTMLDivElement | null>(null);
	const repairId = $props.id();
	const repairCommand = 'agentic-org doctor';
	let repairOpen = $state(false);
	let commandCopied = $state(false);
	// Each restart run has a number; a run that is no longer the newest stops and leaves the state to the newer one.
	let run = 0;
	const action = $derived(office ? restartAction(office, capabilities) : {});
	// Keyed on the id and on a boolean, not on the objects: the page re-lists the offices every check, and a new object for the
	// same office must not cancel the run that is waiting for it.
	const officeId = $derived(office?.id);
	const canRestart = $derived(capabilities?.includes('office.restart'));
	$effect(() => {
		// Another office starts clean (the run for the old one stops).
		void officeId;
		run++;
		restarting = false;
		restartNote = undefined;
		repairOpen = false;
		commandCopied = false;
	});
	$effect(() => {
		if (!open || !why || office?.status_reason !== 'unsupported_version') {
			repairOpen = false;
			commandCopied = false;
		}
	});
	$effect(() => {
		// A new hello may say the host can restart now (it was updated).
		void canRestart;
		unsupported = false;
	});
	$effect(() => () => void run++); // the page is leaving: stop any loop
	async function restart() {
		if (!office || restarting) return;
		const mine = ++run;
		const id = office.id;
		restarting = true;
		restartNote = undefined;
		const live = () => mine === run;
		try {
			await officeApi.restartOffice(id);
		} catch (e) {
			if (!live()) return;
			const err = toAppError(e);
			const outcome = restartOutcome(err.source === 'api' ? err.code : 'unknown');
			if (outcome.kind === 'unsupported') unsupported = true;
			if (outcome.kind === 'refresh') await onrestarted?.();
			if (outcome.kind !== 'wait') {
				if (!live()) return;
				// Unsupported says it once, in the note; the others say it here, in the host's words if the app has none.
				restartNote = outcome.kind === 'unsupported' ? undefined : (outcome.message ?? err.message);
				restarting = false;
				return;
			}
		}
		// Keep the spinner until the host says it is no longer stopped (the live status), for up to 30 s.
		for (let i = 0; i < 15 && live(); i++) {
			await onrestarted?.();
			if (!live()) return;
			if (office?.id !== id || (office.status !== 'offline' && office.status !== 'unavailable')) break;
			await new Promise((r) => setTimeout(r, 2000));
		}
		if (!live()) return;
		const back = office?.id === id && office.status !== 'offline' && office.status !== 'unavailable';
		restarting = false;
		// The button goes with the stop; keep the keyboard somewhere sensible.
		if (back) sheetContent?.querySelector<HTMLButtonElement>('[data-slot="sheet-close"]')?.focus();
	}

	async function copy() {
		if (!office) return;
		try {
			await navigator.clipboard.writeText(copyDetails(office, statusInfo(status).label, new Date(), pkg.version));
			toast.success('Copied the details.');
		} catch {
			toast.error("Couldn't copy. Select the details and copy them yourself.");
		}
	}

	function toggleRepair(event: MouseEvent) {
		// WebKit doesn't focus a button on a pointer click.
		if (event.currentTarget instanceof HTMLElement) event.currentTarget.focus({ preventScroll: true });
		repairOpen = !repairOpen;
		commandCopied = false;
	}

	async function copyRepairCommand() {
		try {
			await navigator.clipboard.writeText(repairCommand);
			if (repairOpen) commandCopied = true;
		} catch {
			toast.error("Couldn't copy. Select the command and copy it yourself.");
		}
	}
</script>

<Sheet bind:open>
	<SheetContent bind:ref={sheetContent} {onCloseAutoFocus} side="right" class="w-full max-w-md overflow-y-auto max-[640px]:max-w-full max-[640px]:data-[side=right]:w-full">
		{#if office}
			<SheetHeader class="pr-14">
				<SheetTitle class="[overflow-wrap:anywhere]">{office.name}</SheetTitle>
				<SheetDescription>{roleInfo(office).description}</SheetDescription>
			</SheetHeader>
			<dl class="grid grid-cols-1 gap-x-6 gap-y-2 px-4 text-sm/6 min-[480px]:grid-cols-[auto_1fr] min-[480px]:gap-y-3" data-testid="office-details">
				<dt class="text-gray-400">Role</dt>
				<dd class="text-white">{roleInfo(office).label}</dd>
				<dt class="text-gray-400">Where</dt>
				<dd class="min-w-0 text-white [overflow-wrap:anywhere]">{secondaryText(office) + (here ? " (you're here)" : '')}</dd>
				<dt class="text-gray-400">Status</dt>
				<dd><OfficeStatusBadge {status} /></dd>
				{#if why}
					<dt class="text-gray-400">Why</dt>
					<dd class="min-w-0 text-white [overflow-wrap:anywhere]">
						<p data-testid="office-why">{why}</p>
						{#if office.status_reason === 'unsupported_version'}
							{@const location = repairLocation(office.host)}
							<Button variant="outline" class="mt-2 h-auto min-h-9 whitespace-normal py-2 text-left"
								aria-expanded={repairOpen} aria-controls={repairId} onclick={toggleRepair}>
								{repairOpen ? 'Hide repair instructions' : 'Show local CLI repair instructions'}
							</Button>
							<div id={repairId} hidden={!repairOpen} class="mt-3 space-y-2 text-gray-300">
								<p>On {location}, run:</p>
								<pre class="overflow-x-auto rounded-md bg-black/20 px-3 py-2 text-xs text-white"><code>{repairCommand}</code></pre>
								<Button variant="outline" size="sm" onclick={copyRepairCommand}>{commandCopied ? 'Copied' : 'Copy command'}</Button>
								<p>It checks the Agentic Org instance on that machine and says what to repair. Follow its steps there. This page updates on its own when the office is back.</p>
								<p>If {location} runs more than one instance, add <code>--name &lt;instance&gt;</code>.</p>
							</div>
						{/if}
					</dd>
				{/if}
				<dt class="text-gray-400">Agents here</dt>
				<dd class="min-w-0 text-white [overflow-wrap:anywhere]">
					{#if !snapshot}{counting ? 'Counting…' : "Couldn't be counted"}{:else}{agents.length === 0 ? 'None' : agents.map((a) => a.name).join(', ')}{/if}
					{#if sample && agents.length > 0}<Badge variant="secondary" data-testid="sample-data-note-sheet">Sample data</Badge>{/if}
				</dd>
			</dl>
			<p class="sr-only" role="status">{restarting ? 'Restarting…' : ''}</p>
			{#if why && (action.note || unsupported)}
				<p class="px-4 text-sm/6 text-gray-300" data-testid="restart-note">{action.note ?? 'Update Agentic Org to restart offices from here'}</p>
			{/if}
			{#if restartNote}<p class="px-4 text-sm/6 text-red-300" role="alert" data-testid="restart-error">{restartNote}</p>{/if}
			{#if why}
				<SheetFooter class="mt-auto">
					{#if action.label && !unsupported}
						<Button aria-disabled={restarting} aria-busy={restarting} class="aria-disabled:opacity-50" onclick={restart} data-testid="restart-office">{restarting ? 'Restarting…' : action.label}</Button>
					{/if}
					<Button variant="outline" onclick={copy}>Copy details</Button>
				</SheetFooter>
			{/if}
		{/if}
	</SheetContent>
</Sheet>
