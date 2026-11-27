<!--
  @component
  Project Docs tab (`/projects/[id]/docs`, decisions-and-tech-debt-ux §2, DEC-1/3/7/8, LMT-1, organization-data 18):
  Switches between "By kind" (Brief pinned, Decisions, Designs, Reports with evaluation excerpts, process kinds,
  Changelogs, Known limitations per feature, and Older collapsed for deprecated docs) and "By section" (the project's
  documentation tree from index.yaml with add/rename/reorder sections and move document). Includes New document menu,
  status/owner/text filters with honest scope and freshness notice, and problem callouts.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ImageIcon from '@lucide/svelte/icons/image';
	import Loaded from '$lib/components/Loaded.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { gutters, inputClass, mobileButton, selectClass } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatDate } from '$lib/modules/preferences/time';
	import { toAppError } from '$lib/proto';
	import { projectFrame } from '$lib/modules/projects';
	import { docsApi } from '../../api';
	import { groupDocsByKind, matchesDocFilter, newDocHref } from '../../present';
	import type { DocumentLifecycle, ProjectDocsOverview, ProjectDocumentRow } from '../../types';
	import DocsByKindView from './DocsByKindView.svelte';
	import DocsListRow from './DocsListRow.svelte';
	import DocsSectionDialogs from './DocsSectionDialogs.svelte';

	const frame = projectFrame();
	const uid = $props.id();

	const docsRead = new Resource<ProjectDocsOverview>(() =>
		docsApi.list(frame.organizationId, frame.projectId)
	);

	let viewMode = $state<'kind' | 'section'>('kind');
	let statusFilter = $state<'all' | DocumentLifecycle>('all');
	let ownerFilter = $state('');
	let query = $state('');
	let olderOpen = $derived(statusFilter === 'deprecated');
	let sectionBusy = $state(false);
	let sectionDialogs = $state<ReturnType<typeof DocsSectionDialogs>>();

	const overview = $derived(docsRead.data);
	const stale = $derived(!!overview?.stale || !!frame.project.stale);
	const canEdit = $derived(!!overview?.canEdit && !stale && !frame.project.archived);

	const filteredDocs = $derived.by(() => {
		if (!overview) return [];
		return overview.documents.filter((d) =>
			matchesDocFilter(d, { status: statusFilter, ownerId: ownerFilter, query })
		);
	});

	const groupedByKind = $derived.by(() => {
		if (!overview) return { brief: null, groups: [], deprecated: [] };
		return groupDocsByKind(filteredDocs, overview.kinds);
	});

	const customKinds = $derived(overview ? overview.kinds.filter((k) => !k.builtIn) : []);
	const hasFilters = $derived(statusFilter !== 'all' || ownerFilter !== '' || query.trim() !== '');

	function formatWhen(iso: string): string {
		const date = new Date(iso);
		if (Number.isNaN(date.getTime())) return iso;
		return formatDate(date, accountZone.zone, new Date());
	}

	async function openNewDoc(kind: string, template?: 'evaluation' | 'review') {
		await goto(resolve(newDocHref(frame.projectId, kind, { template }) as '/'));
	}

	async function moveSectionOrder(index: number, delta: -1 | 1) {
		if (!overview || sectionBusy) return;
		const nextIndex = index + delta;
		if (nextIndex < 0 || nextIndex >= overview.sections.length) return;
		const ordered = overview.sections.map((s) => s.path);
		const [item] = ordered.splice(index, 1);
		ordered.splice(nextIndex, 0, item);
		sectionBusy = true;
		try {
			await docsApi.reorderSections(frame.organizationId, frame.projectId, ordered);
			toast.success('Updated section order.');
			await docsRead.reload();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			sectionBusy = false;
		}
	}

	function docsInSection(sectionPath: string): ProjectDocumentRow[] {
		return filteredDocs.filter(
			(d) =>
				d.sectionPath === sectionPath &&
				(statusFilter === 'deprecated' || d.status !== 'deprecated')
		);
	}

	const unsectionedDeprecated = $derived(
		statusFilter === 'deprecated'
			? filteredDocs.filter((d) => d.status === 'deprecated' && !d.sectionPath)
			: []
	);
</script>

<Loaded
	resource={docsRead}
	label="the project docs"
	error="Couldn't load the project docs"
	errorLevel={4}
	rows={5}
>
	{#snippet children(data)}
		<div class="flex flex-col">
			<!-- Toolbar: view switch, New menu, filters -->
			<div
				class="flex flex-wrap items-center justify-between gap-3 border-b border-line-subtle py-4 {gutters}"
			>
				<div class="flex flex-wrap items-center gap-3">
					<div
						role="group"
						aria-label="Documentation layout"
						class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
					>
						<button
							type="button"
							aria-pressed={viewMode === 'kind'}
							onclick={() => (viewMode = 'kind')}
							class="rounded-sm px-3 py-1.5 text-xs/5 font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {viewMode ===
							'kind'
								? 'bg-surface-overlay text-fg'
								: 'text-fg-muted hover:text-fg'}"
						>
							By kind
						</button>
						<button
							type="button"
							aria-pressed={viewMode === 'section'}
							onclick={() => (viewMode = 'section')}
							class="rounded-sm px-3 py-1.5 text-xs/5 font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {viewMode ===
							'section'
								? 'bg-surface-overlay text-fg'
								: 'text-fg-muted hover:text-fg'}"
						>
							By section
						</button>
					</div>

					<span class="text-xs/5 text-fg-muted">
						Organization Git · <span class="font-mono">{data.stateBranch}</span>
					</span>
				</div>

				{#if canEdit}
					<div class="flex flex-wrap items-center gap-2">
						{#if viewMode === 'section'}
							<Button
								type="button"
								variant="outline"
								size="sm"
								class={mobileButton}
								onclick={() => sectionDialogs?.openAddSection()}
							>
								<FolderPlusIcon data-icon="inline-start" />
								Add section
							</Button>
						{/if}
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<Button {...props} size="sm" class={mobileButton} data-testid="new-doc-trigger">
										<PlusIcon data-icon="inline-start" />
										New document
										<ChevronDownIcon data-icon="inline-end" />
									</Button>
								{/snippet}
							</DropdownMenu.Trigger>
							<DropdownMenu.Content align="end" class="w-56">
								<DropdownMenu.Group>
									<DropdownMenu.Label>Built-in kinds</DropdownMenu.Label>
									<DropdownMenu.Item onSelect={() => openNewDoc('brief')}>Brief</DropdownMenu.Item>
									<DropdownMenu.Item onSelect={() => openNewDoc('decision')}>
										Decision
									</DropdownMenu.Item>
									<DropdownMenu.Item onSelect={() => openNewDoc('design')}>
										Design
									</DropdownMenu.Item>
									<DropdownMenu.Item onSelect={() => openNewDoc('report')}>
										Report
									</DropdownMenu.Item>
									<DropdownMenu.Item onSelect={() => openNewDoc('report', 'evaluation')}>
										Evaluation (report)
									</DropdownMenu.Item>
								</DropdownMenu.Group>
								{#if customKinds.length > 0}
									<DropdownMenu.Separator />
									<DropdownMenu.Group>
										<DropdownMenu.Label>Process kinds</DropdownMenu.Label>
										{#each customKinds as kind (kind.key)}
											<DropdownMenu.Item onSelect={() => openNewDoc(kind.key)}>
												{kind.title}
											</DropdownMenu.Item>
										{/each}
									</DropdownMenu.Group>
								{/if}
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					</div>
				{/if}
			</div>

			<!-- Filter bar -->
			<div class="flex flex-wrap items-end gap-3 border-b border-line-subtle py-4 {gutters}">
				<div class="min-w-48 flex-1 max-[639px]:basis-full">
					<Label for="{uid}-search" class="mb-1 block text-xs/5 font-medium text-fg-secondary">
						Search docs
					</Label>
					<Input
						id="{uid}-search"
						type="search"
						placeholder="Search titles, keys (ADR-0002), IDs, or body text"
						bind:value={query}
						class={inputClass}
					/>
				</div>

				<div class="w-40 max-[639px]:flex-1">
					<Label for="{uid}-status" class="mb-1 block text-xs/5 font-medium text-fg-secondary">
						Status
					</Label>
					<select id="{uid}-status" bind:value={statusFilter} class={selectClass}>
						<option value="all">All statuses</option>
						<option value="draft">Draft</option>
						<option value="current">Current</option>
						<option value="deprecated">Deprecated</option>
					</select>
				</div>

				<div class="w-44 max-[639px]:flex-1">
					<Label for="{uid}-owner" class="mb-1 block text-xs/5 font-medium text-fg-secondary">
						Owner
					</Label>
					<select id="{uid}-owner" bind:value={ownerFilter} class={selectClass}>
						<option value="">All owners</option>
						{#each data.owners as owner (owner.id)}
							<option value={owner.id}>{owner.name}</option>
						{/each}
					</select>
				</div>

				{#if hasFilters}
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						onclick={() => {
							statusFilter = 'all';
							ownerFilter = '';
							query = '';
						}}
					>
						Clear filters
					</Button>
				{/if}
			</div>

			<!-- Honest coverage & freshness notice (WHY-4) -->
			<p
				class="border-b border-line-subtle py-2 text-xs/5 text-fg-muted {gutters}"
				data-testid="docs-index-coverage"
			>
				{data.indexFreshness.scope === 'title_only'
					? 'Title-only search: body text is not indexed yet; zero matches does not rule out body mentions.'
					: 'Searching permitted project documents (titles, keys, stable IDs and body text).'}
				{#if data.indexFreshness.truncatedBodies}
					Long bodies are truncated in search.
				{/if}
				Indexed
				<time datetime={data.indexFreshness.indexedAt}
					>{formatWhen(data.indexFreshness.indexedAt)}</time
				>. Private task notes and protected chat are excluded.
			</p>

			{#if data.documents.length === 0}
				<EmptyState
					level={4}
					title="No documents yet"
					description="Record decisions, designs, reports and the project brief in organization Git so people and agents can trace why things are built this way."
				>
					{#snippet action()}
						{#if canEdit}
							<Button type="button" class={mobileButton} onclick={() => openNewDoc('decision')}>
								New decision
							</Button>
						{/if}
					{/snippet}
				</EmptyState>
			{:else if filteredDocs.length === 0}
				<div class="py-12 {gutters}" role="status">
					<p class="text-sm/6 font-semibold text-fg">No documents match these filters</p>
					<p class="mt-1 text-sm/6 text-fg-muted">
						Clear the filters or try another title, display key, or term.
					</p>
				</div>
			{:else if viewMode === 'kind'}
				<DocsByKindView
					{uid}
					projectId={frame.projectId}
					projectName={frame.project.name}
					kinds={data.kinds}
					brief={groupedByKind.brief}
					groups={groupedByKind.groups}
					deprecated={groupedByKind.deprecated}
					knownLimitations={data.knownLimitations}
					{hasFilters}
					bind:olderOpen
					{formatWhen}
				/>
			{:else}
				<!-- By section view (project documentation tree index.yaml, organization-data 18) -->
				<div class="flex flex-col divide-y divide-line-subtle" data-testid="docs-by-section">
					{#each data.sections as section, idx (section.path)}
						{@const secDocs = docsInSection(section.path)}
						<section aria-labelledby="{uid}-sec-{idx}" class="py-5">
							<div class="flex flex-wrap items-start justify-between gap-3 {gutters}">
								<div class="min-w-0 flex-1">
									<h4
										id="{uid}-sec-{idx}"
										class="flex flex-wrap items-baseline gap-2 text-sm/6 font-semibold text-fg"
									>
										<span>{section.title}</span>
										<span class="font-mono text-xs/5 font-normal text-fg-muted"
											>docs/{section.path}</span
										>
										<span class="text-xs/5 font-normal text-fg-muted">
											· {secDocs.length + section.media.length}
										</span>
									</h4>
									{#if section.summary}
										<p class="mt-0.5 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
											{section.summary}
										</p>
									{/if}
								</div>
								{#if canEdit}
									<div class="flex flex-wrap items-center gap-1.5">
										<Button
											type="button"
											variant="outline"
											size="sm"
											class={mobileButton}
											aria-label="Move {section.title} up"
											disabled={idx === 0 || sectionBusy}
											onclick={() => moveSectionOrder(idx, -1)}
										>
											<ArrowUpIcon data-icon="inline-start" />
											Up
										</Button>
										<Button
											type="button"
											variant="outline"
											size="sm"
											class={mobileButton}
											aria-label="Move {section.title} down"
											disabled={idx === data.sections.length - 1 || sectionBusy}
											onclick={() => moveSectionOrder(idx, 1)}
										>
											<ArrowDownIcon data-icon="inline-start" />
											Down
										</Button>
										<Button
											type="button"
											variant="outline"
											size="sm"
											class={mobileButton}
											onclick={() => sectionDialogs?.openRenameSection(section)}
										>
											Rename
										</Button>
									</div>
								{/if}
							</div>

							{#if secDocs.length === 0 && section.media.length === 0}
								<p class="mt-3 text-xs/5 text-fg-muted {gutters}">
									No documents in this section yet.
								</p>
							{:else}
								<ul role="list" class="mt-2 divide-y divide-line-subtle">
									{#each secDocs as doc (doc.id)}
										<DocsListRow
											{doc}
											projectId={frame.projectId}
											projectName={frame.project.name}
											kinds={data.kinds}
											showMove
											{canEdit}
											sectionCount={data.sections.length}
											{formatWhen}
											onmove={(d) => sectionDialogs?.openMoveDoc(d)}
										/>
									{/each}
									{#each section.media as media (media.id)}
										<li
											class="flex flex-wrap items-center justify-between gap-2 py-3 text-sm/6 {gutters}"
											data-testid="doc-media-row"
										>
											<div class="flex min-w-0 items-center gap-2.5">
												<ImageIcon class="size-4 shrink-0 text-fg-muted" aria-hidden="true" />
												<div class="min-w-0">
													<p class="font-medium text-fg [overflow-wrap:anywhere]">{media.title}</p>
													<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
														{media.summary}
														<span class="ml-2 font-mono">{media.path}</span>
													</p>
												</div>
											</div>
											<StatusPill
												label={media.type === 'diagram' ? 'Diagram' : 'Image'}
												class={tone.neutral}
											/>
										</li>
									{/each}
								</ul>
							{/if}
						</section>
					{/each}

					{#if unsectionedDeprecated.length > 0}
						<section class="py-5" data-testid="section-unsectioned-deprecated">
							<h4 class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase {gutters}">
								Other deprecated · {unsectionedDeprecated.length}
							</h4>
							<ul role="list" class="mt-2 divide-y divide-line-subtle">
								{#each unsectionedDeprecated as doc (doc.id)}
									<DocsListRow
										{doc}
										projectId={frame.projectId}
										projectName={frame.project.name}
										kinds={data.kinds}
										{formatWhen}
									/>
								{/each}
							</ul>
						</section>
					{/if}
				</div>
			{/if}

			{#if stale}
				<div class="py-4 {gutters}">
					<Callout>
						<p class="text-sm/6">
							HQ can't be reached, so documentation changes are paused. What's shown is the last
							copy.
						</p>
					</Callout>
				</div>
			{/if}
		</div>

		<DocsSectionDialogs
			bind:this={sectionDialogs}
			{uid}
			organizationId={frame.organizationId}
			projectId={frame.projectId}
			sections={data.sections}
			onreload={() => docsRead.reload()}
		/>
	{/snippet}
</Loaded>
