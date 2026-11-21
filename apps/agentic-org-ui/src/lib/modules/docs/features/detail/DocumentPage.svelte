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
	import FileCodeIcon from '@lucide/svelte/icons/file-code';
	import Loaded from '$lib/components/Loaded.svelte';
	import Avatar from '$lib/components/Avatar.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import CopyButton from '$lib/components/CopyButton.svelte';
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
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/proto';
	import { projectFrame } from '$lib/modules/projects';
	import { docsApi } from '../../api';
	import {
		checkDocumentDraft,
		conflictLatestDoc,
		defaultSectionForKind,
		deriveCanonicalPath,
		docHref,
		docsHref,
		draftFromDetail,
		expectedSectionsFor,
		isTreeDocument,
		kindTitle,
		newDocHref,
		parseDocumentSource,
		scopeLabel,
		serializeDocumentSource,
		slugifyFileName,
		templateBodyFor
	} from '../../present';
	import type {
		DocField,
		DocumentDraft,
		ProjectDocsOverview,
		ProjectDocumentDetail
	} from '../../types';
	import DocLifecycleBadge from '../../components/DocLifecycleBadge.svelte';
	import DocMarkdown from '../../components/DocMarkdown.svelte';
	import DocDiffView from '../../components/DocDiffView.svelte';

	let { docId = null }: { docId?: string | null } = $props();

	const frame = projectFrame();
	const uid = $props.id();
	const isNew = $derived(docId === null);

	type DocLookup =
		| { found: true; doc: ProjectDocumentDetail; overview: ProjectDocsOverview }
		| { found: false; message: string; overview: ProjectDocsOverview };

	async function loadExisting(): Promise<DocLookup> {
		const overview = await docsApi.list(frame.organizationId, frame.projectId);
		try {
			const doc = await docsApi.get(frame.organizationId, frame.projectId, docId!);
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
	const overviewRead = $derived(
		docId === null
			? new Resource<ProjectDocsOverview>(() => docsApi.list(frame.organizationId, frame.projectId))
			: null
	);

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

	function buildNewDraft(overview: ProjectDocsOverview): DocumentDraft {
		const kind = page.url.searchParams.get('kind') ?? 'decision';
		const templateParam = page.url.searchParams.get('template');
		const templateVariant =
			templateParam === 'evaluation' || templateParam === 'review' ? templateParam : undefined;
		const supersedesId = page.url.searchParams.get('supersedes');
		const superseded = supersedesId
			? overview.documents.find((d) => d.id === supersedesId)
			: undefined;
		const sectionParam =
			page.url.searchParams.get('section') ??
			defaultSectionForKind(kind) ??
			overview.sections[0]?.path ??
			'decisions';

		let body = templateBodyFor(kind, templateVariant, overview.kinds);
		let initialTitle = '';
		if (superseded) {
			const label = superseded.key ? `${superseded.key} (${superseded.title})` : superseded.title;
			initialTitle = `Supersedes ${superseded.key ?? superseded.title}`;
			body = `## Context\nSupersedes ${label}.\n\n## Options considered\n\n## Decision\n\n## Consequences\n\n## Evidence\n\n## Tech debt accepted\n\n## Not covered here\n`;
		}

		return {
			kind,
			title: initialTitle,
			key: null,
			summary: '',
			status: 'draft',
			ownerId: overview.owners[0]?.id ?? null,
			sectionPath: kind === 'brief' ? null : sectionParam,
			fileName: kind === 'brief' ? null : slugifyFileName(initialTitle || `new-${kind}`),
			scope: { kind: 'project' },
			bodyMarkdown: body,
			supersedesId: supersedesId ?? null
		};
	}

	// Sub-mode for existing docs: read | edit | history (writable $derived handles ?edit=details without $effect)
	let activeView = $derived<'read' | 'edit' | 'history'>(
		!isNew && !!currentDoc && canEdit && page.url.searchParams.get('edit') === 'details'
			? 'edit'
			: 'read'
	);
	let editMode = $state<'details' | 'source'>('details');

	// Current decision edit warning dialog (DEC-6 / §4)
	let decisionWarnOpen = $state(false);

	// Issue version dialog (organization-data 18)
	let issueOpen = $state(false);
	let issueNote = $state('');
	let issueDerivedIds = $state<string[]>([]);
	let issueBusy = $state(false);
	let issueError = $state<string | undefined>();

	// Editor draft state (writable $derived with $state proxy for bound fields, zero $effect)
	let draft = $derived.by<DocumentDraft | null>(() => {
		if (isNew && currentOverview) {
			const next = $state(buildNewDraft(currentOverview));
			return next;
		}
		if (!isNew && currentDoc) {
			const next = $state(draftFromDetail(currentDoc));
			return next;
		}
		return null;
	});
	let sourceText = $derived.by(() => {
		if (!draft) return '';
		const metaId = currentDoc?.id ?? 'pending-uuidv7';
		const metaRev = currentDoc?.revision ?? 1;
		const path =
			currentDoc?.path ??
			deriveCanonicalPath(
				frame.projectId,
				metaId,
				draft.kind,
				draft.scope,
				draft.sectionPath,
				draft.fileName
			);
		return serializeDocumentSource(draft, {
			id: metaId,
			revision: metaRev,
			projectId: frame.projectId,
			path
		});
	});
	let sourceError = $state<string | undefined>();
	let fieldErrors = $state<Partial<Record<DocField, string>>>({});
	let changeSummary = $state('');
	let saveBusy = $state(false);
	let saveError = $state<string | undefined>();
	let conflictDoc = $state<ProjectDocumentDetail | null>(null);
	let fileNameTouched = $state(false);

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

	interface CompareSnapshot {
		key: string;
		label: string;
		body: string;
	}

	const snapshots = $derived.by<CompareSnapshot[]>(() => {
		if (!currentDoc) return [];
		const list: CompareSnapshot[] = [
			{
				key: 'working',
				label: `Working copy (rev ${currentDoc.revision})`,
				body: currentDoc.bodyMarkdown
			}
		];
		for (const v of [...currentDoc.versions].toReversed()) {
			list.push({
				key: `v:${v.version}`,
				label: `Issued v${v.version} (rev ${v.revision})`,
				body: v.bodyMarkdown
			});
		}
		for (const h of [...currentDoc.history].toReversed()) {
			if (h.revision === currentDoc.revision) continue;
			list.push({
				key: `rev:${h.revision}`,
				label: `Revision ${h.revision} · ${h.summary}`,
				body: h.bodyMarkdown
			});
		}
		return list;
	});

	// History & compare selectors (writable $derived keeps valid defaults without $effect)
	let compareLeft = $derived(snapshots[1]?.key ?? '');
	let compareRight = $derived(snapshots[0]?.key ?? '');
	const leftSnap = $derived(snapshots.find((s) => s.key === compareLeft) ?? snapshots[1]);
	const rightSnap = $derived(snapshots.find((s) => s.key === compareRight) ?? snapshots[0]);

	function formatWhen(iso: string): string {
		const date = new Date(iso);
		if (Number.isNaN(date.getTime())) return iso;
		return formatDate(date, accountZone.zone, new Date());
	}

	function beginEdit(doc: ProjectDocumentDetail) {
		const next = draftFromDetail(doc);
		draft = next;
		sourceText = serializeDocumentSource(next, {
			id: doc.id,
			revision: doc.revision,
			projectId: doc.projectId,
			path: doc.path
		});
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
			const metaId = currentDoc?.id ?? 'pending-uuidv7';
			const metaRev = currentDoc?.revision ?? 1;
			const path = deriveCanonicalPath(
				frame.projectId,
				metaId,
				draft.kind,
				draft.scope,
				draft.sectionPath,
				draft.fileName
			);
			sourceText = serializeDocumentSource(draft, {
				id: metaId,
				revision: metaRev,
				projectId: frame.projectId,
				path
			});
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

	function openIssueDialog() {
		issueNote = '';
		issueDerivedIds = [];
		issueError = undefined;
		issueOpen = true;
	}

	async function submitIssueVersion(event: SubmitEvent) {
		event.preventDefault();
		if (!currentDoc || issueBusy) return;
		issueBusy = true;
		issueError = undefined;
		try {
			const derivedFrom = currentDoc.derivableSources
				.filter((s) => issueDerivedIds.includes(`${s.documentId}:${s.version}`))
				.map(({ documentId, version }) => ({ documentId, version }));
			const updated = await docsApi.issueVersion(
				frame.organizationId,
				frame.projectId,
				currentDoc.id,
				{
					note: issueNote.trim(),
					derivedFrom,
					expectedRevision: currentDoc.revision
				}
			);
			issueOpen = false;
			toast.success(`Issued version v${updated.version} of ${updated.title}.`);
			await existingRead?.reload();
		} catch (e) {
			issueError = toAppError(e).message;
		} finally {
			issueBusy = false;
		}
	}

	function toggleDerivedSource(key: string) {
		issueDerivedIds = issueDerivedIds.includes(key)
			? issueDerivedIds.filter((k) => k !== key)
			: [...issueDerivedIds, key];
	}

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
</script>

<svelte:head><title>{title}</title></svelte:head>

{#snippet editorForm(overview: ProjectDocsOverview, existing: ProjectDocumentDetail | undefined)}
	{#if draft}
		<div class="flex flex-col gap-6 py-6 {gutters}" data-testid="doc-editor">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div>
					<h3 class="text-base/7 font-semibold text-fg">
						{existing ? `Edit ${existing.title}` : `New ${kindTitle(draft.kind, overview.kinds)}`}
					</h3>
					<p class="mt-0.5 font-mono text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
						{derivedPathPreview}
					</p>
				</div>

				<div
					role="tablist"
					aria-label="Editor mode"
					class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
				>
					<button
						type="button"
						role="tab"
						aria-selected={editMode === 'details'}
						onclick={() => switchEditMode('details')}
						class="rounded-sm px-3 py-1.5 text-xs/5 font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {editMode ===
						'details'
							? 'bg-surface-overlay text-fg'
							: 'text-fg-muted hover:text-fg'}"
					>
						Details & body
					</button>
					<button
						type="button"
						role="tab"
						aria-selected={editMode === 'source'}
						onclick={() => switchEditMode('source')}
						class="rounded-sm px-3 py-1.5 text-xs/5 font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {editMode ===
						'source'
							? 'bg-surface-overlay text-fg'
							: 'text-fg-muted hover:text-fg'}"
					>
						Source
					</button>
				</div>
			</div>

			{#if conflictDoc}
				<div
					class="flex flex-col gap-3 rounded-lg bg-warning/10 p-4 ring-1 ring-warning/30"
					role="alert"
					data-testid="doc-conflict"
				>
					<p class="text-sm/6 font-semibold text-warning-text-strong">
						{conflictDoc.title} changed while you were editing (rev {conflictDoc.revision}).
					</p>
					<p class="text-xs/5 text-fg-secondary">
						Your typed draft is kept below. Review the difference against the latest saved revision
						before retrying.
					</p>
					<DocDiffView
						before={conflictDoc.bodyMarkdown}
						after={draft.bodyMarkdown}
						label="Latest saved (rev {conflictDoc.revision}) vs your draft"
					/>
					<div class="flex flex-wrap gap-2">
						<Button
							type="button"
							size="sm"
							class={mobileButton}
							disabled={saveBusy}
							onclick={() => submitSave(conflictDoc!.revision)}
						>
							{saveBusy ? 'Saving…' : `Save my draft at rev ${conflictDoc.revision}`}
						</Button>
						<Button
							type="button"
							variant="outline"
							size="sm"
							class={mobileButton}
							onclick={() => {
								beginEdit(conflictDoc!);
							}}
						>
							Load latest revision
						</Button>
					</div>
				</div>
			{:else if saveError}
				<div
					class="flex flex-col gap-2 rounded-lg bg-danger-emphasis/10 p-4 ring-1 ring-danger-emphasis/20"
					role="alert"
					data-testid="doc-save-error"
				>
					<p class="text-sm/6 font-semibold text-danger-text">Couldn't save document</p>
					<p class="text-xs/5 text-fg-secondary">{saveError}</p>
					<p class="text-xs/5 text-fg-muted">Your typed text is kept. You can copy it or retry.</p>
					<div class="flex flex-wrap gap-2">
						<Button
							type="button"
							size="sm"
							class={mobileButton}
							disabled={saveBusy}
							onclick={() => submitSave()}
						>
							Retry save
						</Button>
						<CopyButton text={draft.bodyMarkdown} label="Copy body" size="sm" />
					</div>
				</div>
			{/if}

			{#if sourceError}
				<p class={errorClass} role="alert" data-testid="source-error">{sourceError}</p>
			{/if}

			{#if editMode === 'details'}
				<div class="grid grid-cols-1 gap-4 min-[768px]:grid-cols-2">
					<div class="min-[768px]:col-span-2">
						<Label for="{uid}-title" class="mb-1 block text-sm/6 font-medium text-fg">Title</Label>
						<Input
							id="{uid}-title"
							bind:value={draft.title}
							oninput={() => {
								if (isNew && !fileNameTouched && draft && isTreeDocument(draft.kind, draft.scope)) {
									draft.fileName = slugifyFileName(draft.title);
								}
							}}
							aria-invalid={fieldErrors.title ? 'true' : undefined}
							class={inputClass}
						/>
						{#if fieldErrors.title}
							<p class={errorClass} role="alert">{fieldErrors.title}</p>
						{/if}
					</div>

					<div>
						<Label for="{uid}-key" class="mb-1 block text-sm/6 font-medium text-fg">
							Display key (optional)
						</Label>
						<Input
							id="{uid}-key"
							placeholder="ADR-0005"
							value={draft.key ?? ''}
							oninput={(e) => {
								if (draft) draft.key = (e.currentTarget as HTMLInputElement).value || null;
							}}
							aria-invalid={fieldErrors.key ? 'true' : undefined}
							class={inputClass}
						/>
						<p class="mt-1 text-xs/5 text-fg-muted">
							Optional label only. Identity stays on the immutable stable ID.
						</p>
						{#if fieldErrors.key}
							<p class={errorClass} role="alert">{fieldErrors.key}</p>
						{/if}
					</div>

					<div>
						<Label for="{uid}-status" class="mb-1 block text-sm/6 font-medium text-fg">
							Lifecycle status
						</Label>
						<select
							id="{uid}-status"
							bind:value={draft.status}
							aria-invalid={fieldErrors.status ? 'true' : undefined}
							class={selectClass}
						>
							<option value="draft">Draft</option>
							<option value="current">Current</option>
							<option value="deprecated">Deprecated</option>
						</select>
					</div>

					<div>
						<Label for="{uid}-owner" class="mb-1 block text-sm/6 font-medium text-fg">
							Accountable owner
						</Label>
						<select
							id="{uid}-owner"
							value={draft.ownerId ?? ''}
							onchange={(e) => {
								if (draft) draft.ownerId = (e.currentTarget as HTMLSelectElement).value || null;
							}}
							aria-invalid={fieldErrors.ownerId ? 'true' : undefined}
							class={selectClass}
						>
							<option value="">Unassigned</option>
							{#each overview.owners as owner (owner.id)}
								<option value={owner.id}>{owner.name} ({owner.kind})</option>
							{/each}
						</select>
						{#if fieldErrors.ownerId}
							<p class={errorClass} role="alert">{fieldErrors.ownerId}</p>
						{/if}
					</div>

					<div>
						<Label for="{uid}-summary" class="mb-1 block text-sm/6 font-medium text-fg">
							One-line summary
						</Label>
						<Input
							id="{uid}-summary"
							placeholder="Why someone should open this document."
							bind:value={draft.summary}
							class={inputClass}
						/>
					</div>

					{#if isTreeDocument(draft.kind, draft.scope)}
						<div>
							<Label for="{uid}-section" class="mb-1 block text-sm/6 font-medium text-fg">
								Documentation section
							</Label>
							<select
								id="{uid}-section"
								value={draft.sectionPath ?? ''}
								onchange={(e) => {
									if (draft)
										draft.sectionPath = (e.currentTarget as HTMLSelectElement).value || null;
								}}
								aria-invalid={fieldErrors.sectionPath ? 'true' : undefined}
								class={selectClass}
							>
								{#each overview.sections as sec (sec.path)}
									<option value={sec.path}>{sec.title} (docs/{sec.path})</option>
								{/each}
							</select>
							{#if fieldErrors.sectionPath}
								<p class={errorClass} role="alert">{fieldErrors.sectionPath}</p>
							{/if}
						</div>

						<div>
							<Label for="{uid}-filename" class="mb-1 block text-sm/6 font-medium text-fg">
								File name
							</Label>
							<Input
								id="{uid}-filename"
								value={draft.fileName ?? ''}
								oninput={(e) => {
									fileNameTouched = true;
									if (draft) draft.fileName = (e.currentTarget as HTMLInputElement).value;
								}}
								aria-invalid={fieldErrors.fileName ? 'true' : undefined}
								class="font-mono {inputClass}"
							/>
							{#if fieldErrors.fileName}
								<p class={errorClass} role="alert">{fieldErrors.fileName}</p>
							{/if}
						</div>
					{/if}
				</div>

				<div>
					<Label for="{uid}-body" class="mb-1 block text-sm/6 font-medium text-fg">
						Body (Markdown)
					</Label>
					<textarea
						id="{uid}-body"
						rows={14}
						bind:value={draft.bodyMarkdown}
						class="font-mono {textareaClass}"></textarea>
				</div>
			{:else}
				<div>
					<Label for="{uid}-source" class="mb-1 block text-sm/6 font-medium text-fg">
						Front matter and Markdown source
					</Label>
					<textarea
						id="{uid}-source"
						rows={20}
						bind:value={sourceText}
						class="font-mono {textareaClass}"></textarea>
					<p class="mt-1 text-xs/5 text-fg-muted">
						Synchronized with the Details form. Immutable identity fields (<span class="font-mono"
							>id</span
						>, <span class="font-mono">project_id</span>, <span class="font-mono">kind</span>) are
						kept by the server.
					</p>
				</div>
			{/if}

			{#if existing}
				<div class="max-w-lg">
					<Label for="{uid}-change-note" class="mb-1 block text-sm/6 font-medium text-fg">
						Revision note (optional)
					</Label>
					<Input
						id="{uid}-change-note"
						placeholder="What changed in this revision?"
						bind:value={changeSummary}
						class={inputClass}
					/>
				</div>
			{/if}

			<div class="flex flex-wrap items-center gap-3 border-t border-line-subtle pt-4">
				<Button type="button" class={mobileButton} disabled={saveBusy} onclick={() => submitSave()}>
					{saveBusy ? 'Saving…' : existing ? 'Save changes' : 'Create document'}
				</Button>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={saveBusy}
					onclick={() => {
						if (existing) {
							activeView = 'read';
							conflictDoc = null;
							saveError = undefined;
						} else {
							void goto(resolve(docsHref(frame.projectId) as '/'));
						}
					}}
				>
					Cancel
				</Button>
			</div>
		</div>
	{/if}
{/snippet}

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
				{@render editorForm(overview, undefined)}
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
				{@const sectionChecklist = expectedSectionsFor(doc.kind, doc.bodyMarkdown, overview.kinds)}

				{#if activeView === 'edit'}
					{@render editorForm(overview, doc)}
				{:else}
					<!-- Reader & History header card (DEC-2, never raw YAML) -->
					<div class="border-b border-line-subtle py-6 {gutters}" data-testid="doc-header-card">
						<div class="flex flex-wrap items-start justify-between gap-4">
							<div class="min-w-0 flex-1 basis-72">
								<div class="flex flex-wrap items-center gap-2 text-xs/5 text-fg-muted">
									<span class="font-semibold text-fg-secondary"
										>{kindTitle(doc.kind, overview.kinds)}</span
									>
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
										onclick={openIssueDialog}
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
						<!-- History & Diff Compare view -->
						<div class="flex flex-col gap-6 py-6 {gutters}" data-testid="doc-history-panel">
							{#if doc.versions.length > 0}
								<section aria-labelledby="{uid}-versions-h">
									<h4 id="{uid}-versions-h" class="text-sm/6 font-semibold text-fg">
										Issued versions ({doc.versions.length})
									</h4>
									<ul
										role="list"
										class="mt-2 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
									>
										{#each [...doc.versions].toReversed() as ver (ver.version)}
											<li class="px-4 py-3 text-sm/6" data-testid="issued-version-row">
												<div class="flex flex-wrap items-baseline justify-between gap-2">
													<div class="flex flex-wrap items-baseline gap-2">
														<span class="font-mono font-semibold text-fg">v{ver.version}</span>
														<span class="text-fg-secondary">{ver.title}</span>
														<span class="text-xs/5 text-fg-muted">rev {ver.revision}</span>
													</div>
													<span class="text-xs/5 text-fg-muted">
														Issued <time datetime={ver.issuedAt}>{formatWhen(ver.issuedAt)}</time>
														by {ver.issuedBy.name}
													</span>
												</div>
												{#if ver.note}
													<p class="mt-1 text-xs/5 text-fg-body">{ver.note}</p>
												{/if}
												{#if ver.derivedFrom.length > 0}
													<p class="mt-1 text-xs/5 text-fg-muted">
														Derived from:
														{#each ver.derivedFrom as src, idx (`${src.documentId}-${src.version}`)}
															{#if idx > 0},
															{/if}
															<a
																href={resolve(docHref(frame.projectId, src.documentId) as '/')}
																class="text-link hover:underline"
															>
																{src.key ? `${src.key} ` : ''}{src.title} v{src.version}
															</a>
														{/each}
													</p>
												{/if}
											</li>
										{/each}
									</ul>
								</section>
							{/if}

							{#if snapshots.length >= 2}
								<section aria-labelledby="{uid}-compare-h" class="flex flex-col gap-3">
									<h4 id="{uid}-compare-h" class="text-sm/6 font-semibold text-fg">Compare</h4>
									<div class="grid grid-cols-1 gap-3 min-[640px]:grid-cols-2">
										<div>
											<Label
												for="{uid}-cmp-left"
												class="mb-1 block text-xs/5 font-medium text-fg-secondary"
											>
												From (older)
											</Label>
											<select id="{uid}-cmp-left" bind:value={compareLeft} class={selectClass}>
												{#each snapshots as snap (snap.key)}
													<option value={snap.key}>{snap.label}</option>
												{/each}
											</select>
										</div>
										<div>
											<Label
												for="{uid}-cmp-right"
												class="mb-1 block text-xs/5 font-medium text-fg-secondary"
											>
												To (newer)
											</Label>
											<select id="{uid}-cmp-right" bind:value={compareRight} class={selectClass}>
												{#each snapshots as snap (snap.key)}
													<option value={snap.key}>{snap.label}</option>
												{/each}
											</select>
										</div>
									</div>
									{#if leftSnap && rightSnap}
										<DocDiffView
											before={leftSnap.body}
											after={rightSnap.body}
											label="{leftSnap.label} → {rightSnap.label}"
										/>
									{/if}
								</section>
							{/if}

							<section aria-labelledby="{uid}-revs-h">
								<h4 id="{uid}-revs-h" class="text-sm/6 font-semibold text-fg">
									Git revision history ({doc.history.length})
								</h4>
								<ul
									role="list"
									class="mt-2 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
								>
									{#each [...doc.history].toReversed() as entry (entry.revision)}
										<li
											class="flex flex-wrap items-baseline justify-between gap-2 px-4 py-3 text-sm/6"
										>
											<div>
												<span class="font-mono text-xs/5 font-semibold text-fg"
													>rev {entry.revision}</span
												>
												<span class="ml-2 text-fg-body">{entry.summary}</span>
											</div>
											<span class="text-xs/5 text-fg-muted">
												{entry.by.name} · <time datetime={entry.at}>{formatWhen(entry.at)}</time>
											</span>
										</li>
									{/each}
								</ul>
							</section>
						</div>
					{:else}
						<!-- Reader layout: Section rail + Body + Code links + Footer -->
						<div
							class="grid grid-cols-1 gap-8 py-6 min-[1024px]:grid-cols-[14rem_minmax(0,1fr)] {gutters}"
						>
							<!-- Section rail (DEC-2, §3.0) -->
							<nav
								aria-label="Document sections"
								class="flex flex-col gap-1"
								data-testid="doc-section-rail"
							>
								<p class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase">
									Sections
								</p>
								<ul role="list" class="mt-1 flex flex-col gap-1">
									{#each sectionChecklist as sec (sec.heading)}
										<li class="text-xs/5">
											{#if sec.present}
												<a
													href={`#doc-sec-${sec.heading
														.toLowerCase()
														.replace(/[^a-z0-9]+/g, '-')
														.replace(/^-+|-+$/g, '')}`}
													class="block rounded-sm py-1 text-fg-secondary hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
												>
													{sec.heading}
												</a>
												{#if sec.empty && sec.hint}
													<p class="text-fg-muted">{sec.hint}</p>
												{/if}
											{:else}
												<p class="py-1 text-fg-muted">
													{sec.heading} — {sec.hint}
												</p>
											{/if}
										</li>
									{/each}
								</ul>
							</nav>

							<div class="flex min-w-0 flex-col gap-8">
								<article aria-label="Document body" data-testid="doc-body">
									<DocMarkdown body={doc.bodyMarkdown} baseLevel={4} />
								</article>

								{#if doc.codeLinks.length > 0}
									<section
										aria-labelledby="{uid}-codelinks-h"
										class="border-t border-line-subtle pt-5"
										data-testid="doc-code-links"
									>
										<h4 id="{uid}-codelinks-h" class="text-sm/6 font-semibold text-fg">
											Code links ({doc.codeLinks.length})
										</h4>
										<ul role="list" class="mt-2 flex flex-col gap-2">
											{#each doc.codeLinks as cl (cl.id)}
												<li
													class="flex flex-wrap items-start justify-between gap-2 rounded-lg bg-surface-raised p-3 text-xs/5 ring-1 ring-line"
												>
													<div class="flex min-w-0 items-start gap-2">
														<FileCodeIcon
															class="mt-0.5 size-4 shrink-0 text-fg-muted"
															aria-hidden="true"
														/>
														<div class="min-w-0">
															<p class="font-mono text-fg [overflow-wrap:anywhere]">
																{cl.repositoryKey} · {cl.path}{cl.symbol ? `#${cl.symbol}` : ''}
															</p>
															{#if cl.note}
																<p class="mt-0.5 text-fg-muted">{cl.note}</p>
															{/if}
														</div>
													</div>
													{#if cl.stale}
														<StatusPill label="Path moved or missing" class={tone.owner} />
													{/if}
												</li>
											{/each}
										</ul>
									</section>
								{/if}

								<!-- Footer: Led to & Mentioned in (WHY-2) -->
								<footer
									class="grid grid-cols-1 gap-6 border-t border-line-subtle pt-6 min-[640px]:grid-cols-2"
									data-testid="doc-why-footer"
								>
									<section aria-labelledby="{uid}-ledto-h">
										<h4 id="{uid}-ledto-h" class="text-sm/6 font-semibold text-fg">
											Led to ({doc.ledTo.length})
										</h4>
										{#if doc.ledTo.length === 0}
											<p class="mt-1 text-xs/5 text-fg-muted">
												No work items or documents link this document as their Why yet.
											</p>
										{:else}
											<ul role="list" class="mt-2 flex flex-col gap-1.5 text-sm/6">
												{#each doc.ledTo as item (item.id)}
													<li class="flex items-baseline justify-between gap-2">
														<a
															href={resolve(item.href as '/')}
															class="text-link hover:underline [overflow-wrap:anywhere]"
														>
															{#if item.ref || item.key}
																<span class="mr-1 font-mono text-xs/5">{item.ref ?? item.key}</span>
															{/if}
															{item.title}
														</a>
														<span class="shrink-0 text-xs/5 text-fg-muted">{item.status}</span>
													</li>
												{/each}
											</ul>
										{/if}
									</section>

									<section aria-labelledby="{uid}-mentioned-h">
										<h4 id="{uid}-mentioned-h" class="text-sm/6 font-semibold text-fg">
											Mentioned in ({doc.mentionedIn.length})
										</h4>
										{#if doc.mentionedIn.length === 0}
											<p class="mt-1 text-xs/5 text-fg-muted">Not mentioned elsewhere yet.</p>
										{:else}
											<ul role="list" class="mt-2 flex flex-col gap-1.5 text-sm/6">
												{#each doc.mentionedIn as item (item.id)}
													<li class="flex items-baseline justify-between gap-2">
														<a
															href={resolve(item.href as '/')}
															class="text-link hover:underline [overflow-wrap:anywhere]"
														>
															{#if item.ref || item.key}
																<span class="mr-1 font-mono text-xs/5">{item.ref ?? item.key}</span>
															{/if}
															{item.title}
														</a>
														<span class="shrink-0 text-xs/5 text-fg-muted">{item.status}</span>
													</li>
												{/each}
											</ul>
										{/if}
									</section>
								</footer>
							</div>
						</div>
					{/if}
				{/if}

				<!-- Current decision edit warning dialog (DEC-6 / §4) -->
				<Dialog.Root bind:open={decisionWarnOpen}>
					<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
						<Dialog.Header>
							<Dialog.Title text="Edit a current decision?" />
							<Dialog.Description>
								Decisions are a record. Supersede it if the decision changed, or edit only to fix a
								mistake.
							</Dialog.Description>
						</Dialog.Header>
						<Dialog.Footer>
							<Button
								type="button"
								variant="outline"
								class={mobileButton}
								onclick={() => (decisionWarnOpen = false)}
							>
								Cancel
							</Button>
							<Button
								type="button"
								variant="outline"
								class={mobileButton}
								onclick={() => {
									decisionWarnOpen = false;
									void goto(
										resolve(newDocHref(frame.projectId, 'decision', { supersedes: doc.id }) as '/')
									);
								}}
							>
								Supersede with a new decision
							</Button>
							<Button
								type="button"
								class={mobileButton}
								onclick={() => {
									decisionWarnOpen = false;
									beginEdit(doc);
								}}
							>
								Edit anyway
							</Button>
						</Dialog.Footer>
					</Dialog.Content>
				</Dialog.Root>

				<!-- Issue version dialog (organization-data 18) -->
				<Dialog.Root bind:open={issueOpen}>
					<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
						<Dialog.Header>
							<Dialog.Title text="Issue version v{(doc.version ?? 0) + 1}" />
							<Dialog.Description>
								Fixes the current working text into an immutable version in docs-history and sets
								the document status to Current.
							</Dialog.Description>
						</Dialog.Header>
						<form class="flex flex-col gap-4" onsubmit={submitIssueVersion}>
							<div>
								<Label for="{uid}-issue-note" class="mb-1 block text-sm/6 font-medium text-fg">
									Version note
								</Label>
								<Input
									id="{uid}-issue-note"
									placeholder="What changed in this issued version?"
									bind:value={issueNote}
									class={inputClass}
								/>
							</div>

							{#if doc.derivableSources.length > 0}
								<fieldset class="flex flex-col gap-2">
									<legend class="text-sm/6 font-medium text-fg"> Derived from (optional) </legend>
									<p class="text-xs/5 text-fg-muted">
										Record issued versions of other documents this version follows.
									</p>
									{#each doc.derivableSources as src (`${src.documentId}:${src.version}`)}
										{@const srcKey = `${src.documentId}:${src.version}`}
										<label class="flex items-center gap-2 text-sm/6 text-fg-body">
											<input
												type="checkbox"
												checked={issueDerivedIds.includes(srcKey)}
												onchange={() => toggleDerivedSource(srcKey)}
												class="size-4 rounded border-field-border accent-primary"
											/>
											<span>
												{src.key ? `${src.key} · ` : ''}{src.title}
												<span class="font-mono text-xs/5 text-fg-muted">v{src.version}</span>
											</span>
										</label>
									{/each}
								</fieldset>
							{/if}

							{#if issueError}
								<p class={errorClass} role="alert">{issueError}</p>
							{/if}
							<Dialog.Footer>
								<Button
									type="button"
									variant="outline"
									class={mobileButton}
									disabled={issueBusy}
									onclick={() => (issueOpen = false)}
								>
									Cancel
								</Button>
								<Button type="submit" class={mobileButton} disabled={issueBusy}>
									{issueBusy ? 'Issuing…' : `Issue v${(doc.version ?? 0) + 1}`}
								</Button>
							</Dialog.Footer>
						</form>
					</Dialog.Content>
				</Dialog.Root>
			{/if}
		{/snippet}
	</Loaded>
{/if}
