<!--
  @component
  One development process (document 18, development-processes design §2): what it is, the order of work (stages),
  its roles (seed ones start a project), its skills and who each is for, the tabs and sections it gives a project,
  its document kinds and templates, and the projects that follow it. Every file can be read. The organization's own
  can be edited file by file, and new roles, skills and templates added; a built-in one is part of the product and
  is copied to be changed. Any one can be downloaded as a process file.
  States: loading, unavailable, error with Retry, not found, the repository unreachable (changes paused), a viewer
  who may not change it.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import Callout from '$lib/components/Callout.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { focusHeading } from '$lib/components/focus';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { toAppError } from '$lib/office/api';
	import { pageTitle } from '$lib/nav';
	import { processesApi } from '../../api';
	import StaleNotice from '../../components/StaleNotice.svelte';
	import When from '../../components/When.svelte';
	import PageTitle from '../../components/PageTitle.svelte';
	import {
		CHANGES_PAUSED,
		DEV_HEADING,
		DEV_PROCESSES_URL,
		LIFECYCLE_WORD,
		NEW_ROLE_BODY,
		NEW_SKILL_BODY,
		VIEW_WORD,
		devProcessHref,
		listText,
		skillFileText,
		skillRolesText
	} from '../../present';
	import type { DevPart, DevProcessDetail } from '../../types';
	import KeyNameDialog from '../dev-catalogue/KeyNameDialog.svelte';
	import PartSheet, { type PartMode } from './PartSheet.svelte';

	let { organizationId, processId }: { organizationId: string; processId: string } = $props();

	const resource = new Resource(async (): Promise<DevProcessDetail | { notFound: true }> => {
		try {
			return await processesApi.devProcess(organizationId, processId);
		} catch (e) {
			const failure = toAppError(e);
			if (failure.source === 'api' && failure.code === 'not_found') return { notFound: true };
			throw e;
		}
	});
	$effect(() => void accountZone.ensure(organizationId));

	const detail = $derived(resource.data && 'id' in resource.data ? resource.data : undefined);
	const notFound = $derived(!!resource.data && 'notFound' in resource.data);
	const builtIn = $derived(detail?.origin === 'built_in');
	const canEdit = $derived(!!detail?.viewer.canEdit);
	const canWrite = $derived(canEdit && !builtIn);
	const paused = $derived(!!detail?.stale);
	const roleNames = $derived(new Map(detail?.roles.map((r) => [r.key, r.name]) ?? []));

	let sheet = $state<ReturnType<typeof PartSheet>>();
	let copier = $state<ReturnType<typeof KeyNameDialog>>();
	let opener: HTMLElement | null = null;
	let leaving = false;
	let downloading = $state(false);
	let downloadError = $state<string | undefined>();
	let saved = $state<string | undefined>();

	function openPart(event: MouseEvent, mode: PartMode) {
		opener = event.currentTarget as HTMLElement;
		saved = undefined;
		sheet?.start(mode);
	}

	async function save(part: DevPart, content: string, expectedRevision: number) {
		const result = await processesApi.writeDevPart(
			organizationId,
			processId,
			part,
			content,
			expectedRevision
		);
		await resource.reload();
		saved = `Saved ${part.kind === 'manifest' ? 'process.yaml' : `${part.kind}s/${part.key}.md`}: the development process is now rev ${result.revision}.`;
	}

	async function download() {
		if (downloading) return;
		downloading = true;
		downloadError = undefined;
		try {
			const file = await processesApi.exportDevProcess(organizationId, processId);
			const url = URL.createObjectURL(file.blob);
			const link = document.createElement('a');
			link.href = url;
			link.download = file.fileName;
			link.click();
			setTimeout(() => URL.revokeObjectURL(url), 0);
		} catch (e) {
			downloadError = `Couldn't download it. ${toAppError(e).message}`;
		} finally {
			downloading = false;
		}
	}

	function restoreFocus(event: Event) {
		event.preventDefault();
		if (leaving) return;
		if (opener?.isConnected) opener.focus();
		else focusHeading('dev-process-heading');
	}

	const sectionHeading = 'text-sm/6 font-semibold text-fg';
	const rowClass =
		'flex flex-wrap items-start justify-between gap-x-4 gap-y-2 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8';
	const readButton = `${mobileButton} shrink-0`;
</script>

{#if resource.state !== 'unavailable'}
	<PageTitle title={pageTitle(detail?.name, DEV_HEADING, 'Processes')} />

	<PageHeader
		id="dev-process-heading"
		title={detail?.name ?? (notFound ? 'Development process not found' : 'Development process')}
		back={{ href: DEV_PROCESSES_URL, label: 'Development processes' }}
		description={detail?.summary || undefined}
		class="[&_h2:focus]:outline-none"
	>
		{#snippet actions()}
			{#if detail}
				<div class="flex flex-wrap gap-3">
					{#if canWrite}
						<Button
							class={mobileButton}
							disabled={paused}
							aria-describedby={paused ? 'dev-process-paused' : undefined}
							onclick={(e: MouseEvent) =>
								openPart(e, {
									kind: 'edit',
									part: { kind: 'manifest' },
									title: `Edit process.yaml`,
									content: detail!.manifestYaml
								})}>Edit process.yaml</Button
						>
					{/if}
					{#if canEdit}
						<Button
							variant={builtIn ? 'default' : 'outline'}
							class={mobileButton}
							disabled={paused}
							aria-describedby={paused ? 'dev-process-paused' : undefined}
							onclick={(e: MouseEvent) => {
								opener = e.currentTarget as HTMLElement;
								copier?.start({ key: `${detail!.key}-copy`, name: `${detail!.name} (copy)` });
							}}>Copy…</Button
						>
					{/if}
					<Button
						variant="outline"
						class="min-w-28 {mobileButton}"
						aria-busy={downloading}
						aria-disabled={downloading}
						onclick={download}>{downloading ? 'Downloading…' : 'Download'}</Button
					>
				</div>
				{#if paused && canEdit}<p id="dev-process-paused" class="w-full text-xs/5 text-fg-muted">
						{CHANGES_PAUSED}
					</p>{/if}
				{#if downloadError}<p class="w-full text-xs/5 text-danger-text" role="alert">
						{downloadError}
					</p>{/if}
			{/if}
		{/snippet}
	</PageHeader>
{/if}

{#if detail?.stale}
	<StaleNotice
		stale={detail.stale}
		retrying={resource.reloading}
		onretry={() => resource.reload()}
	/>
{/if}

{#if resource.state === 'loading'}
	<SkeletonRows label="the development process" rows={6} />
{:else if resource.state === 'unavailable'}
	<NotBuilt title="Processes" place="Processes" area="Organization settings" />
{:else if resource.state === 'error'}
	<ErrorState
		title="Couldn't load the development process"
		message={resource.message}
		onretry={() => resource.reload()}
		retrying={resource.reloading}
	/>
{:else if notFound}
	<EmptyState
		title="That development process isn't here"
		description="It may have been removed. The catalogue lists the ones there are."
	>
		{#snippet action()}
			<Button variant="outline" class={mobileButton} href={resolve(DEV_PROCESSES_URL as '/')}
				>Development processes</Button
			>
		{/snippet}
	</EmptyState>
{:else if detail}
	<div class="space-y-4 px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
		{#if builtIn}
			<Callout data-testid="built-in">
				<p class="text-sm/6 text-fg-secondary">
					Built in: part of the product, and it changes with it. It can't be edited here; copy it to
					change it.
				</p>
			</Callout>
		{/if}
		{#if saved}<p class="text-sm/6 text-fg-secondary" role="status" data-testid="saved">
				{saved}
			</p>{/if}
		<dl
			class="grid max-w-3xl grid-cols-1 gap-x-6 gap-y-1 text-sm/6 min-[480px]:grid-cols-[auto_minmax(0,1fr)]"
		>
			<dt class="text-fg-muted">Key</dt>
			<dd class="font-mono text-fg-secondary [overflow-wrap:anywhere]">{detail.key}</dd>
			<dt class="text-fg-muted">Comes from</dt>
			<dd class="text-fg-secondary [overflow-wrap:anywhere]">
				{#if builtIn}Built in{:else}The organization{#if detail.copiedFrom}, copied from <a
							href={resolve(devProcessHref(detail.copiedFrom.id) as '/')}
							class="rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
							>{detail.copiedFrom.name}</a
						>{/if}{/if}
			</dd>
			<dt class="text-fg-muted">Lifecycle</dt>
			<dd class="text-fg-secondary" data-testid="lifecycle">
				{LIFECYCLE_WORD[detail.status]}{detail.status === 'draft'
					? ': visible to people, and not offered to projects yet'
					: detail.status === 'deprecated'
						? ': its projects keep working, and it is not offered to new ones'
						: ''}
			</dd>
			{#if detail.versionLabel}<dt class="text-fg-muted">Version</dt>
				<dd class="text-fg-secondary">{detail.versionLabel}</dd>{/if}
			<dt class="text-fg-muted">Owner</dt>
			<dd class="text-fg-secondary">
				{detail.owner?.name ?? (builtIn ? 'Agentic Org' : 'No owner')}
			</dd>
			<dt class="text-fg-muted">Revision</dt>
			<dd class="text-fg-secondary tabular-nums">
				{detail.revision} · changed <When at={detail.updated.at} />{detail.updated.byName
					? ` by ${detail.updated.byName}`
					: ''}
			</dd>
			<dt class="text-fg-muted">Followed by</dt>
			<dd class="text-fg-secondary [overflow-wrap:anywhere]">
				{detail.projects.length ? listText(detail.projects) : 'No project yet'}
			</dd>
		</dl>
	</div>

	<section aria-labelledby="dev-stages" class="mt-8">
		<h3
			id="dev-stages"
			class="border-b border-line px-4 pb-3 {sectionHeading} min-[640px]:px-6 min-[1024px]:px-8"
		>
			Stages <span class="font-normal text-fg-muted"
				>· the order of work, for people and agents</span
			>
		</h3>
		{#if detail.stages.length === 0}
			<p class="px-4 py-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">No stages.</p>
		{:else}
			<ol role="list" class="divide-y divide-line-subtle">
				{#each detail.stages as stage, i (stage.key)}
					<li class="{rowClass} justify-start">
						<span class="w-6 shrink-0 text-sm/6 text-fg-muted tabular-nums">{i + 1}.</span>
						<div class="min-w-0 flex-1">
							<p class="text-sm/6 font-semibold text-fg">{stage.title}</p>
							{#if stage.summary}<p class="text-sm/6 text-fg-body">{stage.summary}</p>{/if}
							{#if stage.leadRole}<p class="text-xs/5 text-fg-muted">
									Led by {roleNames.get(stage.leadRole) ?? stage.leadRole}
								</p>{/if}
						</div>
					</li>
				{/each}
			</ol>
		{/if}
	</section>

	<section aria-labelledby="dev-roles" class="mt-8">
		<div
			class="flex flex-wrap items-end justify-between gap-3 border-b border-line px-4 pb-3 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<h3 id="dev-roles" class={sectionHeading}>
				Roles <span class="font-normal text-fg-muted">{detail.roles.length}</span>
			</h3>
			{#if canWrite}<Button
					variant="outline"
					size="sm"
					class={mobileButton}
					disabled={paused}
					onclick={(e: MouseEvent) =>
						openPart(e, {
							kind: 'edit',
							part: { kind: 'role', key: '' },
							title: 'New role',
							content: NEW_ROLE_BODY,
							isNew: true,
							taken: detail!.roles.map((r) => r.key),
							note: "The role's definition. Add the role to process.yaml's roles too, with its name and whether it is a seed role."
						})}>New role…</Button
				>{/if}
		</div>
		<p class="px-4 pt-3 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
			Seed roles start a project; the others are hired when the work calls for them. Matched to the
			organization's roles by key.
		</p>
		<ul role="list" class="divide-y divide-line-subtle">
			{#each detail.roles as role (role.key)}
				<li class={rowClass} data-testid="dev-role">
					<div class="min-w-0 flex-1 basis-56">
						<p class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
							{role.name}
							{#if role.seed}<span
									class="ml-1 rounded-md bg-surface-raised px-2 py-0.5 text-xs/5 font-medium whitespace-nowrap text-fg-secondary ring-1 ring-line ring-inset"
									>Seed</span
								>{/if}
						</p>
						<p class="font-mono text-xs/5 text-fg-muted">{role.key}</p>
						{#if role.hats.length}<p class="text-xs/5 text-fg-muted">
								Covers {role.hats.join(', ')}
							</p>{/if}
					</div>
					<Button
						variant="outline"
						size="sm"
						class={readButton}
						aria-label="{canWrite ? 'Edit' : 'Read'} the {role.name} role"
						disabled={canWrite && paused}
						onclick={(e: MouseEvent) =>
							openPart(
								e,
								canWrite
									? {
											kind: 'edit',
											part: { kind: 'role', key: role.key },
											title: `Edit roles/${role.key}.md`,
											content: role.bodyMarkdown
										}
									: {
											kind: 'read',
											title: role.name,
											body: role.bodyMarkdown,
											meta: `roles/${role.key}.md`
										}
							)}>{canWrite ? 'Edit' : 'Read'}</Button
					>
				</li>
			{/each}
		</ul>
	</section>

	<section aria-labelledby="dev-skills" class="mt-8">
		<div
			class="flex flex-wrap items-end justify-between gap-3 border-b border-line px-4 pb-3 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<h3 id="dev-skills" class={sectionHeading}>
				Skills <span class="font-normal text-fg-muted">{detail.skills.length}</span>
			</h3>
			{#if canWrite}<Button
					variant="outline"
					size="sm"
					class={mobileButton}
					disabled={paused}
					onclick={(e: MouseEvent) =>
						openPart(e, {
							kind: 'edit',
							part: { kind: 'skill', key: '' },
							title: 'New skill',
							content: skillFileText({
								title: 'New skill',
								description: 'Use when …',
								roles: [],
								bodyMarkdown: NEW_SKILL_BODY
							}),
							isNew: true,
							taken: detail!.skills.map((x) => x.key)
						})}>New skill…</Button
				>{/if}
		</div>
		<p class="px-4 pt-3 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
			An agent in a project that follows it receives the skills for its role and the ones for every
			role, beside the organization's processes and standards.
		</p>
		{#if detail.skills.length === 0}
			<p class="px-4 py-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				No skills yet. A current development process needs at least one.
			</p>
		{:else}
			<ul role="list" class="divide-y divide-line-subtle">
				{#each detail.skills as skill (skill.key)}
					<li class={rowClass} data-testid="dev-skill">
						<div class="min-w-0 flex-1 basis-56">
							<p class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">{skill.title}</p>
							<p class="text-sm/6 text-fg-body">{skill.description}</p>
							<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
								For {skillRolesText(skill.roles, roleNames)}
							</p>
						</div>
						<Button
							variant="outline"
							size="sm"
							class={readButton}
							aria-label="{canWrite ? 'Edit' : 'Read'} the skill {skill.title}"
							disabled={canWrite && paused}
							onclick={(e: MouseEvent) =>
								openPart(
									e,
									canWrite
										? {
												kind: 'edit',
												part: { kind: 'skill', key: skill.key },
												title: `Edit skills/${skill.key}.md`,
												content: skillFileText(skill)
											}
										: {
												kind: 'read',
												title: skill.title,
												body: skill.bodyMarkdown,
												meta: `${skill.description} For ${skillRolesText(skill.roles, roleNames)}.`
											}
								)}>{canWrite ? 'Edit' : 'Read'}</Button
						>
					</li>
				{/each}
			</ul>
		{/if}
	</section>

	<section aria-labelledby="dev-project" class="mt-8 pb-10">
		<h3
			id="dev-project"
			class="border-b border-line px-4 pb-3 {sectionHeading} min-[640px]:px-6 min-[1024px]:px-8"
		>
			What it gives a project
		</h3>
		<dl
			class="grid max-w-3xl grid-cols-1 gap-x-6 gap-y-3 px-4 pt-4 text-sm/6 min-[640px]:grid-cols-[auto_minmax(0,1fr)] min-[640px]:px-6 min-[1024px]:px-8"
		>
			<dt class="text-fg-muted">Tabs</dt>
			<dd class="text-fg-secondary [overflow-wrap:anywhere]">
				{detail.tabs.length
					? detail.tabs.map((t) => `${t.title} (${VIEW_WORD[t.view].toLowerCase()})`).join(', ')
					: 'Only the fixed tabs'}
			</dd>
			<dt class="text-fg-muted">Sections it starts with</dt>
			<dd class="text-fg-secondary">
				{#if detail.folders.length}
					<ul role="list" class="space-y-1">
						{#each detail.folders as folder (folder.path)}
							<li>
								<span class="text-fg">{folder.title}</span>
								<span class="font-mono text-xs/5 text-fg-muted">{folder.path}/</span>
								{#if folder.summary}<span class="block text-fg-muted">{folder.summary}</span>{/if}
							</li>
						{/each}
					</ul>
				{:else}None{/if}
			</dd>
			<dt class="text-fg-muted">Document kinds</dt>
			<dd class="text-fg-secondary">
				{detail.kinds.length
					? detail.kinds
							.map((k) => `${k.title}${k.versioned ? ' (issued in versions)' : ''}`)
							.join(', ')
					: 'Only the built-in kinds'}
			</dd>
			<dt class="text-fg-muted">Templates</dt>
			<dd class="flex flex-wrap items-center gap-2 text-fg-secondary">
				{#each detail.templates as template (template.key)}
					<Button
						variant="outline"
						size="sm"
						class={mobileButton}
						aria-label="{canWrite ? 'Edit' : 'Read'} the {template.key} template"
						disabled={canWrite && paused}
						onclick={(e: MouseEvent) =>
							openPart(
								e,
								canWrite
									? {
											kind: 'edit',
											part: { kind: 'template', key: template.key },
											title: `Edit templates/${template.key}.md`,
											content: template.bodyMarkdown
										}
									: {
											kind: 'read',
											title: `${template.key} template`,
											body: template.bodyMarkdown,
											meta: `templates/${template.key}.md`
										}
							)}><span class="font-mono">{template.key}</span></Button
					>
				{:else}
					<span>None</span>
				{/each}
				{#if canWrite}<Button
						variant="outline"
						size="sm"
						class={mobileButton}
						disabled={paused}
						onclick={(e: MouseEvent) =>
							openPart(e, {
								kind: 'edit',
								part: { kind: 'template', key: '' },
								title: 'New template',
								content: '',
								isNew: true,
								taken: detail!.templates.map((t) => t.key)
							})}>New template…</Button
					>{/if}
			</dd>
		</dl>
	</section>
{/if}

<PartSheet
	bind:this={sheet}
	revision={detail?.revision ?? 0}
	onsave={save}
	onconflict={() => void resource.reload()}
	onCloseAutoFocus={restoreFocus}
/>
<KeyNameDialog
	bind:this={copier}
	title="Copy {detail?.name ?? 'development process'}"
	description="The copy is the organization's own: a draft with a new ID that you can edit. Projects that follow the original keep it."
	confirmLabel="Copy"
	busyLabel="Copying…"
	onsubmit={async (key, name) => {
		const result = await processesApi.copyDevProcess(organizationId, processId, key, name);
		leaving = true;
		await goto(resolve(devProcessHref(result.processId) as '/'));
	}}
	onCloseAutoFocus={restoreFocus}
/>
