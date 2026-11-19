<!--
  @component
  Organization settings › Roles (roles-ux §1): every shared role with its revision and how many employees reference
  it, A–Z. New role… is the one primary action; Ask HR to draft… is offered when the HR lead can run and otherwise
  disabled with the reason. A role file whose details aren't recognized says so on its row, with Fix.
  States: loading, unavailable, error with Retry, no roles, organization data unreachable (the last copy), and a
  viewer without edit rights (no actions).
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { toAppError } from '$lib/office/api';
	import { rolesApi } from '../../api';
	import {
		HR_CANT_RUN,
		emptyRolesText,
		employeesText,
		invalidText,
		sortRoles
	} from '../../present';
	import type { Role, RoleDraft, RoleSummary } from '../../types';
	import AskHrDialog from '../ask-hr/AskHrDialog.svelte';
	import RoleEditor, { type EditorMode } from '../editor/RoleEditor.svelte';
	import StaleNotice from '../stale/StaleNotice.svelte';
	import { roleHref } from '../../urls';
	import { focusHeading } from '../../heading';

	let { organizationId }: { organizationId: string } = $props();

	const resource = new Resource(() => rolesApi.list(organizationId));
	$effect(() => void accountZone.ensure(organizationId));

	const overview = $derived(resource.data);
	const canChange = $derived(!!overview?.viewer.canEdit && !overview.stale);
	const roles = $derived(overview ? sortRoles(overview.roles) : []);

	let editorOpen = $state(false);
	let editorMode = $state.raw<EditorMode>({ kind: 'new' });
	let askOpen = $state(false);
	let pendingDraft: RoleDraft | undefined;
	let opener: HTMLElement | null = null;
	let leaving = false;
	let fixing = $state<string | undefined>();
	let fixError = $state<{ id: string; message: string } | undefined>();

	function newRole(event: MouseEvent) {
		opener = event.currentTarget as HTMLElement;
		editorMode = { kind: 'new' };
		editorOpen = true;
	}

	function askHr(event: MouseEvent) {
		opener = event.currentTarget as HTMLElement;
		askOpen = true;
	}

	// The draft opens in the editor once the dialog has finished closing: one overlay at a time.
	function afterAsk() {
		if (!pendingDraft) return;
		editorMode = { kind: 'new', draft: pendingDraft, fromHr: true };
		pendingDraft = undefined;
		editorOpen = true;
	}

	async function fix(event: MouseEvent, summary: RoleSummary) {
		if (fixing) return;
		opener = event.currentTarget as HTMLElement;
		fixing = summary.id;
		fixError = undefined;
		try {
			const detail = await rolesApi.role(organizationId, summary.id);
			editorMode = { kind: 'edit', role: detail.role, employeeCount: detail.employees.length };
			editorOpen = true;
		} catch (e) {
			fixError = { id: summary.id, message: toAppError(e).message };
		} finally {
			fixing = undefined;
		}
	}

	async function saved(role: Role, created: boolean) {
		if (created) {
			// The new role's page takes over, and focuses its heading.
			leaving = true;
			await goto(resolve(roleHref(role.id) as '/'));
			return;
		}
		await resource.reload();
		// A fixed row loses its Fix button: focus moves to the role's own link rather than the page.
		setTimeout(() => {
			const active = document.activeElement;
			if (active && active !== document.body && active.isConnected) return;
			document.querySelector<HTMLElement>(`[data-role="${CSS.escape(role.id)}"] a`)?.focus();
		}, 0);
	}

	function restoreFocus(event: Event) {
		event.preventDefault();
		if (leaving) return;
		if (opener?.isConnected) opener.focus();
		else focusHeading('roles-heading');
	}
</script>

<PageHeader
	id="roles-heading"
	title="Roles"
	count={overview?.roles.length}
	countLabel={overview?.roles.length === 1 ? 'role' : 'roles'}
	description="What each employee is for. A role describes work for teammates; it grants no tools, budgets or authority."
>
	{#snippet actions()}
		{#if canChange && overview}
			<Button
				variant="outline"
				class={mobileButton}
				disabled={!overview.hr.canRun}
				aria-describedby={overview.hr.canRun ? undefined : 'roles-hr-reason'}
				onclick={askHr}>Ask HR to draft…</Button
			>
			<Button class={mobileButton} onclick={newRole}
				><MiniIcon name="plus" class="-ml-0.5 size-5" />New role…</Button
			>
			{#if !overview.hr.canRun}
				<p
					id="roles-hr-reason"
					class="w-0 min-w-full basis-full text-xs/5 text-gray-400 min-[640px]:text-right"
					data-testid="hr-reason"
				>
					{HR_CANT_RUN}
				</p>
			{/if}
		{/if}
	{/snippet}
</PageHeader>

{#if overview?.stale}
	<StaleNotice
		stale={overview.stale}
		retrying={resource.reloading}
		onretry={() => resource.reload()}
	/>
{/if}

<Loaded
	{resource}
	label="roles"
	error="Couldn't load the roles"
	place="Roles"
	area="Organization settings"
>
	{#snippet children(data)}
		{#if data.roles.length === 0}
			<EmptyState title="No roles yet" description={emptyRolesText(data.hr.canRun)} level={3}>
				{#snippet action()}
					{#if canChange}
						<Button class={mobileButton} onclick={newRole}
							><MiniIcon name="plus" class="-ml-0.5 size-5" />New role…</Button
						>
					{/if}
				{/snippet}
			</EmptyState>
		{:else}
			<div
				class="hidden border-b border-white/10 px-4 py-3 text-xs/5 font-semibold text-gray-400 min-[640px]:flex min-[640px]:gap-x-4 min-[640px]:px-6 min-[1024px]:px-8"
				aria-hidden="true"
			>
				<span class="min-w-0 flex-1">Role</span>
				<span class="w-16 shrink-0">Revision</span>
				<span class="w-36 shrink-0">Employees</span>
			</div>
			<ul role="list" class="divide-y divide-white/5" aria-labelledby="roles-heading">
				{#each roles as role (role.id)}
					<li
						class="relative flex flex-wrap items-start gap-x-4 gap-y-2 px-4 py-4 transition-colors duration-150 hover:bg-white/[0.02] motion-reduce:transition-none min-[640px]:flex-nowrap min-[640px]:px-6 min-[1024px]:px-8"
						data-role={role.id}
					>
						<div class="min-w-0 flex-1 basis-full min-[640px]:basis-auto">
							<p class="text-sm/6 font-semibold text-white">
								<a
									href={resolve(roleHref(role.id) as '/')}
									class="rounded-sm [overflow-wrap:anywhere] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400"
									><span class="absolute inset-0" aria-hidden="true"></span>{role.name}</a
								>
							</p>
							{#if role.key}<p
									class="mt-1 font-mono text-xs/5 text-gray-400 [overflow-wrap:anywhere]"
								>
									{role.key}
								</p>{/if}
							<!-- Wide rows show these in their columns; the text stays for screen readers. -->
							<p class="mt-1 text-xs/5 text-gray-400 min-[640px]:sr-only">
								<span class="whitespace-nowrap">rev {role.revision}</span> ·
								<span class="whitespace-nowrap">{employeesText(role.employeeCount)}</span>
							</p>
							{#if role.invalid}
								<div class="relative mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
									<span class="text-xs/5 text-red-300 [overflow-wrap:anywhere]"
										>{invalidText(role.name, role.invalid.field)}</span
									>
									{#if canChange}
										<Button
											variant="outline"
											size="sm"
											class="min-w-16 {mobileButton}"
											aria-label="Fix {role.name}"
											aria-busy={fixing === role.id}
											onclick={(e: MouseEvent) => fix(e, role)}
											>{fixing === role.id ? 'Opening…' : 'Fix'}</Button
										>
									{/if}
								</div>
								{#if fixError?.id === role.id}
									<p class="relative mt-2 text-xs/5 text-red-300" role="alert">
										{fixError.message}
									</p>
								{/if}
							{/if}
						</div>
						<p
							class="hidden w-16 shrink-0 text-sm/6 text-gray-300 tabular-nums min-[640px]:block"
							aria-hidden="true"
						>
							rev {role.revision}
						</p>
						<p
							class="hidden w-36 shrink-0 text-sm/6 text-gray-300 min-[640px]:block"
							aria-hidden="true"
						>
							{employeesText(role.employeeCount)}
						</p>
					</li>
				{/each}
			</ul>
		{/if}
	{/snippet}
</Loaded>

<AskHrDialog
	bind:open={askOpen}
	{organizationId}
	ondrafted={(draft) => (pendingDraft = draft)}
	onclosed={afterAsk}
	onCloseAutoFocus={(e) => (pendingDraft || editorOpen ? e.preventDefault() : restoreFocus(e))}
/>
<RoleEditor
	bind:open={editorOpen}
	{organizationId}
	mode={editorMode}
	onsaved={saved}
	onCloseAutoFocus={restoreFocus}
/>
