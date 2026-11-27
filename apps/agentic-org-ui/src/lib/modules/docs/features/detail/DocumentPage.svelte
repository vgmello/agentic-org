<!--
  @component
  Project document page for `/projects/[id]/docs/[docId]` and `/projects/[id]/docs/new`
  (decisions-and-tech-debt-ux §2–§5, DEC-2/3/6/7, TPL-1/2/8, WHY-2, organization-data 18):
  - Reader view with front-matter header card (never raw YAML), section rail with expected-section hints,
    safe Markdown body, code links, and Led to / Mentioned in footer.
  - Synchronized Details & body / Source editor (TPL-2), Current decision edit warning + Supersede flow (DEC-6),
    revision conflict diff & preservation, and failed-save copyable draft with Retry.
  - History & Compare panel with issued versions, Git revisions, and accessible line diff.
  - Issue version dialog for versioned kinds (records note and derived_from sources).
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import AlertTriangleIcon from '@lucide/svelte/icons/alert-triangle';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import TagIcon from '@lucide/svelte/icons/tag';
	import Loaded from '$lib/components/Loaded.svelte';
	import Avatar from '$lib/components/Avatar.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatDate } from '$lib/modules/preferences/time';
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/proto';
	import { projectFrame } from '$lib/modules/projects';
	import { docsApi } from '../../api';
	import {
		checkDocumentDraft,
		conflictLatestDoc,
		deriveCanonicalPath,
		docHref,
		docsHref,
		draftFromDetail,
		kindTitle,
		newDocHref,
		parseDocumentSource,
		scopeLabel,
		serializeDocumentSource
	} from '../../present';
	import type {
		DocField,
		DocumentDraft,
		ProjectDocsOverview,
		ProjectDocumentDetail
	} from '../../types';
	import DocLifecycleBadge from '../../components/DocLifecycleBadge.svelte';
	import { buildNewDocumentDraft, clearEditQueryParamFromUrl } from './document-draft-helpers';
	import DocumentDialogs from './DocumentDialogs.svelte';
	import DocumentEditorForm from './DocumentEditorForm.svelte';
	import DocumentHistoryPanel from './DocumentHistoryPanel.svelte';
	import DocumentReaderBody from './DocumentReaderBody.svelte';

	let { docId = null }: { docId?: string | null } = $props();

	const frame = projectFrame();
	const uid = $props.id();
	const isNew = $derived(docId === null);

	type DocLookup =
		| { found: true; doc: ProjectDocumentDetail; overview: ProjectDocsOverview }
		| { found: false; message: string; overview: ProjectDocsOverview };

	let activeView = $state<'read' | 'edit' | 'history'>('read');
	let editMode = $state<'details' | 'source'>('details');
	let decisionWarnOpen = $state(false);
	let dialogs = $state<ReturnType<typeof DocumentDialogs>>();

	let draft = $state<DocumentDraft | null>(null);
	let sourceText = $state('');
	let sourceError = $state<string | undefined>();
	let fieldErrors = $state<Partial<Record<DocField, string>>>({});
	let changeSummary = $state('');
	let saveBusy = $state(false);
	let saveError = $state<string | undefined>();
	let conflictDoc = $state<ProjectDocumentDetail | null>(null);
	let fileNameTouched = $state(false);
	let consumedInitialEditFor: string | null = null;

	const validationErrorList = $derived(
		Object.values(fieldErrors).filter((msg): msg is string => Boolean(msg))
	);

	function serializeDraft(d: DocumentDraft, doc: ProjectDocumentDetail | undefined): string {
		const metaId = doc?.id ?? 'pending-uuidv7';
		const metaRev = doc?.revision ?? 1;
		const path =
			doc?.path ??
			deriveCanonicalPath(frame.projectId, metaId, d.kind, d.scope, d.sectionPath, d.fileName);
		return serializeDocumentSource(d, {
			id: metaId,
			revision: metaRev,
			projectId: frame.projectId,
			path
		});
	}

	async function loadNewOverview(): Promise<ProjectDocsOverview> {
		const overview = await docsApi.list(frame.organizationId, frame.projectId);
		const next = buildNewDocumentDraft(page.url.searchParams, overview);
		draft = next;
		sourceText = serializeDraft(next, undefined);
		sourceError = undefined;
		fieldErrors = {};
		saveError = undefined;
		conflictDoc = null;
		fileNameTouched = false;
		editMode = 'details';
		return overview;
	}

	async function loadExisting(): Promise<DocLookup> {
		const overview = await docsApi.list(frame.organizationId, frame.projectId);
		try {
			const doc = await docsApi.get(frame.organizationId, frame.projectId, docId!);
			const editable =
				doc.canEdit &&
				!doc.stale &&
				!overview.stale &&
				!frame.project.stale &&
				!frame.project.archived;
			if (
				consumedInitialEditFor !== doc.id &&
				editable &&
				page.url.searchParams.get('edit') === 'details'
			) {
				consumedInitialEditFor = doc.id;
				beginEdit(doc);
			}
			return { found: true, doc, overview };
		} catch (e) {
			const err = toAppError(e);
			if (err.source === 'api' && err.code === 'not_found') {
				return { found: false, message: err.message, overview };
			}
			throw e;
		}
	}

	const existingRead = $derived(
		docId !== null ? new Resource<DocLookup>(() => loadExisting()) : null
	);
	const newSearchKey = $derived(isNew ? page.url.search : null);
	const overviewRead = $derived.by(() => {
		if (docId !== null) return null;
		void newSearchKey;
		return new Resource<ProjectDocsOverview>(() => loadNewOverview());
	});

	const currentOverview = $derived(isNew ? overviewRead?.data : existingRead?.data?.overview);
	const currentDoc = $derived(
		!isNew && existingRead?.data?.found ? existingRead.data.doc : undefined
	);
	const stale = $derived(!!currentDoc?.stale || !!currentOverview?.stale || !!frame.project.stale);
	const canEdit = $derived(
		(isNew ? !!currentOverview?.canEdit : !!currentDoc?.canEdit) &&
			!stale &&
			!frame.project.archived
	);

	const kindMeta = $derived.by(() => {
		const k = draft?.kind ?? currentDoc?.kind ?? 'decision';
		return currentOverview?.kinds.find((x) => x.key === k);
	});

	const title = $derived.by(() => {
		if (isNew) {
			const k = page.url.searchParams.get('kind') ?? 'document';
			return pageTitle(`New ${kindTitle(k, currentOverview?.kinds)}`, 'Docs', frame.project.name);
		}
		if (!currentDoc) return pageTitle('Document', 'Docs', frame.project.name);
		return pageTitle(currentDoc.title, 'Docs', frame.project.name);
	});

	const derivedPathPreview = $derived.by(() => {
		if (!draft) return '';
		return deriveCanonicalPath(
			frame.projectId,
			currentDoc?.id ?? 'new-uuidv7',
			draft.kind,
			draft.scope,
			draft.sectionPath,
			draft.fileName
		);
	});

	function formatWhen(iso: string): string {
		const date = new Date(iso);
		if (Number.isNaN(date.getTime())) return iso;
		return formatDate(date, accountZone.zone, new Date());
	}

	function beginEdit(doc: ProjectDocumentDetail) {
		const next = draftFromDetail(doc);
		draft = next;
		sourceText = serializeDraft(next, doc);
		sourceError = undefined;
		fieldErrors = {};
		saveError = undefined;
		conflictDoc = null;
		changeSummary = '';
		editMode = 'details';
		activeView = 'edit';
	}

	function requestEdit(doc: ProjectDocumentDetail) {
		if (doc.kind === 'decision' && doc.status === 'current') {
			decisionWarnOpen = true;
			return;
		}
		beginEdit(doc);
	}

	function switchEditMode(nextMode: 'details' | 'source') {
		if (!draft || nextMode === editMode) return;
		if (nextMode === 'source') {
			sourceText = serializeDraft(draft, currentDoc);
			sourceError = undefined;
			editMode = 'source';
		} else {
			const parsed = parseDocumentSource(sourceText, draft);
			if (!parsed.ok) {
				sourceError = parsed.error;
				return;
			}
			draft = parsed.draft;
			sourceError = undefined;
			editMode = 'details';
		}
	}

	function syncDraftFromSourceIfNeeded(): DocumentDraft | null {
		if (!draft) return null;
		if (editMode === 'source') {
			const parsed = parseDocumentSource(sourceText, draft);
			if (!parsed.ok) {
				sourceError = parsed.error;
				return null;
			}
			draft = parsed.draft;
			sourceError = undefined;
			return parsed.draft;
		}
		return draft;
	}

	async function submitSave(expectedRevision?: number) {
		if (saveBusy) return;
		const ready = syncDraftFromSourceIfNeeded();
		if (!ready) return;
		const errs = checkDocumentDraft(ready);
		fieldErrors = errs;
		if (Object.keys(errs).length > 0) return;

		saveBusy = true;
		saveError = undefined;
		const plainDraft = $state.snapshot(ready);
		try {
			if (isNew) {
				const created = await docsApi.create(frame.organizationId, frame.projectId, plainDraft);
				toast.success(`Created ${plainDraft.title.trim()}.`);
				await goto(resolve(docHref(frame.projectId, created.id) as '/'));
			} else if (currentDoc) {
				await docsApi.save(frame.organizationId, frame.projectId, currentDoc.id, {
					draft: plainDraft,
					expectedRevision: expectedRevision ?? currentDoc.revision,
					summary: changeSummary.trim() || undefined
				});
				conflictDoc = null;
				activeView = 'read';
				clearEditQueryParamFromUrl(page.url, page.state);
				toast.success(`Saved ${plainDraft.title.trim()}.`);
				await existingRead?.reload();
			}
		} catch (e) {
			const latest = conflictLatestDoc(e);
			if (latest) {
				conflictDoc = latest;
				saveError = `${latest.title} changed while you were editing (rev ${latest.revision}). Compare below before saving again.`;
			} else {
				saveError = toAppError(e).message;
			}
		} finally {
			saveBusy = false;
		}
	}

	function cancelEdit(existing: ProjectDocumentDetail | undefined) {
		if (existing) {
			activeView = 'read';
			conflictDoc = null;
			saveError = undefined;
			fieldErrors = {};
			sourceError = undefined;
			clearEditQueryParamFromUrl(page.url, page.state);
		} else {
			void goto(resolve(docsHref(frame.projectId) as '/'));
		}
	}
</script>

<svelte:head><title>{title}</title></svelte:head>

<div class="border-b border-line-subtle py-3 {gutters}">
	<a
		href={resolve(docsHref(frame.projectId) as '/')}
		class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
	>
		Docs
	</a>
</div>

{#if isNew && overviewRead}
	<Loaded
		resource={overviewRead}
		label="the document template"
		error="Couldn't load the document template"
		errorLevel={4}
		rows={4}
	>
		{#snippet children(overview)}
			{#if !overview.canEdit || stale || frame.project.archived}
				<div class="py-6 {gutters}">
					<Callout>
						<p class="text-sm/6">
							You can't create documents in {frame.project.name} right now.
						</p>
					</Callout>
				</div>
			{:else}
				<DocumentEditorForm
					{uid}
					{isNew}
					{overview}
					existing={undefined}
					{derivedPathPreview}
					{editMode}
					{conflictDoc}
					{saveError}
					{saveBusy}
					{sourceError}
					{fieldErrors}
					{validationErrorList}
					bind:draft
					bind:sourceText
					bind:changeSummary
					bind:fileNameTouched
					onSwitchMode={switchEditMode}
					onSave={(rev) => void submitSave(rev)}
					onLoadLatest={beginEdit}
					onCancel={() => cancelEdit(undefined)}
				/>
			{/if}
		{/snippet}
	</Loaded>
{:else if existingRead}
	<Loaded
		resource={existingRead}
		label="the document"
		error="Couldn't load this document"
		errorLevel={4}
		rows={5}
	>
		{#snippet children(lookup)}
			{#if !lookup.found}
				<div class="py-8 {gutters}">
					<h3 class="text-base/7 font-semibold text-fg">Document not found</h3>
					<p class="mt-1 max-w-prose text-sm/6 text-fg-muted">{lookup.message}</p>
				</div>
			{:else}
				{@const doc = lookup.doc}
				{@const overview = lookup.overview}

				{#if activeView === 'edit'}
					<DocumentEditorForm
						{uid}
						{isNew}
						{overview}
						existing={doc}
						{derivedPathPreview}
						{editMode}
						{conflictDoc}
						{saveError}
						{saveBusy}
						{sourceError}
						{fieldErrors}
						{validationErrorList}
						bind:draft
						bind:sourceText
						bind:changeSummary
						bind:fileNameTouched
						onSwitchMode={switchEditMode}
						onSave={(rev) => void submitSave(rev)}
						onLoadLatest={beginEdit}
						onCancel={() => cancelEdit(doc)}
					/>
				{:else}
					<!-- Reader & History header card (DEC-2, never raw YAML) -->
					<div class="border-b border-line-subtle py-6 {gutters}" data-testid="doc-header-card">
						<div class="flex flex-wrap items-start justify-between gap-4">
							<div class="min-w-0 flex-1 basis-72">
								<div class="flex flex-wrap items-center gap-2 text-xs/5 text-fg-muted">
									<span class="font-semibold text-fg-secondary">
										{kindTitle(doc.kind, overview.kinds)}
									</span>
									{#if doc.key}
										<span class="font-mono text-fg-secondary">{doc.key}</span>
									{/if}
									<DocLifecycleBadge status={doc.status} />
									{#if doc.version !== null}
										<StatusPill label="v{doc.version}" class={tone.neutral} />
									{/if}
									{#if doc.hasUnissuedChanges}
										<StatusPill label="Unissued changes" class={tone.owner} />
									{/if}
									{#if doc.pendingPublication && doc.pendingPublication > 0}
										<StatusPill
											label="Waiting to publish · {doc.pendingPublication} {doc.pendingPublication ===
											1
												? 'change'
												: 'changes'}"
											class={tone.owner}
										/>
									{/if}
								</div>

								<h3 class="mt-2 text-lg/7 font-semibold text-fg [overflow-wrap:anywhere]">
									{doc.title}
								</h3>

								{#if doc.summary}
									<p class="mt-1 max-w-prose text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
										{doc.summary}
									</p>
								{/if}

								<div
									class="mt-3 flex flex-wrap items-center gap-x-5 gap-y-2 text-xs/5 text-fg-muted"
								>
									<span class="inline-flex items-center gap-1.5">
										{#if doc.owner}
											<Avatar name={doc.owner.name} size="sm" />
											<span>Owner: <span class="text-fg-secondary">{doc.owner.name}</span></span>
										{:else}
											<span>Owner: Unassigned</span>
										{/if}
									</span>
									<span>Scope: {scopeLabel(doc.scope, frame.project.name)}</span>
									<span>
										Created <time datetime={doc.createdAt}>{formatWhen(doc.createdAt)}</time> by {doc
											.createdBy.name}
									</span>
									<span>
										Updated <time datetime={doc.updatedAt}>{formatWhen(doc.updatedAt)}</time> by {doc
											.updatedBy.name} · rev {doc.revision}
									</span>
								</div>

								<div
									class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 font-mono text-xs/5 text-fg-muted"
								>
									<span class="[overflow-wrap:anywhere]">{doc.path}</span>
									<span>ID: {doc.id}</span>
								</div>

								{#if doc.links.length > 0}
									<div
										class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs/5 text-fg-muted"
										data-testid="doc-related-links"
									>
										<span class="font-medium text-fg-secondary">Related:</span>
										{#each doc.links as item (item.id)}
											<a
												href={resolve(item.href as '/')}
												class="text-link hover:underline [overflow-wrap:anywhere]"
											>
												{#if item.ref || item.key}
													<span class="mr-1 font-mono">{item.ref ?? item.key}</span>
												{/if}
												{item.title}
											</a>
										{/each}
									</div>
								{/if}
							</div>

							<!-- Action buttons -->
							<div class="flex flex-wrap items-center gap-2">
								<Button
									type="button"
									variant={activeView === 'history' ? 'secondary' : 'outline'}
									size="sm"
									class={mobileButton}
									onclick={() => (activeView = activeView === 'history' ? 'read' : 'history')}
								>
									<HistoryIcon data-icon="inline-start" />
									{activeView === 'history' ? 'Back to reader' : 'History & compare'}
								</Button>

								{#if canEdit && doc.canIssue && kindMeta?.versioned}
									<Button
										type="button"
										variant="outline"
										size="sm"
										class={mobileButton}
										onclick={() => dialogs?.openIssueDialog()}
										data-testid="issue-version-btn"
									>
										<TagIcon data-icon="inline-start" />
										Issue version
									</Button>
								{/if}

								{#if canEdit && doc.kind === 'decision' && doc.status === 'current'}
									<Button
										type="button"
										variant="outline"
										size="sm"
										class={mobileButton}
										onclick={() =>
											void goto(
												resolve(
													newDocHref(frame.projectId, 'decision', { supersedes: doc.id }) as '/'
												)
											)}
									>
										<GitBranchIcon data-icon="inline-start" />
										Supersede
									</Button>
								{/if}

								{#if canEdit}
									<Button
										type="button"
										size="sm"
										class={mobileButton}
										onclick={() => requestEdit(doc)}
										data-testid="edit-doc-btn"
									>
										<PencilIcon data-icon="inline-start" />
										Edit
									</Button>
								{/if}
							</div>
						</div>

						{#if doc.problem}
							<div
								class="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-lg bg-warning/10 px-4 py-3 text-sm/6 text-warning-text-strong ring-1 ring-warning/30"
								role="alert"
								data-testid="doc-detail-problem"
							>
								<div class="flex items-start gap-2">
									<AlertTriangleIcon class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
									{#if doc.problem.kind === 'invalid_metadata'}
										<p>
											Details not recognized: <span class="font-mono">{doc.problem.field}</span> — {doc
												.problem.message}. The document body below stays readable.
										</p>
									{:else}
										<p>
											Duplicate display key <span class="font-mono">{doc.problem.key}</span> is also
											used by {doc.problem.others
												.map((o) => `${o.title} (${o.scopeLabel})`)
												.join(', ')}. Stable IDs stay unchanged.
										</p>
									{/if}
								</div>
								{#if canEdit}
									<Button
										type="button"
										variant="outline"
										size="sm"
										class={mobileButton}
										onclick={() => beginEdit(doc)}
									>
										{doc.problem.kind === 'invalid_metadata' ? 'Fix' : 'Resolve key…'}
									</Button>
								{/if}
							</div>
						{/if}
					</div>

					{#if activeView === 'history'}
						<DocumentHistoryPanel {uid} projectId={frame.projectId} {doc} {formatWhen} />
					{:else}
						<DocumentReaderBody {uid} {doc} kinds={overview.kinds} />
					{/if}
				{/if}

				<DocumentDialogs
					bind:this={dialogs}
					{uid}
					organizationId={frame.organizationId}
					projectId={frame.projectId}
					{doc}
					bind:decisionWarnOpen
					onEditAnyway={beginEdit}
					onIssued={async () => {
						await existingRead?.reload();
					}}
				/>
			{/if}
		{/snippet}
	</Loaded>
{/if}
