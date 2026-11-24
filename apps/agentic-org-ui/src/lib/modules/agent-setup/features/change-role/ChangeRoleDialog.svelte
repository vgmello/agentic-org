<!--
  @component
  Change an agent's role (our-org-in-the-app §5: role editing lives in Agent setup; roles-ux): the employee record's
  `role_id`, or none, against the employee revision the person saw. Execution isn't touched, and a role grants no
  tools, budgets or authority. A conflict shows the newer record behind the dialog and keeps the choice; any other
  refusal stays inline. Mount it in a {#key} that changes on each opening.
-->
<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { helpClass, mobileButton, selectClass } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/proto';
	import { agentSetupApi } from '../../api';
	import type { AgentSetupDetail } from '../../types';
	import { latestOf } from '../agent-page/lookup';

	let {
		open = $bindable(false),
		organizationId,
		agent,
		onlatest,
		onsaved,
		onCloseAutoFocus
	}: {
		open: boolean;
		organizationId: string;
		agent: AgentSetupDetail;
		onlatest: (latest: AgentSetupDetail) => void;
		onsaved: (saved: AgentSetupDetail) => void;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	const ids = $props.id();
	let roleId = $state(untrack(() => agent.roleId ?? ''));
	let busy = $state(false);
	let error = $state<string | undefined>();
	let nowSaved = $state<string | undefined>();
	let alertElement = $state<HTMLElement | null>(null);

	function changeOpen(next: boolean) {
		if (!next && busy) open = true;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		if (roleId === (agent.roleId ?? '')) return void (open = false);
		busy = true;
		error = undefined;
		try {
			const result = await agentSetupApi.setRole(organizationId, agent.memberId, {
				roleId: roleId || null,
				expectedRevision: agent.employeeRevision
			});
			open = false;
			onsaved(result);
		} catch (e) {
			const failure = toAppError(e);
			const latest = failure.code === 'conflict' ? latestOf(e) : undefined;
			if (latest) {
				onlatest(latest);
				nowSaved = `Saved now, rev ${latest.employeeRevision}: ${latest.role ?? 'no role'}.`;
			} else nowSaved = undefined;
			error = failure.message;
			busy = false;
			// A refusal focuses its reason (app-shell-ux §6a); the next Tab reaches Cancel and Save.
			await tick();
			alertElement?.focus();
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={changeOpen}>
	<DialogContent
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4 sm:max-w-lg"
	>
		<form class="grid gap-6" onsubmit={save} novalidate>
			<DialogHeader class="pr-8">
				<DialogTitle text="Change {agent.name}'s role" />
				<DialogDescription>
					A role describes what {agent.name} is for, for its teammates. It grants no tools, budgets or
					authority, and doesn't change how it runs.
				</DialogDescription>
			</DialogHeader>
			<div>
				<Label for="{ids}-role">Role</Label>
				<select
					id="{ids}-role"
					bind:value={roleId}
					disabled={busy}
					class="mt-2 {selectClass}"
					aria-describedby="{ids}-role-help"
				>
					<option value="">No role</option>
					{#each agent.roles as role (role.id)}
						<option value={role.id}>{role.name}</option>
					{/each}
				</select>
				<p id="{ids}-role-help" class={helpClass}>
					Roles are written in Organization settings › Roles.
				</p>
			</div>
			{#if error}<div
					role="alert"
					tabindex="-1"
					bind:this={alertElement}
					class="text-sm/6 [overflow-wrap:anywhere] focus:outline-hidden"
				>
					<p class="text-danger-text">{error}</p>
					{#if nowSaved}<p class="mt-1 text-fg-secondary">{nowSaved}</p>{/if}
				</div>{/if}
			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => (open = false)}>Cancel</Button
				>
				<Button type="submit" class="min-w-20 {mobileButton}" aria-busy={busy} aria-disabled={busy}
					>{busy ? 'Saving…' : 'Save'}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
