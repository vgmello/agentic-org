<!--
  @component
  Organization settings › Roles (roles-ux §1): every shared role with its revision and how many employees reference
  it, A–Z. New role… is the one primary action; Ask HR to draft… is offered when the HR lead can run and otherwise
  disabled with the reason beneath it. A role file whose details aren't recognized says so on its row, with Fix.
  With no roles the empty state holds the actions, so the view still has one primary.
  States: loading, unavailable, error with Retry, no roles, organization data unreachable (the last copy: the actions
  stay, disabled with the reason, RL-8), and a viewer without edit rights (no actions, §0.1).
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
	import { accountZone } from '$lib/modules/preferences';
	import { toAppError } from '$lib/proto';
	import { rolesApi } from '../../api';
	import {
		CHANGES_PAUSED,
		HR_CANT_RUN,
		emptyRolesText,
		employeesText,
		invalidText,
		sortRoles
	} from '../../present';
	import type { Role, RoleDraft, RoleSummary, RolesOverview } from '../../types';
	import AskHrDialog from '../ask-hr/AskHrDialog.svelte';
	import RoleEditor from '../editor/RoleEditor.svelte';
	import StaleNotice from '../stale/StaleNotice.svelte';
	import { roleHref } from '../../urls';
	import { focusHeading } from '$lib/components/focus';

	let { organizationId }: { organizationId: string } = $props();

	const resource = new Resource(() => rolesApi.list(organizationId));
	$effect(() => void accountZone.ensure(organizationId));

	const overview = $derived(resource.data);
	/** The viewer may change roles at all; viewers without the right see no actions (§0.1). */
	const mayEdit = $derived(!!overview?.viewer.canEdit);
	/** Changes are paused while organization data can't be reached: the actions say why (RL-8). */
	const paused = $derived(!!overview?.stale);
	const canChange = $derived(mayEdit && !paused);
	const roles = $derived(overview ? sortRoles(overview.roles) : []);

	let editor = $state<ReturnType<typeof RoleEditor>>();
	let askDialog = $state<ReturnType<typeof AskHrDialog>>();
	let pendingDraft: RoleDraft | undefined;
	let opener: HTMLElement | null = null;
	let leaving = false;
	let fixing = $state<string | undefined>();
	let fixError = $state<{ id: string; message: string } | undefined>();

	function newRole(event: MouseEvent) {
		opener = event.currentTarget as HTMLElement;
		editor?.start({ kind: 'new' });
	}

	function askHr(event: MouseEvent) {
		opener = event.currentTarget as HTMLElement;
		askDialog?.start();
	}

	// The draft opens in the editor once the dialog has finished closing: one overlay at a time.
	function afterAsk() {
		if (!pendingDraft) return;
		const draft = pendingDraft;
		pendingDraft = undefined;
		editor?.start({ kind: 'new', draft, fromHr: true });
	}

	async function fix(event: MouseEvent, summary: RoleSummary) {
		if (fixing) return;
		opener = event.currentTarget as HTMLElement;
		fixing = summary.id;
		fixError = undefined;
		try {
			const detail = await rolesApi.role(organizationId, summary.id);
			editor?.start({ kind: 'edit', role: detail.role, employeeCount: detail.employees.length });
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

<!-- Ask HR to draft… and New role…, each disabled with its reason beside it when it can't be used now. -->
{#snippet roleActions(data: RolesOverview, idPrefix: string)}
	{@const hrReason = paused ? undefined : data.hr.canRun ? undefined : `${idPrefix}-hr-reason`}
	{@const pausedReason = paused ? `${idPrefix}-paused-reason` : undefined}
	<div class="flex flex-wrap items-start gap-3 {idPrefix === 'empty' ? 'justify-center' : ''}">
		<div class="flex max-w-60 flex-col items-start gap-y-1">
			<Button
				variant="outline"
				class={mobileButton}
				disabled={paused || !data.hr.canRun}
				aria-describedby={pausedReason ?? hrReason}
				onclick={askHr}>Ask HR to draft…</Button
			>
			{#if hrReason}
				<p
					id={hrReason}
					class="text-xs/5 text-fg-muted {idPrefix === 'empty' ? 'text-center' : 'text-left'}"
					data-testid="hr-reason"
				>
					{HR_CANT_RUN}
				</p>
			{/if}
		</div>
		<Button class={mobileButton} disabled={paused} aria-describedby={pausedReason} onclick={newRole}
			><MiniIcon name="plus" class="-ml-0.5 size-5" />New role…</Button
		>
		{#if pausedReason}
			<p
				id={pausedReason}
				class="w-0 min-w-full basis-full text-xs/5 text-fg-muted {idPrefix === 'empty'
					? 'text-center'
					: 'text-left'}"
				data-testid="paused-reason"
			>
				{CHANGES_PAUSED}
			</p>
		{/if}
	</div>
{/snippet}

<PageHeader
	id="roles-heading"
	title="Roles"
	count={overview?.roles.length}
	countLabel={overview?.roles.length === 1 ? 'role' : 'roles'}
	description="What each employee is for. A role describes work for teammates; it grants no tools, budgets or authority."
>
	{#snippet actions()}
		{#if overview && mayEdit && overview.roles.length > 0}
			{@render roleActions(overview, 'roles')}
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
			<!-- The empty state owns the actions (the header has none), so there is one primary. Its words only
				 invite what the viewer can do now (ROLE-10). -->
			<EmptyState
				title="No roles yet"
				description={emptyRolesText(data.hr.canRun, canChange)}
				level={3}
			>
				{#snippet action()}
					{#if mayEdit}{@render roleActions(data, 'empty')}{/if}
				{/snippet}
			</EmptyState>
		{:else}
			<div
				class="hidden border-b border-line px-4 py-3 text-xs/5 font-semibold text-fg-muted min-[640px]:flex min-[640px]:gap-x-4 min-[640px]:px-6 min-[1024px]:px-8"
				aria-hidden="true"
			>
				<span class="min-w-0 flex-1">Role</span>
				<span class="w-16 shrink-0">Revision</span>
				<span class="w-36 shrink-0">Employees</span>
			</div>
			<ul role="list" class="divide-y divide-line-subtle" aria-labelledby="roles-heading">
				{#each roles as role (role.id)}
					<li
						class="relative flex flex-wrap items-start gap-x-4 gap-y-2 px-4 py-4 transition-colors duration-150 hover:bg-tint/[0.02] motion-reduce:transition-none min-[640px]:flex-nowrap min-[640px]:px-6 min-[1024px]:px-8"
						data-role={role.id}
					>
						<div class="min-w-0 flex-1 basis-full min-[640px]:basis-auto">
							<p class="text-sm/6 font-semibold text-fg">
								<a
									href={resolve(roleHref(role.id) as '/')}
									class="rounded-sm [overflow-wrap:anywhere] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
									><span class="absolute inset-0" aria-hidden="true"></span>{role.name}</a
								>
							</p>
							{#if role.key}<p
									class="mt-1 font-mono text-xs/5 text-fg-muted [overflow-wrap:anywhere]"
								>
									{role.key}
								</p>{/if}
							<!-- Wide rows show these in their columns; the text stays for screen readers. -->
							<p class="mt-1 text-xs/5 text-fg-muted min-[640px]:sr-only">
								<span class="whitespace-nowrap">rev {role.revision}</span> ·
								<span class="whitespace-nowrap">{employeesText(role.employeeCount)}</span>
							</p>
							{#if role.invalid}
								<div class="relative mt-2 flex flex-wrap items-center gap-x-3 gap-y-2">
									<span class="text-xs/5 text-danger-text [overflow-wrap:anywhere]"
										>{invalidText(role.name, role.invalid.field)}</span
									>
									{#if mayEdit}
										<Button
											variant="outline"
											size="sm"
											class="min-w-16 {mobileButton}"
											aria-label="Fix {role.name}"
											disabled={paused}
											aria-describedby={paused ? 'roles-paused-reason' : undefined}
											aria-busy={fixing === role.id}
											onclick={(e: MouseEvent) => fix(e, role)}
											>{fixing === role.id ? 'Opening…' : 'Fix'}</Button
										>
									{/if}
								</div>
								{#if fixError?.id === role.id}
									<p class="relative mt-2 text-xs/5 text-danger-text" role="alert">
										{fixError.message}
									</p>
								{/if}
							{/if}
						</div>
						<p
							class="hidden w-16 shrink-0 text-sm/6 text-fg-secondary tabular-nums min-[640px]:block"
							aria-hidden="true"
						>
							rev {role.revision}
						</p>
						<p
							class="hidden w-36 shrink-0 text-sm/6 text-fg-secondary min-[640px]:block"
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
	bind:this={askDialog}
	{organizationId}
	ondrafted={(draft) => (pendingDraft = draft)}
	onclosed={afterAsk}
	onCloseAutoFocus={(e) => (pendingDraft ? e.preventDefault() : restoreFocus(e))}
/>
<RoleEditor bind:this={editor} {organizationId} onsaved={saved} onCloseAutoFocus={restoreFocus} />
