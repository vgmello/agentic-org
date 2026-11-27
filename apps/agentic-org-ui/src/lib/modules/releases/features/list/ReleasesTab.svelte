<!--
  @component
  Project Releases tab (`/projects/[id]/releases`, features-ux §2, organization-data 07 & 09):
  Lists releases (R- references, optional version label, status, counts, latest deployment summary),
  with status filter, search, and New release dialog.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Loaded from '$lib/components/Loaded.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Dialog from '$lib/components/ui/dialog';
	import {
		errorClass,
		gutters,
		inputClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatDate } from '$lib/modules/preferences/time';
	import { toAppError } from '$lib/proto';
	import { projectFrame } from '$lib/platform/workspace/project-frame';
	import { releasesApi } from '../../api';
	import {
		RELEASE_STATUS_TONE,
		RELEASE_STATUS_WORD,
		matchesReleaseFilter,
		releaseHref,
		releaseRef
	} from '../../present';
	import type { ProjectReleasesOverview, ReleaseStatus } from '../../types';

	const frame = projectFrame();
	const uid = $props.id();

	const releasesRead = new Resource<ProjectReleasesOverview>(() =>
		releasesApi.list(frame.organizationId, frame.projectId)
	);

	let statusFilter = $state<'all' | ReleaseStatus>('all');
	let query = $state('');

	let newOpen = $state(false);
	let newTitle = $state('');
	let newVersion = $state('');
	let newBody = $state('');
	let createBusy = $state(false);
	let createError = $state<string | undefined>();

	const overview = $derived(releasesRead.data);
	const stale = $derived(!!overview?.stale || !!frame.project.stale);
	const canEdit = $derived(!!overview?.canEdit && !stale && !frame.project.archived);

	const filtered = $derived.by(() => {
		if (!overview) return [];
		return overview.releases.filter((r) => matchesReleaseFilter(r, statusFilter, query));
	});

	function formatWhen(iso: string): string {
		const date = new Date(iso);
		if (Number.isNaN(date.getTime())) return iso;
		return formatDate(date, accountZone.zone, new Date());
	}

	function openNew() {
		newTitle = '';
		newVersion = '';
		newBody = '## Purpose\n\n## Compatibility notes\n\n## Known limitations\n';
		createError = undefined;
		newOpen = true;
	}

	async function submitNew(event: SubmitEvent) {
		event.preventDefault();
		if (createBusy) return;
		if (!newTitle.trim()) {
			createError = 'Enter a title for this release.';
			return;
		}
		createBusy = true;
		createError = undefined;
		try {
			const created = await releasesApi.create(frame.organizationId, frame.projectId, {
				title: newTitle.trim(),
				versionLabel: newVersion.trim() || null,
				bodyMarkdown: newBody
			});
			newOpen = false;
			toast.success(`Created release ${newTitle.trim()}.`);
			await goto(resolve(releaseHref(frame.projectId, created.id) as '/'));
		} catch (e) {
			createError = toAppError(e).message;
		} finally {
			createBusy = false;
		}
	}
</script>

<Loaded
	resource={releasesRead}
	label="the project releases"
	error="Couldn't load the project releases"
	errorLevel={4}
	rows={4}
>
	{#snippet children(data)}
		<div class="flex flex-col">
			<!-- Toolbar -->
			<div
				class="flex flex-wrap items-end justify-between gap-3 border-b border-line-subtle py-4 {gutters}"
			>
				<div class="flex flex-wrap items-end gap-3">
					<div class="min-w-48 max-[639px]:w-full">
						<Label for="{uid}-search" class="sr-only">Search releases</Label>
						<Input
							id="{uid}-search"
							type="search"
							placeholder="Search R- reference, version, or title"
							bind:value={query}
							class={inputClass}
						/>
					</div>

					<div class="w-40 max-[639px]:flex-1">
						<Label for="{uid}-status" class="sr-only">Filter by status</Label>
						<select id="{uid}-status" bind:value={statusFilter} class={selectClass}>
							<option value="all">All statuses</option>
							<option value="planned">Planned</option>
							<option value="in_progress">In progress</option>
							<option value="released">Released</option>
							<option value="cancelled">Cancelled</option>
						</select>
					</div>
				</div>

				{#if canEdit}
					<Button
						type="button"
						size="sm"
						class={mobileButton}
						onclick={openNew}
						data-testid="new-release-btn"
					>
						<PlusIcon data-icon="inline-start" />
						New release
					</Button>
				{/if}
			</div>

			{#if data.releases.length === 0}
				<EmptyState
					level={4}
					title="No releases yet"
					description="Releases group explicit features, tasks, milestones, product changelogs, intended flag settings, and deployment attempt records."
				>
					{#snippet action()}
						{#if canEdit}
							<Button type="button" class={mobileButton} onclick={openNew}>New release</Button>
						{/if}
					{/snippet}
				</EmptyState>
			{:else if filtered.length === 0}
				<div class="py-12 {gutters}" role="status">
					<p class="text-sm/6 font-semibold text-fg">No releases match these filters</p>
					<p class="mt-1 text-sm/6 text-fg-muted">Try another status or search term.</p>
				</div>
			{:else}
				<ul role="list" class="divide-y divide-line-subtle border-b border-line-subtle">
					{#each filtered as rel (rel.id)}
						<li
							class="relative flex flex-wrap items-start justify-between gap-x-6 gap-y-2 py-4 hover:bg-tint/[0.03] {gutters}"
							data-testid="release-row"
							data-release-id={rel.id}
						>
							<div class="min-w-0 flex-1 basis-64">
								<div class="flex flex-wrap items-baseline gap-x-2 gap-y-1">
									<span class="font-mono text-xs/5 font-semibold text-fg-muted"
										>{releaseRef(rel)}</span
									>
									{#if rel.versionLabel}
										<span class="font-mono text-xs/5 text-fg-secondary">{rel.versionLabel}</span>
									{/if}
									<a
										href={resolve(releaseHref(frame.projectId, rel.id) as '/')}
										class="rounded-sm text-sm/6 font-semibold text-fg hover:text-brand-text focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
									>
										{rel.title}
									</a>
									<StatusPill
										label={RELEASE_STATUS_WORD[rel.status]}
										class={tone[RELEASE_STATUS_TONE[rel.status]]}
									/>
								</div>

								<p class="mt-1 flex flex-wrap gap-x-4 gap-y-0.5 text-xs/5 text-fg-muted">
									<span>Owner: {rel.owner?.name ?? 'Unassigned'}</span>
									<span>
										{rel.milestoneCount}
										{rel.milestoneCount === 1 ? 'milestone' : 'milestones'}
									</span>
									<span>
										{rel.featureCount}
										{rel.featureCount === 1 ? 'feature' : 'features'}
									</span>
									<span>
										{rel.taskCount}
										{rel.taskCount === 1 ? 'task' : 'tasks'}
									</span>
									<span>
										{rel.deploymentCount}
										{rel.deploymentCount === 1 ? 'deployment attempt' : 'deployment attempts'}
									</span>
									<span>
										Updated <time datetime={rel.updatedAt}>{formatWhen(rel.updatedAt)}</time>
									</span>
								</p>
							</div>

							{#if rel.latestDeploymentSummary}
								<p class="max-w-xs text-right text-xs/5 text-fg-secondary max-[639px]:text-left">
									{rel.latestDeploymentSummary}
								</p>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}

			{#if stale}
				<div class="py-4 {gutters}">
					<Callout>
						<p class="text-sm/6">
							HQ can't be reached, so release changes are paused. What's shown is the last copy.
						</p>
					</Callout>
				</div>
			{/if}
		</div>

		<!-- New release dialog -->
		<Dialog.Root bind:open={newOpen}>
			<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
				<Dialog.Header>
					<Dialog.Title text="New release" />
					<Dialog.Description>
						Creates a Planned release manifest in organization Git. Nothing is deployed or enabled
						automatically.
					</Dialog.Description>
				</Dialog.Header>
				<form class="flex flex-col gap-4" onsubmit={submitNew}>
					<div>
						<Label for="{uid}-rel-title" class="mb-1 block text-sm/6 font-medium text-fg">
							Title
						</Label>
						<Input
							id="{uid}-rel-title"
							placeholder="Checkout v2.2 rollout"
							bind:value={newTitle}
							class={inputClass}
						/>
					</div>
					<div>
						<Label for="{uid}-rel-version" class="mb-1 block text-sm/6 font-medium text-fg">
							Version label (optional)
						</Label>
						<Input
							id="{uid}-rel-version"
							placeholder="2.2.0"
							bind:value={newVersion}
							class="font-mono {inputClass}"
						/>
					</div>
					<div>
						<Label for="{uid}-rel-body" class="mb-1 block text-sm/6 font-medium text-fg">
							Release notes / purpose (Markdown)
						</Label>
						<textarea id="{uid}-rel-body" rows={6} bind:value={newBody} class={textareaClass}
						></textarea>
					</div>
					{#if createError}
						<p class={errorClass} role="alert">{createError}</p>
					{/if}
					<Dialog.Footer>
						<Button
							type="button"
							variant="outline"
							class={mobileButton}
							disabled={createBusy}
							onclick={() => (newOpen = false)}
						>
							Cancel
						</Button>
						<Button type="submit" class={mobileButton} disabled={createBusy}>
							{createBusy ? 'Creating…' : 'Create release'}
						</Button>
					</Dialog.Footer>
				</form>
			</Dialog.Content>
		</Dialog.Root>
	{/snippet}
</Loaded>
