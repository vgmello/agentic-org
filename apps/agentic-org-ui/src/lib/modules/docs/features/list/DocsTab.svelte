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
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import FolderPlusIcon from '@lucide/svelte/icons/folder-plus';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ImageIcon from '@lucide/svelte/icons/image';
	import AlertTriangleIcon from '@lucide/svelte/icons/alert-triangle';
	import Loaded from '$lib/components/Loaded.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Dialog from '$lib/components/ui/dialog';
	import {
		errorClass,
		gutters,
		inputClass,
		mobileButton,
		selectClass
	} from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatDate } from '$lib/modules/preferences/time';
	import { toAppError } from '$lib/proto';
	import { projectFrame } from '$lib/modules/projects';
	import { docsApi } from '../../api';
	import {
		checkSectionPath,
		docHref,
		groupDocsByKind,
		isTreeDocument,
		kindTitle,
		matchesDocFilter,
		newDocHref,
		scopeLabel
	} from '../../present';
	import type {
		DocSection,
		DocumentLifecycle,
		ProjectDocsOverview,
		ProjectDocumentRow
	} from '../../types';
	import DocLifecycleBadge from '../../components/DocLifecycleBadge.svelte';

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

	// Section management dialogs (By section view)
	let addSectionOpen = $state(false);
	let newSectionPath = $state('');
	let newSectionTitle = $state('');
	let newSectionSummary = $state('');
	let sectionBusy = $state(false);
	let sectionError = $state<string | undefined>();

	let renameTarget = $state<DocSection | null>(null);
	let renameTitle = $state('');
	let renameSummary = $state('');

	let moveTarget = $state<ProjectDocumentRow | null>(null);
	let moveSectionPath = $state('');
	let moveBusy = $state(false);
	let moveError = $state<string | undefined>();

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

	function openAddSection() {
		newSectionPath = '';
		newSectionTitle = '';
		newSectionSummary = '';
		sectionError = undefined;
		addSectionOpen = true;
	}

	async function submitAddSection(event: SubmitEvent) {
		event.preventDefault();
		if (sectionBusy) return;
		const pathErr = checkSectionPath(newSectionPath);
		if (pathErr) {
			sectionError = pathErr;
			return;
		}
		if (!newSectionTitle.trim()) {
			sectionError = 'Enter a title for the section.';
			return;
		}
		sectionBusy = true;
		sectionError = undefined;
		try {
			await docsApi.addSection(frame.organizationId, frame.projectId, {
				path: newSectionPath.trim().replace(/^\/+|\/+$/g, ''),
				title: newSectionTitle.trim(),
				summary: newSectionSummary.trim()
			});
			addSectionOpen = false;
			toast.success(`Added section ${newSectionTitle.trim()}.`);
			await docsRead.reload();
		} catch (e) {
			sectionError = toAppError(e).message;
		} finally {
			sectionBusy = false;
		}
	}

	function openRenameSection(section: DocSection) {
		renameTarget = section;
		renameTitle = section.title;
		renameSummary = section.summary;
		sectionError = undefined;
	}

	async function submitRenameSection(event: SubmitEvent) {
		event.preventDefault();
		if (!renameTarget || sectionBusy) return;
		if (!renameTitle.trim()) {
			sectionError = 'Enter a title for the section.';
			return;
		}
		sectionBusy = true;
		sectionError = undefined;
		try {
			await docsApi.renameSection(frame.organizationId, frame.projectId, renameTarget.path, {
				title: renameTitle.trim(),
				summary: renameSummary.trim()
			});
			const saved = renameTitle.trim();
			renameTarget = null;
			toast.success(`Updated section ${saved}.`);
			await docsRead.reload();
		} catch (e) {
			sectionError = toAppError(e).message;
		} finally {
			sectionBusy = false;
		}
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

	function openMoveDoc(doc: ProjectDocumentRow) {
		moveTarget = doc;
		moveSectionPath = doc.sectionPath ?? overview?.sections[0]?.path ?? '';
		moveError = undefined;
	}

	async function submitMoveDoc(event: SubmitEvent) {
		event.preventDefault();
		if (!moveTarget || moveBusy || !moveSectionPath) return;
		moveBusy = true;
		moveError = undefined;
		try {
			await docsApi.moveDocument(frame.organizationId, frame.projectId, moveTarget.id, {
				sectionPath: moveSectionPath,
				expectedRevision: moveTarget.revision
			});
			const title = moveTarget.title;
			moveTarget = null;
			toast.success(`Moved ${title}.`);
			await docsRead.reload();
		} catch (e) {
			moveError = toAppError(e).message;
		} finally {
			moveBusy = false;
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

{#snippet docRow(doc: ProjectDocumentRow, showMove: boolean)}
	<li
		class="relative flex flex-wrap items-start justify-between gap-x-4 gap-y-2 py-4 hover:bg-tint/[0.03] {gutters}"
		data-testid="doc-row"
		data-doc-id={doc.id}
	>
		<div class="min-w-0 flex-1 basis-60">
			<div class="flex flex-wrap items-baseline gap-x-2 gap-y-1">
				{#if doc.key}
					<span class="font-mono text-xs/5 font-medium text-fg-muted">{doc.key}</span>
				{/if}
				<a
					href={resolve(docHref(frame.projectId, doc.id) as '/')}
					class="rounded-sm text-sm/6 font-semibold text-fg hover:text-brand-text focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus [overflow-wrap:anywhere]"
				>
					{doc.title}
				</a>
				<DocLifecycleBadge status={doc.status} />
				{#if doc.version !== null}
					<StatusPill label="v{doc.version}" class={tone.neutral} />
				{/if}
				{#if doc.hasUnissuedChanges}
					<StatusPill label="Unissued changes" class={tone.owner} />
				{/if}
				{#if doc.pendingPublication && doc.pendingPublication > 0}
					<StatusPill
						label="Waiting to publish · {doc.pendingPublication} {doc.pendingPublication === 1
							? 'change'
							: 'changes'}"
						class={tone.owner}
					/>
				{/if}
			</div>
			{#if doc.summary}
				<p class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">{doc.summary}</p>
			{/if}
			{#if doc.evaluationExcerpt}
				<p class="mt-1 text-xs/5 text-fg-body [overflow-wrap:anywhere]" data-testid="eval-excerpt">
					<span class="font-semibold text-fg-muted">Excerpt:</span>
					{doc.evaluationExcerpt.question} → {doc.evaluationExcerpt.verdict}
				</p>
			{/if}
			<p class="mt-1 flex flex-wrap gap-x-3 gap-y-0.5 text-xs/5 text-fg-muted">
				<span>{kindTitle(doc.kind, overview?.kinds)}</span>
				<span>{scopeLabel(doc.scope, frame.project.name)}</span>
				<span>Owner: {doc.owner?.name ?? 'Unassigned'}</span>
				<span>
					Updated <time datetime={doc.updatedAt}>{formatWhen(doc.updatedAt)}</time>
				</span>
			</p>
			{#if doc.problem}
				<div
					class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md bg-warning/10 px-3 py-1.5 text-xs/5 text-warning-text-strong ring-1 ring-warning/30"
					data-testid="doc-problem"
				>
					<AlertTriangleIcon class="size-3.5 shrink-0" aria-hidden="true" />
					{#if doc.problem.kind === 'invalid_metadata'}
						<span>Details not recognized: {doc.problem.field} ({doc.problem.message})</span>
						<a
							href={resolve(`${docHref(frame.projectId, doc.id)}?edit=details` as '/')}
							class="font-semibold text-link hover:underline"
						>
							Fix
						</a>
					{:else}
						<span>
							Duplicate display key {doc.problem.key} also used by {doc.problem.others
								.map((o) => `${o.title} (${o.scopeLabel})`)
								.join(', ')}.
						</span>
						<a
							href={resolve(`${docHref(frame.projectId, doc.id)}?edit=details` as '/')}
							class="font-semibold text-link hover:underline"
						>
							Resolve key…
						</a>
					{/if}
				</div>
			{/if}
		</div>
		{#if showMove && canEdit && isTreeDocument(doc.kind, doc.scope) && (overview?.sections.length ?? 0) > 1}
			<div class="shrink-0">
				<Button
					type="button"
					variant="outline"
					size="sm"
					class={mobileButton}
					onclick={() => openMoveDoc(doc)}
				>
					Move…
				</Button>
			</div>
		{/if}
	</li>
{/snippet}

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
								onclick={openAddSection}
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
				<!-- By kind view (DEC-1, DEC-8) -->
				{#if groupedByKind.brief}
					<section aria-labelledby="{uid}-brief-group" class="border-b border-line-subtle pt-5">
						<div class="flex items-baseline justify-between gap-2 pb-2 {gutters}">
							<h4
								id="{uid}-brief-group"
								class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase"
							>
								Brief (pinned)
							</h4>
						</div>
						<ul role="list" class="divide-y divide-line-subtle">
							{@render docRow(groupedByKind.brief, false)}
						</ul>
					</section>
				{/if}

				{#each groupedByKind.groups as group (group.kind)}
					<section
						aria-labelledby="{uid}-group-{group.kind}"
						class="border-b border-line-subtle pt-5"
						data-testid="doc-kind-group"
						data-kind={group.kind}
					>
						<div class="flex items-baseline justify-between gap-2 pb-2 {gutters}">
							<h4
								id="{uid}-group-{group.kind}"
								class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase"
							>
								{group.label} · {group.docs.length}
							</h4>
						</div>
						<ul role="list" class="divide-y divide-line-subtle">
							{#each group.docs as doc (doc.id)}
								{@render docRow(doc, false)}
							{/each}
						</ul>
					</section>
				{/each}

				<!-- Known limitations per feature (LMT-1) -->
				{#if data.knownLimitations.length > 0 && !hasFilters}
					<section
						aria-labelledby="{uid}-limitations"
						class="border-b border-line-subtle py-5 {gutters}"
						data-testid="known-limitations-section"
					>
						<h4
							id="{uid}-limitations"
							class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase"
						>
							Known limitations · {data.knownLimitations.length}
						</h4>
						<p class="mt-1 text-xs/5 text-fg-muted">
							Kept in each feature's canonical definition file; no separate limitation record is
							created.
						</p>
						<ul
							role="list"
							class="mt-3 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
						>
							{#each data.knownLimitations as lim (`${lim.featureId}-${lim.text}`)}
								<li class="px-4 py-3 text-sm/6">
									<div class="flex flex-wrap items-baseline justify-between gap-2">
										<a
											href={resolve(
												`/projects/${encodeURIComponent(frame.projectId)}/features/${encodeURIComponent(lim.featureId)}` as '/'
											)}
											class="font-semibold text-link hover:underline"
										>
											<span class="font-mono text-xs/5">{lim.featureRef}</span>
											{lim.featureTitle}
										</a>
										<span class="text-xs/5 text-fg-muted">{lim.featureStatus}</span>
									</div>
									<p class="mt-1 text-fg-body [overflow-wrap:anywhere]">{lim.text}</p>
								</li>
							{/each}
						</ul>
					</section>
				{/if}

				<!-- Deprecated documents collapsed under Older (DEC-8) -->
				{#if groupedByKind.deprecated.length > 0}
					<section class="border-b border-line-subtle py-4" data-testid="older-deprecated">
						<div class={gutters}>
							<button
								type="button"
								aria-expanded={olderOpen}
								aria-controls="{uid}-older-list"
								onclick={() => (olderOpen = !olderOpen)}
								class="inline-flex min-h-9 items-center gap-2 rounded-sm text-xs/5 font-semibold tracking-wide text-fg-muted uppercase hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
							>
								{#if olderOpen}
									<ChevronDownIcon class="size-4" aria-hidden="true" />
								{:else}
									<ChevronRightIcon class="size-4" aria-hidden="true" />
								{/if}
								Older · {groupedByKind.deprecated.length} (deprecated)
							</button>
						</div>
						{#if olderOpen}
							<ul
								id="{uid}-older-list"
								role="list"
								class="mt-2 divide-y divide-line-subtle border-t border-line-subtle"
							>
								{#each groupedByKind.deprecated as doc (doc.id)}
									{@render docRow(doc, false)}
								{/each}
							</ul>
						{/if}
					</section>
				{/if}
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
											onclick={() => openRenameSection(section)}
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
										{@render docRow(doc, true)}
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
									{@render docRow(doc, false)}
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

		<!-- Add section dialog -->
		<Dialog.Root bind:open={addSectionOpen}>
			<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
				<Dialog.Header>
					<Dialog.Title text="Add documentation section" />
					<Dialog.Description>
						Adds a folder section to the project's documentation index.
					</Dialog.Description>
				</Dialog.Header>
				<form class="flex flex-col gap-4" onsubmit={submitAddSection}>
					<div>
						<Label for="{uid}-sec-path" class="mb-1 block text-sm/6 font-medium text-fg">
							Folder path
						</Label>
						<Input
							id="{uid}-sec-path"
							placeholder="architecture"
							bind:value={newSectionPath}
							class={inputClass}
						/>
						<p class="mt-1 text-xs/5 text-fg-muted">
							Relative to <span class="font-mono">docs/</span>. Lowercase letters, digits, hyphens
							and slashes.
						</p>
					</div>
					<div>
						<Label for="{uid}-sec-title" class="mb-1 block text-sm/6 font-medium text-fg">
							Title
						</Label>
						<Input
							id="{uid}-sec-title"
							placeholder="Architecture"
							bind:value={newSectionTitle}
							class={inputClass}
						/>
					</div>
					<div>
						<Label for="{uid}-sec-summary" class="mb-1 block text-sm/6 font-medium text-fg">
							Summary
						</Label>
						<Input
							id="{uid}-sec-summary"
							placeholder="One-line reason to open this section."
							bind:value={newSectionSummary}
							class={inputClass}
						/>
					</div>
					{#if sectionError}
						<p class={errorClass} role="alert">{sectionError}</p>
					{/if}
					<Dialog.Footer>
						<Button
							type="button"
							variant="outline"
							class={mobileButton}
							disabled={sectionBusy}
							onclick={() => (addSectionOpen = false)}
						>
							Cancel
						</Button>
						<Button type="submit" class={mobileButton} disabled={sectionBusy}>
							{sectionBusy ? 'Adding section…' : 'Add section'}
						</Button>
					</Dialog.Footer>
				</form>
			</Dialog.Content>
		</Dialog.Root>

		<!-- Rename section dialog -->
		<Dialog.Root
			open={renameTarget !== null}
			onOpenChange={(open) => {
				if (!open) renameTarget = null;
			}}
		>
			<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
				<Dialog.Header>
					<Dialog.Title text="Rename section" />
					<Dialog.Description>
						Updates the section title and summary in the documentation index without moving files.
					</Dialog.Description>
				</Dialog.Header>
				<form class="flex flex-col gap-4" onsubmit={submitRenameSection}>
					<div>
						<Label for="{uid}-rename-title" class="mb-1 block text-sm/6 font-medium text-fg">
							Title
						</Label>
						<Input id="{uid}-rename-title" bind:value={renameTitle} class={inputClass} />
					</div>
					<div>
						<Label for="{uid}-rename-summary" class="mb-1 block text-sm/6 font-medium text-fg">
							Summary
						</Label>
						<Input id="{uid}-rename-summary" bind:value={renameSummary} class={inputClass} />
					</div>
					{#if sectionError}
						<p class={errorClass} role="alert">{sectionError}</p>
					{/if}
					<Dialog.Footer>
						<Button
							type="button"
							variant="outline"
							class={mobileButton}
							disabled={sectionBusy}
							onclick={() => (renameTarget = null)}
						>
							Cancel
						</Button>
						<Button type="submit" class={mobileButton} disabled={sectionBusy}>
							{sectionBusy ? 'Saving…' : 'Save section'}
						</Button>
					</Dialog.Footer>
				</form>
			</Dialog.Content>
		</Dialog.Root>

		<!-- Move document dialog -->
		<Dialog.Root
			open={moveTarget !== null}
			onOpenChange={(open) => {
				if (!open) moveTarget = null;
			}}
		>
			<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
				<Dialog.Header>
					<Dialog.Title text="Move document" />
					<Dialog.Description>
						Moving a document updates its path in the documentation tree while preserving its stable
						ID, history, issued versions, and links.
					</Dialog.Description>
				</Dialog.Header>
				<form class="flex flex-col gap-4" onsubmit={submitMoveDoc}>
					<div>
						<Label for="{uid}-move-sec" class="mb-1 block text-sm/6 font-medium text-fg">
							Destination section
						</Label>
						<select id="{uid}-move-sec" bind:value={moveSectionPath} class={selectClass}>
							{#each data.sections as sec (sec.path)}
								<option value={sec.path}>{sec.title} (docs/{sec.path})</option>
							{/each}
						</select>
					</div>
					{#if moveError}
						<p class={errorClass} role="alert">{moveError}</p>
					{/if}
					<Dialog.Footer>
						<Button
							type="button"
							variant="outline"
							class={mobileButton}
							disabled={moveBusy}
							onclick={() => (moveTarget = null)}
						>
							Cancel
						</Button>
						<Button type="submit" class={mobileButton} disabled={moveBusy}>
							{moveBusy ? 'Moving…' : 'Move document'}
						</Button>
					</Dialog.Footer>
				</form>
			</Dialog.Content>
		</Dialog.Root>
	{/snippet}
</Loaded>
