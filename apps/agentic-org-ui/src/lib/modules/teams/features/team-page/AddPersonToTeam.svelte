<!--
  @component
  A team page's one primary action (§7.2, Hire or add): Add a person, with this team as their one home team
  (offices-and-teams-ux §7.4, PEOPLE-3/4). Name and Handle are separate, both required; the handle is suggested from
  the name until it is edited, then left alone; HQ checks it is free across people and agents and a refusal shows
  on the Handle field with its suggestion. Home team is a single required choice, preset to this team. Contact and
  Reports to are optional; the owner's person can be chosen like anyone (§7.1). The footer stays in view while the
  fields scroll (390 px, 200% text). Success is said only after HQ accepts it. Hire an agent beside it opens the
  Hire flow with this team preset. The Org chart uses it too, with its home team starting at the top-level team.
-->
<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import { errorClass, helpClass, inputClass, selectClass } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { byName, withoutFormer } from '$lib/org/model';
	import { teamOptions } from '$lib/org/team-edit';
	import type { Connection, OrgSnapshot } from '$lib/proto';
	import { teamsApi } from '../../api';
	import { handleProblem, nameProblem, suggestHandle } from '../../person';

	let {
		connection,
		snapshot,
		teamId,
		onadded
	}: {
		connection: Connection;
		snapshot: OrgSnapshot;
		teamId: string;
		/** After HQ accepted the new person: re-read what the caller shows, and focus the new person there. */
		onadded: (added: { memberId: string; name: string }) => Promise<void>;
	} = $props();

	const organization = $derived(connection.organization);
	/**
	 * Archived teams take no new home employees (§7.10, PEOPLE-3): read which are archived each time the sheet opens
	 * and leave them out. Until that read answers, or if it fails, HQ still refuses an archived team on submit.
	 */
	let archived = $state.raw<ReadonlySet<string>>(new Set());
	let archivedForOrg = $state<string | undefined>();
	let archivedError = $state<string | undefined>();
	let inFlightOrg: string | undefined;
	let inFlightPromise: Promise<void> | undefined;
	let archivedSeq = 0;
	const archivedLoading = $derived(archivedForOrg !== organization.id);
	const teams = $derived(teamOptions(snapshot).filter((o) => !archived.has(o.team.id)));
	// Every current employee, the owner's person included: ownership is no employee field (§7.1, OWNER-2).
	const managers = $derived(
		withoutFormer(snapshot)
			.members.filter((m) => m.kind === 'person' || m.kind === 'agent')
			.toSorted(byName)
	);

	let open = $state(false);
	let name = $state('');
	let handle = $state('');
	/** Once the person types a handle it is theirs: the name no longer rewrites it. */
	let handleEdited = $state(false);
	let contact = $state('');
	let homeTeamId = $state('');
	let reportsToId = $state('');
	let touched = $state(false);
	let busy = $state(false);
	/** HQ's refusal of the handle (taken, invalid), shown on the field. */
	let handleRefusal = $state<string | undefined>();
	let failure = $state<string | undefined>();

	const nameError = $derived(touched ? nameProblem(name) : undefined);
	const handleError = $derived(
		handleRefusal ?? (touched || handleEdited ? handleProblem(handle) : undefined)
	);

	function refreshArchived(orgId: string, force = false) {
		// Reuse an in-flight read for the same organization instead of superseding it.
		if (!force && inFlightOrg === orgId && inFlightPromise) return inFlightPromise;
		const seq = ++archivedSeq;
		inFlightOrg = orgId;
		archivedError = undefined;
		const promise = teamsApi
			.list(orgId)
			.then(
				(rows) => {
					if (seq !== archivedSeq || orgId !== organization.id) return;
					archived = new Set(rows.filter((t) => t.status === 'archived').map((t) => t.id));
					archivedForOrg = orgId;
					archivedError = undefined;
				},
				(e) => {
					if (seq !== archivedSeq || orgId !== organization.id) return;
					// Never mark an unverified initial read as ready; keep picker gated and offer Retry.
					if (archivedForOrg !== orgId) archivedError = toAppError(e).message;
				}
			)
			.finally(() => {
				if (inFlightPromise === promise) {
					inFlightOrg = undefined;
					inFlightPromise = undefined;
				}
			});
		inFlightPromise = promise;
		return promise;
	}

	$effect(() => {
		const orgId = organization.id;
		const isOpen = open;
		untrack(() => {
			if (archivedForOrg && archivedForOrg !== orgId) {
				archived = new Set();
				archivedForOrg = undefined;
				archivedError = undefined;
			}
			if (isOpen || archivedForOrg !== orgId) {
				void refreshArchived(orgId);
			}
		});
	});

	function start() {
		name = '';
		handle = '';
		handleEdited = false;
		contact = '';
		homeTeamId = teamId;
		reportsToId = '';
		touched = false;
		busy = false;
		handleRefusal = undefined;
		failure = undefined;
		open = true;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || archivedLoading) return;
		touched = true;
		const form = event.currentTarget as HTMLFormElement;
		if (nameProblem(name) || handleProblem(handle) || !homeTeamId) {
			await tick();
			form.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
			return;
		}
		busy = true;
		failure = undefined;
		handleRefusal = undefined;
		const added = name.trim();
		try {
			const { memberId } = await teamsApi.addPerson(organization.id, {
				name: added,
				handle: handle.trim(),
				homeTeamId,
				contact: contact.trim() || undefined,
				reportsToId: reportsToId || undefined
			});
			open = false;
			await onadded({ memberId, name: added });
			toast.success(`Added ${added}. Agents can now reach ${added}.`);
		} catch (e) {
			const refused = toAppError(e);
			if ((refused as { field?: string }).field === 'handle') {
				handleRefusal = refused.message;
				// The field is enabled again before it takes focus.
				busy = false;
				await tick();
				form.querySelector<HTMLElement>('#team-person-handle')?.focus();
			} else failure = refused.message;
		} finally {
			busy = false;
		}
	}
</script>

<Button class="max-[639px]:h-11" onclick={start}>
	<MiniIcon name="plus" class="-ml-0.5 size-5" /> Add a person
</Button>

<Sheet bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<SheetContent side="right" class="gap-0 data-[side=right]:w-full data-[side=right]:sm:max-w-md">
		<SheetHeader class="border-b border-line">
			<SheetTitle>Add person</SheetTitle>
			<SheetDescription>
				Once added, agents in {organization.name} can reach them as a participant.
			</SheetDescription>
		</SheetHeader>
		<form class="flex min-h-0 flex-1 flex-col" onsubmit={submit} novalidate>
			<div class="flex min-h-0 flex-1 flex-col gap-y-5 overflow-y-auto p-4">
				<div>
					<Label for="team-person-name">Name</Label>
					<Input
						id="team-person-name"
						bind:value={name}
						disabled={busy}
						autocomplete="off"
						class="mt-2 {inputClass}"
						aria-invalid={nameError ? true : undefined}
						aria-describedby={nameError ? 'team-person-name-error' : undefined}
						oninput={() => {
							if (!handleEdited) handle = suggestHandle(name);
							handleRefusal = undefined;
						}}
					/>
					{#if nameError}<p id="team-person-name-error" class={errorClass}>{nameError}</p>{/if}
				</div>

				<div>
					<Label for="team-person-handle">Handle</Label>
					<div class="mt-2 flex items-center gap-x-2">
						<span aria-hidden="true" class="text-sm/6 text-fg-muted">@</span>
						<Input
							id="team-person-handle"
							bind:value={handle}
							disabled={busy}
							autocomplete="off"
							autocapitalize="none"
							spellcheck={false}
							class={inputClass}
							aria-invalid={handleError ? true : undefined}
							aria-describedby={handleError
								? 'team-person-handle-error'
								: 'team-person-handle-help'}
							oninput={() => {
								handleEdited = true;
								handleRefusal = undefined;
							}}
						/>
					</div>
					{#if handleError}
						<p id="team-person-handle-error" class="{errorClass} [overflow-wrap:anywhere]">
							{handleError}
						</p>
					{:else}
						<p id="team-person-handle-help" class={helpClass}>
							How agents address them. Lowercase, unique across people and agents. Renaming them
							later keeps it.
						</p>
					{/if}
				</div>

				<div>
					<Label for="team-person-contact"
						>Contact <span class="font-normal text-fg-muted">(optional)</span></Label
					>
					<Input
						id="team-person-contact"
						bind:value={contact}
						disabled={busy}
						autocomplete="off"
						class="mt-2 {inputClass}"
						aria-describedby="team-person-contact-help"
					/>
					<p id="team-person-contact-help" class={helpClass}>
						How to reach them outside Agentic Org, such as an email.
					</p>
				</div>

				<div>
					<Label for="team-person-home">Home team</Label>
					<select
						id="team-person-home"
						bind:value={homeTeamId}
						disabled={busy || archivedLoading}
						aria-busy={archivedLoading && !archivedError}
						required
						class="mt-2 {selectClass}"
						aria-describedby={archivedError ? 'team-person-home-error' : 'team-person-home-help'}
					>
						{#if archivedLoading}
							<option value={homeTeamId}
								>{archivedError ? "Couldn't load active teams" : 'Loading teams…'}</option
							>
						{:else}
							{#each teams as option (option.team.id)}
								<option value={option.team.id}>{option.path}</option>
							{/each}
						{/if}
					</select>
					{#if archivedError}
						<div
							id="team-person-home-error"
							role="alert"
							class="mt-2 rounded-md border border-line p-3 text-sm/6"
						>
							<p class="font-semibold text-danger-text">Couldn't check active teams</p>
							<p class="mt-1 text-fg-secondary [overflow-wrap:anywhere]">{archivedError}</p>
							<Button
								type="button"
								variant="outline"
								class="mt-2 min-h-9 max-[639px]:min-h-11"
								onclick={() => void refreshArchived(organization.id, true)}>Retry</Button
							>
						</div>
					{:else}
						<p id="team-person-home-help" class={helpClass}>
							One team, required. They can be moved later from their profile.
						</p>
					{/if}
				</div>

				<div>
					<Label for="team-person-manager"
						>Reports to <span class="font-normal text-fg-muted">(optional)</span></Label
					>
					<select
						id="team-person-manager"
						bind:value={reportsToId}
						disabled={busy}
						class="mt-2 {selectClass}"
					>
						<option value="">No explicit manager</option>
						{#each managers as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
					</select>
				</div>

				{#if failure}
					<div role="alert" class="rounded-md border border-line p-3 text-sm/6">
						<p class="font-semibold text-danger-text">Couldn't add the person</p>
						<p class="mt-1 text-fg-secondary [overflow-wrap:anywhere]">{failure}</p>
					</div>
				{/if}
			</div>
			<!-- Outside the scrolling fields, so Cancel and Add person stay in view. -->
			<div
				data-sheet-footer
				class="flex flex-wrap justify-end gap-3 border-t border-line p-4 max-[639px]:[&>*]:flex-1"
			>
				<Button
					type="button"
					variant="outline"
					disabled={busy}
					class="max-[639px]:h-11"
					onclick={() => (open = false)}>Cancel</Button
				>
				<Button
					type="submit"
					disabled={busy || archivedLoading}
					aria-busy={busy}
					class="max-[639px]:h-11">{busy ? 'Adding…' : 'Add person'}</Button
				>
			</div>
		</form>
	</SheetContent>
</Sheet>
