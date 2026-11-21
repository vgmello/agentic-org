<!--
  @component
  Dedicated Private task notes pane on /work/[ref] (work-items-ux §7.1, ADR-0031, PN-01..06).
  Visually distinct from shared public comments, grouped and attributed by author, strictly read-only
  on other authors' (including agents') notes, and editable/deletable/publishable only on the signed-in
  person's own notes when active and not stale.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { newId } from '$lib/modules/organization/commands';
	import Avatar from '$lib/components/Avatar.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { errorClass, mobileButton, textareaClass } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { when } from '../../present';
	import {
		PRIVATE_NOTES_VISIBILITY_DETAIL,
		PRIVATE_NOTES_VISIBILITY_NOTICE,
		authorKindWord,
		filterPrivateNoteGroups,
		groupPrivateNotes,
		isNoteEdited,
		privateNoteCountLabel
	} from '../../private-notes';
	import type { PrivateNote, PrivateNotesFeed } from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';

	let {
		organizationId,
		taskId,
		stale = false,
		onsharedchanged
	}: {
		organizationId: string;
		taskId: string;
		stale?: boolean;
		onsharedchanged: () => void;
	} = $props();

	const uid = $props.id();

	// Separate private notes store (never mixed into shared task history or counts).
	const notesRead = new Resource<PrivateNotesFeed>(() =>
		workItemsApi.notes(organizationId, taskId)
	);

	let selectedAuthorId = $state<string | 'all'>('all');
	let collapsedAuthors = $state<Record<string, boolean>>({});

	// Create note state
	let noteBody = $state('');
	let noteOpId = $state<string | null>(null);
	let noteBusy = $state(false);
	let noteError = $state<string | undefined>();

	// Edit own note state (retains draft and operation ID across retries/conflicts)
	let editingNote = $state<{
		id: string;
		body: string;
		revision: number;
		operationId: string;
	} | null>(null);
	let editBusy = $state(false);
	let editError = $state<string | undefined>();
	let editConflict = $state(false);

	// Publish & delete dialog targets
	let publishTarget = $state<{ note: PrivateNote; operationId: string } | null>(null);
	let deleteTarget = $state<{ note: PrivateNote; operationId: string } | null>(null);

	const allNotes = $derived(notesRead.data?.notes ?? []);
	const canWrite = $derived(Boolean(notesRead.data?.canWrite) && !stale);
	const groups = $derived(groupPrivateNotes(allNotes));
	const visibleGroups = $derived(filterPrivateNoteGroups(groups, selectedAuthorId));
	const effectiveFilterId = $derived(
		selectedAuthorId === 'all' || groups.some((g) => g.author.id === selectedAuthorId)
			? selectedAuthorId
			: 'all'
	);

	async function reloadNotes() {
		await notesRead.reload();
		if (
			notesRead.state === 'ready' &&
			selectedAuthorId !== 'all' &&
			!groups.some((g) => g.author.id === selectedAuthorId)
		) {
			selectedAuthorId = 'all';
		}
	}

	function isCollapsed(authorId: string): boolean {
		return Boolean(collapsedAuthors[authorId]);
	}

	function toggleAuthorGroup(authorId: string) {
		collapsedAuthors = {
			...collapsedAuthors,
			[authorId]: !collapsedAuthors[authorId]
		};
	}

	function startEdit(note: PrivateNote) {
		if (!note.mine || !canWrite) return;
		editingNote = {
			id: note.id,
			body: note.body,
			revision: note.revision,
			operationId: newId()
		};
		editError = undefined;
		editConflict = false;
	}

	function cancelEdit() {
		editingNote = null;
		editError = undefined;
		editConflict = false;
	}

	async function addPrivateNote(event: SubmitEvent) {
		event.preventDefault();
		if (noteBusy || !canWrite) return;
		const body = noteBody.trim();
		if (!body) {
			noteError = 'Write a note first.';
			return;
		}
		noteBusy = true;
		noteError = undefined;
		const operationId = noteOpId ?? newId();
		noteOpId = operationId;
		try {
			await workItemsApi.addNote(organizationId, taskId, { body, operationId });
			noteBody = '';
			noteOpId = null;
			toast.success('Private note saved.');
			await reloadNotes();
		} catch (e) {
			// Private save failure never falls back to a shared comment.
			noteError = toAppError(e).message;
		} finally {
			noteBusy = false;
		}
	}

	async function saveEditedNote(event: SubmitEvent) {
		event.preventDefault();
		if (!editingNote || editBusy || notesRead.reloading || !canWrite) return;
		const body = editingNote.body.trim();
		if (!body) {
			editError = 'Note cannot be empty.';
			return;
		}
		editBusy = true;
		editError = undefined;
		editConflict = false;
		try {
			await workItemsApi.editNote(organizationId, taskId, editingNote.id, {
				body,
				expectedRevision: editingNote.revision,
				operationId: editingNote.operationId
			});
			editingNote = null;
			toast.success('Private note updated.');
			await reloadNotes();
		} catch (e) {
			const err = toAppError(e);
			editError = err.message;
			editConflict = err.source === 'api' && err.code === 'conflict';
		} finally {
			editBusy = false;
		}
	}

	async function reloadAfterConflict() {
		if (!editingNote) return;
		const draftBody = editingNote.body;
		const targetId = editingNote.id;
		await reloadNotes();
		if (notesRead.state !== 'ready') {
			toast.error("Couldn't reload private notes. Your draft was kept.");
			return;
		}
		const latest = notesRead.data?.notes.find((n) => n.id === targetId && n.mine);
		if (latest) {
			editingNote = {
				id: latest.id,
				body: draftBody,
				revision: latest.revision,
				operationId: newId()
			};
			editError = undefined;
			editConflict = false;
		} else {
			editingNote = null;
			editError = undefined;
			editConflict = false;
			toast.error('This private note was deleted in another session.');
		}
	}

	async function confirmDeleteNote() {
		if (!deleteTarget) return;
		if (!deleteTarget.note.mine || !canWrite) {
			throw new Error(
				stale
					? 'Offline copy — private note edits are paused while read-only.'
					: "You don't have permission to delete this private note."
			);
		}
		await workItemsApi.deleteNote(organizationId, taskId, deleteTarget.note.id, {
			expectedRevision: deleteTarget.note.revision,
			operationId: deleteTarget.operationId
		});
		if (editingNote?.id === deleteTarget.note.id) {
			editingNote = null;
		}
		deleteTarget = null;
		toast.success('Private note deleted.');
		await reloadNotes();
	}

	async function confirmPublishNote() {
		if (!publishTarget) return;
		if (!publishTarget.note.mine || !canWrite) {
			throw new Error(
				stale
					? 'Offline copy — private note edits are paused while read-only.'
					: "You don't have permission to publish this private note."
			);
		}
		await workItemsApi.publishNote(organizationId, taskId, publishTarget.note.id, {
			expectedRevision: publishTarget.note.revision,
			operationId: publishTarget.operationId
		});
		publishTarget = null;
		toast.success('Published a copy as a shared comment. Your private note stays private.');
		onsharedchanged();
	}
</script>

<div
	class="flex min-w-0 flex-col gap-4 rounded-lg bg-surface-sunken p-4 ring-1 ring-brand/25"
	data-testid="private-notes-pane"
>
	<!-- Plain-language visibility banner -->
	<div
		class="flex items-start gap-2.5 rounded-lg border-l-2 border-brand bg-brand/10 px-3.5 py-2.5"
		data-testid="private-notes-audience"
	>
		<WorkIcon name="lock" class="mt-0.5 size-4 text-brand-text" />
		<div class="flex min-w-0 flex-col gap-0.5">
			<p class="text-xs/5 font-semibold text-fg [overflow-wrap:anywhere]">
				{PRIVATE_NOTES_VISIBILITY_NOTICE}
			</p>
			<p class="text-xs/5 text-fg-secondary [overflow-wrap:anywhere]">
				{PRIVATE_NOTES_VISIBILITY_DETAIL}
			</p>
		</div>
	</div>

	{#if notesRead.state === 'loading'}
		<p class="text-xs/5 text-fg-muted" role="status">Loading private notes…</p>
	{:else if notesRead.state === 'unavailable'}
		<p class="text-xs/5 text-fg-muted" data-testid="private-notes-unavailable">
			Private notes aren't served on this server yet.
		</p>
	{:else if notesRead.state === 'unsupported'}
		<div class="flex flex-col gap-0.5" data-testid="private-notes-unsupported">
			<p class="text-xs/5 font-medium text-fg">Not available in this version</p>
			<p class="text-xs/5 text-fg-muted">
				{notesRead.message || "This version of the office doesn't support private task notes."}
			</p>
		</div>
	{:else if notesRead.state === 'error'}
		<div class="flex flex-col gap-2" role="alert" data-testid="private-notes-error">
			<p class="text-xs/5 text-danger-text">
				Couldn't load private notes. {notesRead.message}
			</p>
			<div>
				<Button
					type="button"
					variant="outline"
					size="sm"
					class={mobileButton}
					disabled={notesRead.reloading}
					onclick={() => reloadNotes()}
				>
					{notesRead.reloading ? 'Retrying…' : 'Retry'}
				</Button>
			</div>
		</div>
	{:else if notesRead.data}
		{#if groups.length > 1}
			<div
				role="group"
				aria-label="Filter private notes by author"
				class="flex flex-wrap items-center gap-1.5"
				data-testid="private-notes-author-filter"
			>
				<button
					type="button"
					aria-pressed={effectiveFilterId === 'all'}
					onclick={() => (selectedAuthorId = 'all')}
					class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {effectiveFilterId ===
					'all'
						? 'bg-primary text-on-emphasis'
						: 'bg-surface text-fg-secondary ring-1 ring-line hover:bg-surface-overlay hover:text-fg'}"
				>
					<span>All authors</span>
					<span class="tabular-nums opacity-80">{allNotes.length}</span>
				</button>
				{#each groups as group (group.author.id)}
					{@const active = effectiveFilterId === group.author.id}
					<button
						type="button"
						aria-pressed={active}
						data-author-filter={group.author.id}
						onclick={() => (selectedAuthorId = group.author.id)}
						class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 {active
							? 'bg-primary text-on-emphasis'
							: 'bg-surface text-fg-secondary ring-1 ring-line hover:bg-surface-overlay hover:text-fg'}"
					>
						<span class="[overflow-wrap:anywhere]">{group.author.name}</span>
						<span class="opacity-80">
							{group.mine ? 'You' : authorKindWord(group.author.kind)}
						</span>
						<span class="tabular-nums opacity-80">{group.notes.length}</span>
					</button>
				{/each}
			</div>
		{/if}

		{#if groups.length === 0}
			<p class="text-sm/6 text-fg-muted" data-testid="private-notes-empty">No private notes yet.</p>
		{:else}
			<div class="flex min-w-0 flex-col gap-3" data-testid="private-notes-list">
				{#each visibleGroups as group (group.author.id)}
					{@const collapsed = isCollapsed(group.author.id)}
					{@const groupHeadingId = `${uid}-author-${group.author.id}`}
					{@const groupPanelId = `${uid}-group-${group.author.id}`}
					<section
						aria-labelledby={groupHeadingId}
						class="flex min-w-0 flex-col gap-3 rounded-lg bg-surface p-3.5 outline-1 -outline-offset-1 {group.mine
							? 'outline-brand/30'
							: 'outline-line'}"
						data-testid="private-note-author-group"
						data-author-id={group.author.id}
						data-author-kind={group.author.kind}
						data-mine={group.mine ? 'true' : 'false'}
					>
						<div class="flex flex-wrap items-center justify-between gap-2">
							<div class="flex min-w-0 flex-wrap items-center gap-2">
								<Avatar name={group.author.name} size="sm" />
								<h4
									id={groupHeadingId}
									class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]"
								>
									{group.author.name}
								</h4>
								<span
									class="rounded bg-surface-overlay px-1.5 py-0.5 text-xs/5 font-medium text-fg-secondary"
									data-testid="private-note-author-kind"
								>
									{authorKindWord(group.author.kind)}
								</span>
								{#if group.author.former}
									<span class="rounded bg-surface-overlay px-1.5 py-0.5 text-xs/5 text-fg-muted">
										Former
									</span>
								{/if}
								{#if group.mine}
									<span
										class="rounded bg-brand/15 px-1.5 py-0.5 text-xs/5 font-medium text-brand-text-strong"
									>
										You
									</span>
								{:else}
									<span
										class="inline-flex items-center gap-1 rounded bg-surface-raised px-1.5 py-0.5 text-xs/5 text-fg-muted ring-1 ring-line"
										data-testid="private-note-readonly-badge"
									>
										<WorkIcon name="lock" class="size-3" />
										<span>Read-only</span>
									</span>
								{/if}
							</div>

							<button
								type="button"
								aria-expanded={!collapsed}
								aria-controls={groupPanelId}
								onclick={() => toggleAuthorGroup(group.author.id)}
								class="inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs/5 text-fg-muted hover:bg-surface-overlay hover:text-fg focus-visible:outline-2 focus-visible:outline-focus max-[639px]:min-h-11"
							>
								<span class="sr-only">{group.author.name}:</span>
								<span class="tabular-nums">{privateNoteCountLabel(group.notes.length)}</span>
								<WorkIcon name={collapsed ? 'down' : 'up'} class="size-3.5" />
							</button>
						</div>

						{#if !collapsed}
							<ul
								id={groupPanelId}
								role="list"
								class="flex min-w-0 flex-col gap-2.5 border-t border-line-subtle pt-3"
							>
								{#each group.notes as note (note.id)}
									{@const edited = isNoteEdited(note)}
									<li
										class="flex min-w-0 flex-col gap-2 rounded-md bg-surface-raised p-3 outline-1 -outline-offset-1 outline-line-subtle"
										data-testid="private-note"
										data-note-id={note.id}
										data-author-id={note.author.id}
										data-author-kind={note.author.kind}
										data-mine={note.mine ? 'true' : 'false'}
									>
										<div class="flex flex-wrap items-baseline justify-between gap-2 text-xs/5">
											<div class="flex flex-wrap items-center gap-1.5 text-fg-muted">
												<time datetime={note.createdAt}>{when(note.createdAt)}</time>
												{#if edited}
													<span data-testid="private-note-edited">
														· Edited <time datetime={note.updatedAt}>{when(note.updatedAt)}</time>
													</span>
												{/if}
											</div>
											{#if !note.mine}
												<span class="text-fg-muted">Read-only</span>
											{/if}
										</div>

										{#if editingNote?.id === note.id && note.mine && canWrite}
											<form class="flex flex-col gap-2" onsubmit={saveEditedNote} novalidate>
												<label for="{uid}-edit-{note.id}" class="sr-only">
													Edit private note
												</label>
												<textarea
													id="{uid}-edit-{note.id}"
													rows={3}
													bind:value={editingNote.body}
													oninput={() => {
														if (editingNote) editingNote.operationId = newId();
														if (editError && !editConflict) editError = undefined;
													}}
													disabled={editBusy || notesRead.reloading}
													aria-invalid={editError ? 'true' : undefined}
													aria-describedby={editError ? `${uid}-edit-err-${note.id}` : undefined}
													class={textareaClass}></textarea>
												{#if editError}
													<div
														id="{uid}-edit-err-{note.id}"
														class="flex flex-wrap items-center justify-between gap-2"
														role="alert"
													>
														<p class="text-xs/5 text-danger-text">{editError}</p>
														{#if editConflict}
															<Button
																type="button"
																variant="outline"
																size="sm"
																class={mobileButton}
																disabled={notesRead.reloading}
																onclick={reloadAfterConflict}
															>
																{notesRead.reloading ? 'Reloading…' : 'Reload latest revision'}
															</Button>
														{/if}
													</div>
												{/if}
												<div class="flex flex-wrap justify-end gap-2">
													<Button
														type="button"
														variant="outline"
														size="sm"
														class={mobileButton}
														disabled={editBusy || notesRead.reloading}
														onclick={cancelEdit}
													>
														Cancel
													</Button>
													<Button
														type="submit"
														size="sm"
														class={mobileButton}
														disabled={editBusy || notesRead.reloading}
													>
														{editBusy ? 'Saving…' : 'Save note'}
													</Button>
												</div>
											</form>
										{:else}
											<p
												class="whitespace-pre-wrap text-sm/6 text-fg-body [overflow-wrap:anywhere]"
											>
												{note.body}
											</p>
											{#if note.mine && canWrite}
												<div class="flex flex-wrap gap-2 pt-1">
													<Button
														type="button"
														variant="outline"
														size="sm"
														class={mobileButton}
														onclick={() => startEdit(note)}
													>
														Edit
													</Button>
													<Button
														type="button"
														variant="outline"
														size="sm"
														class={mobileButton}
														onclick={() => (publishTarget = { note, operationId: newId() })}
													>
														Publish as shared comment…
													</Button>
													<Button
														type="button"
														variant="outline"
														size="sm"
														class={mobileButton}
														onclick={() => (deleteTarget = { note, operationId: newId() })}
													>
														Delete
													</Button>
												</div>
											{/if}
										{/if}
									</li>
								{/each}
							</ul>
						{/if}
					</section>
				{/each}
			</div>
		{/if}

		{#if canWrite}
			<form
				class="flex flex-col gap-2 border-t border-line-subtle pt-3"
				onsubmit={addPrivateNote}
				novalidate
				data-testid="private-note-composer"
			>
				<label for="{uid}-note" class="text-xs/5 font-medium text-fg">
					Add a private note (hidden from agents)
				</label>
				<textarea
					id="{uid}-note"
					rows={3}
					bind:value={noteBody}
					oninput={() => {
						if (noteError) noteError = undefined;
						noteOpId = null;
					}}
					disabled={noteBusy}
					aria-invalid={noteError ? 'true' : undefined}
					aria-describedby={noteError ? `${uid}-note-err` : undefined}
					placeholder="Working notes visible to you and people who can read this task, hidden from agents…"
					class={textareaClass}></textarea>
				{#if noteError}
					<p id="{uid}-note-err" class={errorClass} role="alert">{noteError}</p>
				{/if}
				<div class="flex justify-end">
					<Button
						type="submit"
						variant="outline"
						size="sm"
						class={mobileButton}
						disabled={noteBusy}
					>
						{noteBusy ? 'Saving…' : 'Save private note'}
					</Button>
				</div>
			</form>
		{:else if stale}
			<p class="text-xs/5 text-fg-muted" data-testid="private-notes-stale-note">
				Offline copy — private note edits are paused while read-only.
			</p>
		{:else}
			<p class="text-xs/5 text-fg-muted" data-testid="private-notes-readonly-reason">
				You can read private notes here, but this account cannot write new private notes.
			</p>
		{/if}
	{/if}
</div>

<!-- Explicit publish preview: private original stays private -->
<ConfirmDialog
	bind:open={
		() => publishTarget !== null,
		(next) => {
			if (!next) publishTarget = null;
		}
	}
	title="Publish note as a shared comment?"
	description="This posts a copy to Shared discussion where everyone who can read this work item (including agents) can see it. Your private note stays private."
	confirmLabel="Publish shared comment"
	busyLabel="Publishing…"
	onconfirm={confirmPublishNote}
>
	{#snippet body()}
		{#if publishTarget}
			<blockquote
				class="rounded-md border border-line bg-surface-sunken p-3 whitespace-pre-wrap text-sm/6 text-fg-body [overflow-wrap:anywhere]"
			>
				{publishTarget.note.body}
			</blockquote>
		{/if}
	{/snippet}
</ConfirmDialog>

<ConfirmDialog
	bind:open={
		() => deleteTarget !== null,
		(next) => {
			if (!next) deleteTarget = null;
		}
	}
	title="Delete this private note?"
	description="This removes the note from your private notes on this work item."
	confirmLabel="Delete note"
	busyLabel="Deleting…"
	destructive
	onconfirm={confirmDeleteNote}
/>
