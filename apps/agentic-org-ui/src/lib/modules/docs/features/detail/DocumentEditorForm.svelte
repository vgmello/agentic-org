<!--
  @component
  Synchronized Details & body / Source editor for DocumentPage (TPL-2, conflict diff, failed-save copyable draft).
-->
<script lang="ts">
	import CopyButton from '$lib/components/CopyButton.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		errorClass,
		gutters,
		inputClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { isTreeDocument, kindTitle, slugifyFileName } from '../../present';
	import type {
		DocField,
		DocumentDraft,
		ProjectDocsOverview,
		ProjectDocumentDetail
	} from '../../types';
	import DocDiffView from '../../components/DocDiffView.svelte';

	let {
		uid,
		isNew,
		overview,
		existing,
		derivedPathPreview,
		editMode,
		conflictDoc,
		saveError,
		saveBusy,
		sourceError,
		fieldErrors,
		validationErrorList,
		draft = $bindable<DocumentDraft | null>(null),
		sourceText = $bindable(''),
		changeSummary = $bindable(''),
		fileNameTouched = $bindable(false),
		onSwitchMode,
		onSave,
		onLoadLatest,
		onCancel
	}: {
		uid: string;
		isNew: boolean;
		overview: ProjectDocsOverview;
		existing: ProjectDocumentDetail | undefined;
		derivedPathPreview: string;
		editMode: 'details' | 'source';
		conflictDoc: ProjectDocumentDetail | null;
		saveError: string | undefined;
		saveBusy: boolean;
		sourceError: string | undefined;
		fieldErrors: Partial<Record<DocField, string>>;
		validationErrorList: string[];
		draft: DocumentDraft | null;
		sourceText: string;
		changeSummary: string;
		fileNameTouched: boolean;
		onSwitchMode: (mode: 'details' | 'source') => void;
		onSave: (expectedRevision?: number) => void;
		onLoadLatest: (latest: ProjectDocumentDetail) => void;
		onCancel: () => void;
	} = $props();
</script>

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
				role="group"
				aria-label="Editor mode"
				class="inline-flex rounded-md bg-surface-raised p-0.5 ring-1 ring-line"
			>
				<button
					type="button"
					aria-pressed={editMode === 'details'}
					onclick={() => onSwitchMode('details')}
					class="rounded-sm px-3 py-1.5 text-xs/5 font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {editMode ===
					'details'
						? 'bg-surface-overlay text-fg'
						: 'text-fg-muted hover:text-fg'}"
				>
					Details & body
				</button>
				<button
					type="button"
					aria-pressed={editMode === 'source'}
					onclick={() => onSwitchMode('source')}
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
						onclick={() => onSave(conflictDoc.revision)}
					>
						{saveBusy ? 'Saving…' : `Save my draft at rev ${conflictDoc.revision}`}
					</Button>
					<Button
						type="button"
						variant="outline"
						size="sm"
						class={mobileButton}
						onclick={() => onLoadLatest(conflictDoc)}
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
						onclick={() => onSave()}
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

		{#if editMode === 'source' && validationErrorList.length > 0}
			<div
				class="flex flex-col gap-1 rounded-lg bg-danger-emphasis/10 p-3 text-xs/5 text-danger-text ring-1 ring-danger-emphasis/20"
				role="alert"
				data-testid="source-validation-errors"
			>
				{#each validationErrorList as msg (msg)}
					<p>{msg}</p>
				{/each}
			</div>
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
								if (draft) draft.sectionPath = (e.currentTarget as HTMLSelectElement).value || null;
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
			<Button type="button" class={mobileButton} disabled={saveBusy} onclick={() => onSave()}>
				{saveBusy ? 'Saving…' : existing ? 'Save changes' : 'Create document'}
			</Button>
			<Button
				type="button"
				variant="outline"
				class={mobileButton}
				disabled={saveBusy}
				onclick={onCancel}
			>
				Cancel
			</Button>
		</div>
	</div>
{/if}
