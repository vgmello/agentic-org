<!--
  @component
  One role, `/settings/roles/:roleId` (roles-ux §1, ROLE-1, ROLE-5): back to Roles; its name, key, revision and last
  update; Edit (the one primary action); its text; the employees that reference it; its revision history, earlier
  text one step away; and the record's provenance. States: loading, unavailable, error with Retry, not found,
  organization data unreachable (the last copy; Edit stays, disabled with the reason, RL-8), details not recognized
  (with Fix), read-only viewer (no Edit, §0.1).
-->
<script lang="ts">
	import { afterNavigate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Avatar from '$lib/components/Avatar.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import DescriptionList from '$lib/components/DescriptionList.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { eventTime } from '$lib/modules/preferences/time';
	import { pageTitle } from '$lib/nav';
	import { rolesApi } from '../../api';
	import { CHANGES_PAUSED, actorName, employeesText, invalidText } from '../../present';
	import type { Attribution, Role } from '../../types';
	import { ROLES_URL } from '../../urls';
	import RoleEditor from '../editor/RoleEditor.svelte';
	import StaleNotice from '../stale/StaleNotice.svelte';
	import { focusHeading } from '$lib/components/focus';
	import { lookUpRole } from './lookup';
	import RoleBody from './RoleBody.svelte';

	let { organizationId, roleId }: { organizationId: string; roleId: string } = $props();

	const resource = new Resource(() => lookUpRole(() => rolesApi.role(organizationId, roleId)));
	$effect(() => void accountZone.ensure(organizationId));

	const detail = $derived(resource.data?.found ? resource.data.detail : undefined);
	const mayEdit = $derived(!!detail?.viewer.canEdit);
	const paused = $derived(!!detail?.stale);
	const title = $derived(
		resource.state !== 'ready'
			? pageTitle('Role', 'Roles', 'Organization settings')
			: detail
				? pageTitle(detail.role.name, 'Roles', 'Organization settings')
				: pageTitle('Role not found', 'Roles', 'Organization settings')
	);
	const now = new Date();

	let editor = $state<ReturnType<typeof RoleEditor>>();
	let opener: HTMLElement | null = null;

	// Arriving by a link or Back, the role's heading takes focus once the role has loaded: after the shell's own
	// focus on the top bar's heading, a tick after the move. A full page load leaves focus where the browser puts it.
	let focusOnLoad = false;
	afterNavigate((move) => {
		focusOnLoad = move.type !== 'enter';
	});
	$effect(() => {
		if (resource.state !== 'ready' || !focusOnLoad) return;
		focusOnLoad = false;
		setTimeout(() => {
			const active = document.activeElement;
			const idle = !active || active === document.body || active.matches('main h1');
			if (idle) focusHeading('role-heading');
		}, 0);
	});

	const when = (a: Attribution) => (a.at ? eventTime(a.at, now, accountZone.zone) : undefined);

	function edit(event: MouseEvent) {
		opener = event.currentTarget as HTMLElement;
		if (detail)
			editor?.start({ kind: 'edit', role: detail.role, employeeCount: detail.employees.length });
	}

	async function saved(role: Role) {
		await resource.reload();
		toast.success(`${role.name} saved as rev ${role.revision}.`);
	}

	function restoreFocus(event: Event) {
		event.preventDefault();
		if (opener?.isConnected) opener.focus();
		else focusHeading('role-heading');
	}
</script>

<svelte:head><title>{title}</title></svelte:head>

{#snippet backLink()}
	<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
		<a
			href={resolve(ROLES_URL)}
			class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
			>Roles</a
		>
	</div>
{/snippet}

{#snippet stamp(a: Attribution)}
	{@const t = when(a)}
	<span class="[overflow-wrap:anywhere]">{actorName(a)}</span>{#if t}<span
			aria-hidden="true"
			class="mx-1">·</span
		><time datetime={t.iso} title={t.full}>{t.text}</time>{/if}
{/snippet}

{#if resource.state === 'error' || resource.state === 'unavailable' || resource.state === 'loading'}
	<!-- The one named back link stays while the role itself can't be shown. -->
	{@render backLink()}
{/if}

<Loaded
	{resource}
	label="the role"
	error="Couldn't load this role"
	place="Roles"
	area="Organization settings"
	rows={5}
>
	{#snippet children(lookup)}
		{#if !lookup.found}
			<PageHeader
				id="role-heading"
				title="Role not found"
				back={{ href: ROLES_URL, label: 'Roles' }}
				description="{lookup.message} It may have been removed, or the link may be incomplete."
			/>
		{:else}
			{@const d = lookup.detail}
			{@const role = d.role}
			<PageHeader id="role-heading" title={role.name} back={{ href: ROLES_URL, label: 'Roles' }}>
				{#snippet description()}
					<!-- Below 640 px each group takes its own line, so no separator is left at a line's start. -->
					<p
						class="flex flex-col gap-y-1 min-[640px]:flex-row min-[640px]:flex-wrap min-[640px]:items-baseline min-[640px]:gap-x-2"
					>
						{#if role.key}<span class="font-mono text-xs/5 [overflow-wrap:anywhere]"
								>{role.key}</span
							><span aria-hidden="true" class="hidden min-[640px]:inline">·</span>{/if}
						<span>rev {role.revision} · {employeesText(d.employees.length)}</span>
						<span aria-hidden="true" class="hidden min-[640px]:inline">·</span>
						<span>Updated by {@render stamp(role.updated)}</span>
					</p>
				{/snippet}
				{#snippet actions()}
					{#if mayEdit}
						<div class="flex max-w-60 flex-col items-start gap-y-1">
							<Button
								class="min-w-20 {mobileButton}"
								disabled={paused}
								aria-describedby={paused ? 'role-paused-reason' : undefined}
								onclick={edit}>Edit</Button
							>
							{#if paused}
								<p
									id="role-paused-reason"
									class="text-xs/5 text-fg-muted"
									data-testid="paused-reason"
								>
									{CHANGES_PAUSED}
								</p>
							{/if}
						</div>
					{/if}
				{/snippet}
			</PageHeader>

			{#if d.stale}
				<StaleNotice
					stale={d.stale}
					retrying={resource.reloading}
					onretry={() => resource.reload()}
				/>
			{:else if role.invalid}
				<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
					<Callout data-testid="invalid">
						<p class="text-sm/6 font-semibold text-danger-text">
							{invalidText(role.name, role.invalid.field)}
						</p>
						<p class="mt-1 text-sm/6">
							The office couldn't read this detail in the role's file, so it isn't shown.
						</p>
						{#if mayEdit}
							<Button variant="outline" class="mt-3 min-w-16 {mobileButton}" onclick={edit}
								>Fix</Button
							>
						{/if}
					</Callout>
				</div>
			{/if}

			<!-- Its own h3, so the body's sections (h4) read as part of the text, not as page sections. -->
			<section
				aria-labelledby="role-text-heading"
				class="border-b border-line-subtle px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8"
				data-testid="role-body"
			>
				<h3 id="role-text-heading" class="sr-only">Role text</h3>
				<RoleBody body={role.bodyMarkdown} level={4} />
			</section>

			<Section
				level={3}
				title="Employees with this role"
				description="Agents read this text as part of who they are. It grants no tools, budgets or authority."
			>
				{#if d.employees.length === 0}
					<p class="px-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
						No employees have this role yet.
					</p>
				{:else}
					<ul
						role="list"
						class="divide-y divide-line-subtle border-y border-line-subtle"
						data-testid="role-employees"
					>
						{#each d.employees as employee (employee.id)}
							<li
								class="relative flex items-center gap-x-4 px-4 py-4 hover:bg-tint/[0.02] min-[640px]:px-6 min-[1024px]:px-8"
							>
								<Avatar name={employee.name} />
								<div class="min-w-0 flex-1">
									<p class="text-sm/6 font-semibold text-fg">
										<a
											href={resolve(`/members/${encodeURIComponent(employee.id)}` as '/')}
											class="rounded-sm [overflow-wrap:anywhere] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
											><span class="absolute inset-0" aria-hidden="true"></span>{employee.name}</a
										>
									</p>
									<p class="mt-1 flex flex-wrap gap-x-2 text-xs/5 text-fg-muted">
										<span>{employee.kind === 'agent' ? 'Agent' : 'Person'}</span>
										{#if employee.title}<span aria-hidden="true">·</span><span
												class="[overflow-wrap:anywhere]">{employee.title}</span
											>{/if}
									</p>
								</div>
							</li>
						{/each}
					</ul>
				{/if}
			</Section>

			<Section level={3} title="History" description="Every saved revision, newest first.">
				<ol
					role="list"
					class="divide-y divide-line-subtle border-y border-line-subtle"
					data-testid="role-history"
				>
					{#each d.history as rev, i (rev.revision)}
						{@const older = d.history[i + 1]}
						<li class="px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
							<p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm/6">
								<span class="font-semibold text-fg">rev {rev.revision}</span>
								{#if i === 0}<span
										class="inline-flex items-center rounded-full bg-quiet/10 px-2 py-1 text-xs/4 font-medium text-fg-secondary ring-1 ring-quiet/20 ring-inset"
										>Current</span
									>{/if}
							</p>
							<p class="mt-1 flex flex-wrap gap-x-2 text-xs/5 text-fg-muted">
								<span>{@render stamp(rev.updated)}</span>
								{#if older && older.name !== rev.name}<span aria-hidden="true">·</span><span
										class="[overflow-wrap:anywhere]">Renamed from {older.name}</span
									>{/if}
								{#if !older}<span aria-hidden="true">·</span><span>Created</span>{/if}
							</p>
							{#if i > 0}
								<details class="group mt-2">
									<summary
										class="inline-flex cursor-pointer list-none items-center rounded-sm text-sm/6 [&::-webkit-details-marker]:hidden font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
										><MiniIcon
											name="chevronDown"
											class="mr-1 size-5 transition-transform duration-150 group-open:rotate-180 motion-reduce:transition-none"
										/>Text of rev {rev.revision}</summary
									>
									<div class="mt-3 border-l-2 border-line pl-4">
										<RoleBody body={rev.bodyMarkdown} level={4} />
									</div>
								</details>
							{/if}
						</li>
					{/each}
				</ol>
			</Section>

			{#snippet created()}{@render stamp(role.created)}{/snippet}
			{#snippet updated()}{@render stamp(role.updated)}{/snippet}
			{#snippet keyValue()}
				{#if role.key}<span class="font-mono [overflow-wrap:anywhere]">{role.key}</span>{:else}No
					key{/if}
				{#if role.keyNamedBy?.length}
					<span class="mt-1 block text-xs/5" data-testid="key-lock"
						>{role.keyNamedBy.join(', ')} uses this key, so it can't be changed or removed.</span
					>
				{/if}
			{/snippet}
			<Section
				level={3}
				title="Record"
				description="Where the role is kept in the organization's records, and who wrote it."
			>
				<DescriptionList
					class="border-y border-line-subtle"
					items={[
						{ term: 'ID', value: role.id, mono: true },
						{ term: 'File', value: `roles/${role.id}.md`, mono: true },
						{ term: 'Key', content: keyValue },
						{ term: 'Schema version', value: String(role.schemaVersion) },
						{ term: 'Created by', content: created },
						{ term: 'Last updated by', content: updated }
					]}
				/>
			</Section>
		{/if}
	{/snippet}
</Loaded>

<RoleEditor bind:this={editor} {organizationId} onsaved={saved} onCloseAutoFocus={restoreFocus} />
