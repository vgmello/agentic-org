<!--
  @component
  Development processes: how a project builds software (processes-ux decision 7; development-processes design §2).
  Every development process, built-in ones first, each with its summary, its seed roles, the tabs it gives a project,
  the sections it starts one with and the projects that follow it. A draft is listed and can't be chosen for a
  project. Any person of the organization creates one, or imports a process file; a built-in one is copied from its
  page. A project chooses its development process on the project, not here.
  States: loading, unavailable, error with Retry, none yet, the repository unreachable (the last copy, changes
  paused), a viewer who may not change them (no actions).
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { focusHeading } from '$lib/components/focus';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { accountZone } from '$lib/modules/preferences';
	import { pageTitle } from '$lib/nav';
	import { processesApi } from '../../api';
	import StaleNotice from '../../components/StaleNotice.svelte';
	import PageTitle from '../../components/PageTitle.svelte';
	import {
		CHANGES_PAUSED,
		DEV_HEADING,
		DEV_SUBTITLE,
		LIFECYCLE_WORD,
		PROCESSES_URL,
		VIEW_WORD,
		devProcessHref,
		listText,
		projectsText
	} from '../../present';
	import type { DevProcessCatalogue } from '../../types';
	import ImportDialog from './ImportDialog.svelte';
	import KeyNameDialog from './KeyNameDialog.svelte';

	let { organizationId }: { organizationId: string } = $props();

	const resource = new Resource(() => processesApi.devProcesses(organizationId));
	$effect(() => void accountZone.ensure(organizationId));
	const catalogue = $derived(resource.data);
	const canEdit = $derived(!!catalogue?.viewer.canEdit);
	const paused = $derived(!!catalogue?.stale);

	let creator = $state<ReturnType<typeof KeyNameDialog>>();
	let importer = $state<ReturnType<typeof ImportDialog>>();
	let opener: HTMLElement | null = null;
	let leaving = false;

	async function created(processId: string) {
		leaving = true;
		await goto(resolve(devProcessHref(processId) as '/'));
	}
	function restoreFocus(event: Event) {
		event.preventDefault();
		if (leaving) return;
		if (opener?.isConnected) opener.focus();
		else focusHeading('dev-heading');
	}
</script>

{#snippet actions(data: DevProcessCatalogue, idPrefix: string)}
	<div class="flex flex-wrap gap-3 {idPrefix === 'empty' ? 'justify-center' : ''}">
		<Button
			variant="outline"
			class={mobileButton}
			disabled={paused}
			aria-describedby={paused ? 'dev-paused' : undefined}
			onclick={(e: MouseEvent) => {
				opener = e.currentTarget as HTMLElement;
				importer?.start();
			}}>Import…</Button
		>
		<Button
			class={mobileButton}
			disabled={paused}
			aria-describedby={paused ? 'dev-paused' : undefined}
			onclick={(e: MouseEvent) => {
				opener = e.currentTarget as HTMLElement;
				creator?.start();
			}}><MiniIcon name="plus" class="-ml-0.5 size-5" />New development process…</Button
		>
	</div>
	{#if paused && data}
		<p id="dev-paused" class="w-full text-xs/5 text-fg-muted">{CHANGES_PAUSED}</p>
	{/if}
{/snippet}

{#if resource.state !== 'unavailable'}
	<PageTitle title={pageTitle(DEV_HEADING, 'Processes')} />
	<PageHeader
		id="dev-heading"
		title="{DEV_HEADING}: {DEV_SUBTITLE.charAt(0).toLowerCase()}{DEV_SUBTITLE.slice(1)}"
		count={catalogue?.processes.length}
		countLabel={catalogue?.processes.length === 1 ? 'development process' : 'development processes'}
		back={{ href: PROCESSES_URL, label: 'Organization processes and standards' }}
		class="[&_h2:focus]:outline-none"
	>
		{#snippet description()}
			<p>
				A project follows at most one, chosen on the project. It names the roles that start the
				project, what each role does, and the tabs and sections the project has. It grants nothing,
				and never overrides a process or standard marked not overridable.
			</p>
		{/snippet}
		{#snippet actions()}
			{#if catalogue && canEdit && catalogue.processes.length > 0}
				{@render actionsBar(catalogue)}
			{/if}
		{/snippet}
	</PageHeader>
{/if}

{#snippet actionsBar(data: DevProcessCatalogue)}{@render actions(data, 'header')}{/snippet}

{#if catalogue?.stale}
	<StaleNotice
		stale={catalogue.stale}
		retrying={resource.reloading}
		onretry={() => resource.reload()}
	/>
{/if}

<Loaded
	{resource}
	label="development processes"
	error="Couldn't load the development processes"
	place="Processes"
	area="Organization settings"
>
	{#snippet children(data)}
		{#if data.processes.length === 0}
			<EmptyState
				level={3}
				title="No development processes yet"
				description="A development process says how a project builds software. Create one, or import a process file."
			>
				{#snippet action()}
					{#if canEdit}{@render actions(data, 'empty')}{/if}
				{/snippet}
			</EmptyState>
		{:else}
			<ul role="list" class="divide-y divide-line-subtle" aria-labelledby="dev-heading">
				{#each data.processes as p (p.id)}
					<li
						class="relative px-4 py-5 transition-colors duration-150 hover:bg-tint/[0.02] motion-reduce:transition-none min-[640px]:px-6 min-[1024px]:px-8"
						data-testid="dev-row"
						data-key={p.key}
					>
						<div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
							<p class="min-w-0 text-sm/6 font-semibold text-fg">
								<a
									href={resolve(devProcessHref(p.id) as '/')}
									class="rounded-sm [overflow-wrap:anywhere] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
									><span class="absolute inset-0" aria-hidden="true"></span>{p.name}</a
								>
							</p>
							<p class="flex flex-wrap gap-x-2 text-xs/5 text-fg-muted">
								<span class="whitespace-nowrap"
									>{p.origin === 'built_in' ? 'Built in' : "Organization's own"}</span
								>
								<span aria-hidden="true">·</span>
								<span
									class="whitespace-nowrap {p.status === 'current'
										? ''
										: 'font-semibold text-fg-secondary'}"
									data-testid="lifecycle">{LIFECYCLE_WORD[p.status]}</span
								>
								{#if p.versionLabel}<span aria-hidden="true">·</span><span class="whitespace-nowrap"
										>Version {p.versionLabel}</span
									>{/if}
							</p>
						</div>
						<p class="mt-1 font-mono text-xs/5 text-fg-muted [overflow-wrap:anywhere]">{p.key}</p>
						{#if p.summary}<p class="mt-2 max-w-prose text-sm/6 text-fg-body">{p.summary}</p>{/if}
						<dl
							class="mt-3 grid gap-x-6 gap-y-1 text-xs/5 min-[640px]:grid-cols-[auto_minmax(0,1fr)]"
						>
							<dt class="text-fg-muted">Seed roles</dt>
							<dd class="text-fg-secondary [overflow-wrap:anywhere]">
								{p.seedRoles.length ? listText(p.seedRoles) : 'None'}
							</dd>
							<dt class="text-fg-muted">Tabs it gives a project</dt>
							<dd class="text-fg-secondary [overflow-wrap:anywhere]">
								{p.tabs.length
									? p.tabs.map((t) => `${t.title} (${VIEW_WORD[t.view].toLowerCase()})`).join(', ')
									: 'Only the fixed tabs'}
							</dd>
							<dt class="text-fg-muted">Sections it starts with</dt>
							<dd class="text-fg-secondary [overflow-wrap:anywhere]">
								{p.folders.length ? listText(p.folders) : 'None'}
							</dd>
							<dt class="text-fg-muted">Followed by</dt>
							<dd class="text-fg-secondary [overflow-wrap:anywhere]">
								{p.projects.length ? listText(p.projects) : projectsText(0)}
							</dd>
						</dl>
					</li>
				{/each}
			</ul>
		{/if}
	{/snippet}
</Loaded>

<KeyNameDialog
	bind:this={creator}
	title="New development process"
	description="It starts as a draft with no roles or skills. A draft is visible to people and can't be chosen for a project."
	confirmLabel="Create"
	busyLabel="Creating…"
	onsubmit={async (key, name) => {
		const { processId } = await processesApi.createDevProcess(organizationId, key, name);
		await created(processId);
	}}
	onCloseAutoFocus={restoreFocus}
/>
<ImportDialog
	bind:this={importer}
	onimport={async (file, key) => {
		const { processId } = await processesApi.importDevProcess(organizationId, file, key);
		await created(processId);
	}}
	onCloseAutoFocus={restoreFocus}
/>
