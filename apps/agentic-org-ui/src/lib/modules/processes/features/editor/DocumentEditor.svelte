<!--
  @component
  The process and standard editor (processes-ux §1, §2): a new document at a scope, an override of an inherited one
  (its text as the starting point, the same key, a new ID), a document from the review-and-merge template, an edit
  or a Fix. Fields in the order a person decides them: title, key, owner, lifecycle, whether lower levels may replace
  it, and the text. The difference from the document it replaces is one click away.
  An edit sends the revision the person saw; a conflict keeps every input and names the effect of saving again
  ("Save over rev N"). A refusal shows on its field and focuses it; replacing a document marked not overridable is
  refused with the rule that refused it. Saving with nothing changed says so. Text left unsaved is kept for the
  session and put back when the same editor opens again; Cancel discards it.
-->
<script lang="ts">
	import { beforeNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import Callout from '$lib/components/Callout.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import {
		errorClass,
		helpClass,
		inputClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/proto';
	import { pageTitle } from '$lib/nav';
	import { processesApi } from '../../api';
	import DiffView from '../../components/DiffView.svelte';
	import PageTitle from '../../components/PageTitle.svelte';
	import { wrapButton } from '../../components/classes';
	import {
		KIND_TAB,
		KIND_WORD,
		LIFECYCLE_WORD,
		checkDraft,
		conflictLatest,
		draftOf,
		invalidText,
		listHref,
		normalizeDraft,
		placeLabel,
		placeOfDocument,
		placeParam,
		refusedField,
		sameDraft,
		sourceText
	} from '../../present';
	import type { DocumentDraft, DocumentField, Lifecycle, ProcessDocument } from '../../types';
	import { arriveWith } from '../place-view/arrival';
	import { discardDraft, draftSlot, keepDraft, keptDraft } from './kept-drafts';
	import { loadEditor, type EditorMode, type EditorStart } from './load';

	let { organizationId, mode }: { organizationId: string; mode: EditorMode } = $props();

	// A document that isn't here is a state of its own; the fields fill once the read arrives.
	const resource = new Resource(async (): Promise<EditorStart | { notFound: true }> => {
		try {
			const loaded = await loadEditor(organizationId, mode);
			fill(loaded);
			return loaded;
		} catch (e) {
			const failure = toAppError(e);
			if (failure.source === 'api' && failure.code === 'not_found') return { notFound: true };
			throw e;
		}
	});
	const slot = $derived(
		draftSlot(
			organizationId,
			mode.kind === 'edit'
				? mode.documentId
				: `new/${mode.documentKind}/${placeParam(mode.place) ?? 'organization'}/${mode.from ?? mode.template ?? ''}`
		)
	);
	const back = $derived(listHref(mode.place, mode.documentKind));

	let draft = $state<DocumentDraft | undefined>();
	let expectedRevision = $state(0);
	let latest = $state.raw<ProcessDocument | undefined>();
	let restored = $state(false);
	let errors = $state<Partial<Record<DocumentField, string>>>({});
	let failure = $state<string | undefined>();
	let notice = $state<string | undefined>();
	let busy = $state(false);
	let comparing = $state(false);
	let leaving = false;

	const start = $derived(resource.data && 'options' in resource.data ? resource.data : undefined);
	const notFound = $derived(!!resource.data && 'notFound' in resource.data);
	const editing = $derived(start?.editing?.document);
	const parent = $derived(start?.from?.document ?? start?.editing?.parent);
	const isOverride = $derived(mode.kind === 'new' && !!start?.from);
	const kindWord = $derived(KIND_WORD[draft?.kind ?? mode.documentKind]);

	/** Fill the fields once the read arrives, with any text kept from last time. */
	function fill(loaded: EditorStart) {
		if (draft) return;
		const kept = keptDraft(slot);
		draft = structuredClone(kept?.draft ?? loaded.draft);
		expectedRevision = kept?.expectedRevision ?? loaded.editing?.document.revision ?? 0;
		latest = kept?.latest;
		restored = !!kept;
	}

	const scopeName = $derived.by(() => {
		if (!draft || !start) return '';
		const option = start.options.places.find((o) =>
			draft!.teamId
				? o.place.kind === 'team' && o.place.id === draft!.teamId
				: draft!.projectId
					? o.place.kind === 'project' && o.place.id === draft!.projectId
					: o.place.kind === 'organization'
		);
		return option ? placeLabel(option) : 'Organization';
	});
	const scopeValue = $derived(
		draft?.teamId
			? `team:${draft.teamId}`
			: draft?.projectId
				? `project:${draft.projectId}`
				: 'organization'
	);
	function setScope(value: string) {
		if (!draft) return;
		const [k, id] = value.split(':');
		draft.teamId = k === 'team' ? id : undefined;
		draft.projectId = k === 'project' ? id : undefined;
	}

	const heading = $derived.by(() => {
		const word = KIND_WORD[mode.documentKind];
		if (editing?.invalid) return `Fix ${editing.title}`;
		if (editing) return `Edit ${editing.title}`;
		if (isOverride && start?.from) return `Override ${start.from.document.title}`;
		return `New ${word}`;
	});
	const submitLabel = $derived(
		latest
			? `Save over rev ${latest.revision}`
			: editing
				? `Save ${kindWord}`
				: `Create ${kindWord}`
	);

	const fields: Record<DocumentField, HTMLElement | null> = $state({
		title: null,
		key: null,
		status: null,
		ownerId: null,
		overridable: null,
		bodyMarkdown: null
	});
	const focusField = (field: DocumentField) => setTimeout(() => fields[field]?.focus(), 0);

	function typed(): boolean {
		if (!draft || !start) return false;
		return !!latest || !sameDraft(draft, start.draft);
	}

	beforeNavigate(() => {
		if (leaving || !draft) return;
		if (typed()) keepDraft(slot, { draft: $state.snapshot(draft), expectedRevision, latest });
		else discardDraft(slot);
	});

	function cancel() {
		leaving = true;
		discardDraft(slot);
		void goto(resolve(back as '/'));
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy || !draft || !start) return;
		const clean = normalizeDraft($state.snapshot(draft));
		errors = checkDraft(clean);
		notice = undefined;
		failure = undefined;
		const first = (['title', 'key', 'ownerId'] as const).find((f) => errors[f]);
		if (first) return focusField(first);
		if (editing && !editing.invalid && !latest && sameDraft(clean, draftOf(editing))) {
			notice = 'Nothing to save: nothing changed.';
			return;
		}
		busy = true;
		try {
			const saved = editing
				? await processesApi.update(organizationId, editing.id, clean, expectedRevision)
				: await processesApi.create(organizationId, clean);
			leaving = true;
			discardDraft(slot);
			// The list of the place it was written for, with its row focused and where it went said (P2-4).
			const to = placeOfDocument(saved);
			arriveWith({
				documentId: saved.id,
				message: `${editing ? 'Saved' : 'Created'} ${saved.title} for ${scopeName}.`
			});
			await goto(resolve(listHref(to, saved.kind) as '/'));
		} catch (e) {
			busy = false;
			const newer = conflictLatest(e);
			if (newer) {
				latest = newer;
				expectedRevision = newer.revision;
				failure = `${newer.updated.byName ?? 'Someone else'} saved rev ${newer.revision} while you were editing. Your text is kept; saving again replaces theirs.`;
				return;
			}
			const field = refusedField(e);
			const message = toAppError(e).message;
			if (field) {
				errors = { [field]: message };
				focusField(field);
			} else failure = message;
		}
	}

	const LIFECYCLES: Lifecycle[] = ['draft', 'current', 'deprecated'];
	const LIFECYCLE_HELP: Record<Lifecycle, string> = {
		draft: 'Written down, not applied yet.',
		current: 'Applies to the work below this level.',
		deprecated: 'Stops applying here; the one above applies again.'
	};
</script>

{#if resource.state !== 'unavailable'}
	<PageTitle title={pageTitle(heading, 'Processes')} />

	<PageHeader
		id="editor-heading"
		title={start
			? heading
			: `${mode.kind === 'edit' ? 'Edit' : 'New'} ${KIND_WORD[mode.documentKind]}`}
		back={{ href: back, label: 'Back to Processes' }}
		description={start && draft
			? `${KIND_TAB[draft.kind] === 'Standards' ? 'Standard' : 'Organization process'} · ${scopeName}`
			: undefined}
	/>
{/if}

{#if resource.state === 'loading'}
	<SkeletonRows label="the editor" rows={5} lines={2} />
{:else if resource.state === 'unavailable'}
	<NotBuilt title="Processes" place="Processes" area="Organization settings" />
{:else if notFound}
	<EmptyState
		title="That document isn't here"
		description="It may have been removed or moved. Its place's list shows what is there now."
	>
		{#snippet action()}
			<Button variant="outline" class={mobileButton} href={resolve(back as '/')}
				>Back to Processes</Button
			>
		{/snippet}
	</EmptyState>
{:else if resource.state === 'error'}
	<ErrorState
		title="Couldn't open the editor"
		message={resource.message}
		onretry={() => resource.reload()}
		retrying={resource.reloading}
	/>
{:else if start && !start.options.viewer.canEdit}
	<EmptyState
		title="You can't change processes"
		description="Ask the organization's owner or an administrator for access."
	/>
{:else if start && draft}
	<form
		class="max-w-3xl space-y-8 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8"
		onsubmit={submit}
		novalidate
	>
		{#if restored}
			<Callout data-testid="restored">
				<p class="text-sm/6 text-fg-secondary">
					Your unsaved text from before is back. Cancel discards it.
				</p>
			</Callout>
		{/if}
		{#if editing?.invalid}
			<Callout data-testid="fixing">
				<p class="text-sm/6 text-fg-secondary">
					{invalidText(editing.title, editing.invalid.field)} Saving writes every field again, so it applies
					once it's saved.
				</p>
			</Callout>
		{/if}
		{#if isOverride && start.from}
			<p class="text-sm/6 text-fg-muted" data-testid="override-note">
				Starts from {sourceText(start.from.document)} rev {start.from.document.revision}. It keeps
				the key {start.from.document.key}, so it replaces that {kindWord} for {scopeName} and everything
				below it.
			</p>
		{/if}

		<div>
			<Label for="doc-title">Title</Label>
			<Input
				bind:ref={fields.title}
				id="doc-title"
				bind:value={draft.title}
				class="mt-2 {inputClass}"
				aria-invalid={errors.title ? true : undefined}
				aria-describedby={errors.title ? 'doc-title-error' : undefined}
			/>
			{#if errors.title}<p id="doc-title-error" class={errorClass}>{errors.title}</p>{/if}
		</div>

		<div>
			<Label for="doc-key">Key <span class="font-normal text-fg-muted">(optional)</span></Label>
			<Input
				bind:ref={fields.key}
				id="doc-key"
				bind:value={() => draft!.key ?? '', (v) => (draft!.key = v)}
				readonly={isOverride}
				autocomplete="off"
				spellcheck={false}
				class="mt-2 font-mono {inputClass} read-only:text-fg-muted"
				aria-invalid={errors.key ? true : undefined}
				aria-describedby={errors.key ? 'doc-key-error' : 'doc-key-help'}
			/>
			{#if errors.key}<p id="doc-key-error" class={errorClass} data-testid="key-error">
					{errors.key}
				</p>
			{:else}<p id="doc-key-help" class={helpClass}>
					{isOverride
						? 'An override keeps the key of the one it replaces.'
						: 'Matches it across levels: a team or project replaces an inherited one by using the same key.'}
				</p>{/if}
		</div>

		<div>
			<Label for="doc-scope">Written for</Label>
			{#if mode.kind === 'new' && !isOverride}
				<select
					id="doc-scope"
					class="mt-2 {selectClass} min-[640px]:max-w-sm"
					value={scopeValue}
					onchange={(e) => setScope(e.currentTarget.value)}
					aria-describedby="doc-scope-help"
				>
					{#each start.options.places as o (placeParam(o.place) ?? 'organization')}
						<option value={placeParam(o.place) ?? 'organization'}>{placeLabel(o)}</option>
					{/each}
				</select>
				<p id="doc-scope-help" class={helpClass}>
					It applies to work there and below. Agents receive it when they start.
				</p>
			{:else}
				<p id="doc-scope" class="mt-2 text-sm/6 text-fg-secondary">{scopeName}</p>
			{/if}
		</div>

		<div>
			<Label for="doc-owner">Owner</Label>
			<select
				bind:this={fields.ownerId}
				id="doc-owner"
				class="mt-2 {selectClass} min-[640px]:max-w-sm"
				bind:value={() => draft!.ownerId ?? '', (v) => (draft!.ownerId = v || undefined)}
				aria-invalid={errors.ownerId ? true : undefined}
				aria-describedby={errors.ownerId ? 'doc-owner-error' : 'doc-owner-help'}
			>
				<option value="">No owner</option>
				{#each start.options.owners as owner (owner.id)}
					<option value={owner.id}>{owner.name}</option>
				{/each}
			</select>
			{#if errors.ownerId}<p id="doc-owner-error" class={errorClass}>{errors.ownerId}</p>
			{:else}<p id="doc-owner-help" class={helpClass}>
					Who keeps it accurate. It grants no permission.
				</p>{/if}
		</div>

		<fieldset
			role="radiogroup"
			aria-invalid={errors.status ? true : undefined}
			aria-describedby={errors.status ? 'doc-status-error' : undefined}
		>
			<legend class="text-sm/6 font-medium text-fg">Lifecycle</legend>
			<div class="mt-2 space-y-2">
				{#each LIFECYCLES as status, i (status)}
					<label class="flex items-start gap-x-3 text-sm/6 max-[639px]:min-h-11">
						<input
							type="radio"
							name="doc-status"
							value={status}
							bind:group={draft.status}
							{@attach (el) => {
								// The first choice takes focus when the office refuses the lifecycle.
								if (i === 0) fields.status = el;
							}}
							class="mt-1 size-4 accent-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
						/>
						<span
							><span class="font-medium text-fg">{LIFECYCLE_WORD[status]}</span>
							<span class="text-fg-muted">· {LIFECYCLE_HELP[status]}</span></span
						>
					</label>
				{/each}
			</div>
			{#if errors.status}<p id="doc-status-error" class={errorClass}>{errors.status}</p>{/if}
		</fieldset>

		<div class="flex items-start gap-x-3">
			<input
				bind:this={fields.overridable}
				id="doc-overridable"
				type="checkbox"
				bind:checked={draft.overridable}
				class="mt-1 size-4 accent-primary focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:size-5"
				aria-describedby={errors.overridable
					? 'doc-overridable-help doc-overridable-error'
					: 'doc-overridable-help'}
				aria-invalid={errors.overridable ? true : undefined}
			/>
			<div>
				<Label for="doc-overridable">Teams and projects below may replace it</Label>
				<p id="doc-overridable-help" class="text-xs/5 text-fg-muted">
					Off: it is not overridable. Every agent below gets it in full as must-follow, and a lower
					version with its key is refused.
				</p>
				{#if errors.overridable}<p id="doc-overridable-error" class={errorClass}>
						{errors.overridable}
					</p>{/if}
			</div>
		</div>

		<div>
			<div class="flex flex-wrap items-end justify-between gap-3">
				<Label for="doc-body">Text</Label>
				{#if parent}
					<Button
						type="button"
						variant="outline"
						size="sm"
						class="{mobileButton} {wrapButton}"
						aria-expanded={comparing}
						onclick={() => (comparing = !comparing)}
						>{comparing ? 'Hide the difference' : `Compare with ${sourceText(parent)}`}</Button
					>
				{/if}
			</div>
			<textarea
				bind:this={fields.bodyMarkdown}
				id="doc-body"
				rows="16"
				bind:value={draft.bodyMarkdown}
				class="mt-2 font-mono {textareaClass}"
				aria-describedby={errors.bodyMarkdown ? 'doc-body-help doc-body-error' : 'doc-body-help'}
				aria-invalid={errors.bodyMarkdown ? true : undefined}></textarea>
			<p id="doc-body-help" class={helpClass}>
				Markdown. Agents read it as written: say what to do, and how to tell it is done.
			</p>
			{#if errors.bodyMarkdown}<p id="doc-body-error" class={errorClass}>
					{errors.bodyMarkdown}
				</p>{/if}
			{#if comparing && parent}
				<div class="mt-4">
					<DiffView
						before={parent.bodyMarkdown}
						after={draft.bodyMarkdown}
						label="From {sourceText(parent)} rev {parent.revision} to this text"
					/>
				</div>
			{/if}
		</div>

		{#if failure}<p class="text-sm/6 text-danger-text" role="alert" data-testid="save-failure">
				{failure}
			</p>{/if}
		{#if notice}<p class="text-sm/6 text-fg-secondary" role="status">{notice}</p>{/if}

		<div class="flex flex-wrap justify-end gap-3 border-t border-line-subtle pt-6">
			<Button type="button" variant="outline" class={mobileButton} disabled={busy} onclick={cancel}
				>Cancel</Button
			>
			<Button type="submit" class="min-w-32 {mobileButton}" aria-busy={busy} aria-disabled={busy}
				>{busy ? (editing ? 'Saving…' : 'Creating…') : submitLabel}</Button
			>
		</div>
	</form>
{/if}
