<!--
  @component
  Distinct Shared discussion and Private task notes tabs (work-items-ux §7.1, feature-requests-ux §3, PN-01..06).
  Private notes use a separate Resource explicitly handling all five states (`loading`, `unavailable`, `unsupported`,
  `error`, `ready`). Publishing shows a preview and asks for audience confirmation while keeping the private note.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import { Resource } from '$lib/components/resource.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { errorClass, mobileButton, textareaClass } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import { when } from '../../present';
	import type { Comment, PrivateNote } from '../../types';

	let {
		organizationId,
		taskId,
		comments,
		stale = false,
		onsharedchanged
	}: {
		organizationId: string;
		taskId: string;
		comments: Comment[];
		stale?: boolean;
		onsharedchanged: () => void;
	} = $props();

	const uid = $props.id();
	let activePane = $state<'shared' | 'private'>('shared');

	// Shared discussion state
	let commentBody = $state('');
	let commentBusy = $state(false);
	let commentError = $state<string | undefined>();

	// Separate private notes store (PN-01..06)
	const notesRead = new Resource<PrivateNote[]>(() => workItemsApi.notes(organizationId, taskId));
	let noteBody = $state('');
	let noteBusy = $state(false);
	let noteError = $state<string | undefined>();
	let editingNote = $state<{ id: string; body: string; revision: number } | null>(null);
	let editBusy = $state(false);
	let editError = $state<string | undefined>();
	let publishTarget = $state<PrivateNote | null>(null);
	let deleteTarget = $state<PrivateNote | null>(null);

	function commentKindLabel(kind: Comment['kind']): string {
		if (kind === 'question') return 'Question';
		if (kind === 'answer') return 'Answer';
		return 'Shared comment';
	}

	async function postComment(event: SubmitEvent) {
		event.preventDefault();
		if (commentBusy || stale) return;
		const body = commentBody.trim();
		if (!body) {
			commentError = 'Write a comment first.';
			return;
		}
		commentBusy = true;
		commentError = undefined;
		try {
			await workItemsApi.comment(organizationId, taskId, body);
			commentBody = '';
			toast.success('Shared comment posted.');
			onsharedchanged();
		} catch (e) {
			commentError = toAppError(e).message;
		} finally {
			commentBusy = false;
		}
	}

	async function addPrivateNote(event: SubmitEvent) {
		event.preventDefault();
		if (noteBusy || stale) return;
		const body = noteBody.trim();
		if (!body) {
			noteError = 'Write a note first.';
			return;
		}
		noteBusy = true;
		noteError = undefined;
		try {
			await workItemsApi.addNote(organizationId, taskId, body);
			noteBody = '';
			toast.success('Private note saved.');
			await notesRead.reload();
		} catch (e) {
			// Private save failure never falls back to a shared save (PN-04).
			noteError = toAppError(e).message;
		} finally {
			noteBusy = false;
		}
	}

	async function saveEditedNote(event: SubmitEvent) {
		event.preventDefault();
		if (!editingNote || editBusy || stale) return;
		const body = editingNote.body.trim();
		if (!body) {
			editError = 'Note cannot be empty.';
			return;
		}
		editBusy = true;
		editError = undefined;
		try {
			await workItemsApi.editNote(organizationId, taskId, editingNote.id, {
				body,
				expectedRevision: editingNote.revision
			});
			editingNote = null;
			toast.success('Private note updated.');
			await notesRead.reload();
		} catch (e) {
			editError = toAppError(e).message;
		} finally {
			editBusy = false;
		}
	}

	async function confirmDeleteNote() {
		if (!deleteTarget) return;
		await workItemsApi.deleteNote(organizationId, taskId, deleteTarget.id);
		deleteTarget = null;
		toast.success('Private note deleted.');
		await notesRead.reload();
	}

	async function confirmPublishNote() {
		if (!publishTarget) return;
		await workItemsApi.publishNote(organizationId, taskId, publishTarget.id);
		publishTarget = null;
		toast.success('Published a copy as a shared comment. The private original stays private.');
		onsharedchanged();
	}
</script>

<section
	aria-labelledby="{uid}-heading"
	class="flex flex-col gap-4 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="discussion-section"
>
	<div class="flex flex-wrap items-center justify-between gap-3 border-b border-line-subtle pb-3">
		<h3 id="{uid}-heading" class="text-sm/6 font-semibold text-fg">Discussion and notes</h3>
		<div role="tablist" aria-label="Discussion and private notes" class="flex gap-1">
			<button
				id="{uid}-tab-shared"
				type="button"
				role="tab"
				aria-selected={activePane === 'shared'}
				aria-controls="{uid}-panel"
				onclick={() => (activePane = 'shared')}
				class="rounded-md px-3 py-1.5 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus max-[639px]:min-h-11 {activePane ===
				'shared'
					? 'bg-surface-overlay text-fg'
					: 'text-fg-secondary hover:text-fg'}"
			>
				Shared discussion ({comments.length})
			</button>
			<button
				id="{uid}-tab-private"
				type="button"
				role="tab"
				aria-selected={activePane === 'private'}
				aria-controls="{uid}-panel"
				onclick={() => (activePane = 'private')}
				class="rounded-md px-3 py-1.5 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus max-[639px]:min-h-11 {activePane ===
				'private'
					? 'bg-surface-overlay text-fg'
					: 'text-fg-secondary hover:text-fg'}"
			>
				Private notes
			</button>
		</div>
	</div>

	<div
		id="{uid}-panel"
		role="tabpanel"
		aria-labelledby="{uid}-tab-{activePane}"
		class="flex flex-col gap-4"
	>
		{#if activePane === 'shared'}
			<p class="text-xs/5 text-fg-muted">
				Visible to everyone who can read this work item, including agents.
			</p>

			{#if comments.length === 0}
				<p class="text-sm/6 text-fg-muted">No shared comments yet.</p>
			{:else}
				<ul role="list" class="flex flex-col gap-3" data-testid="shared-comments">
					{#each comments as c (c.id)}
						<li
							class="flex flex-col gap-1 rounded-lg bg-surface p-3 outline-1 -outline-offset-1 outline-line-subtle"
						>
							<div class="flex flex-wrap items-baseline justify-between gap-2 text-xs/5">
								<div class="flex flex-wrap items-center gap-1.5">
									<span class="font-semibold text-fg">{c.author.name}</span>
									<span class="text-fg-muted">{c.author.kind}</span>
									<span class="rounded bg-surface-overlay px-1.5 py-0.5 text-fg-secondary">
										{commentKindLabel(c.kind)}
									</span>
								</div>
								<time class="text-fg-muted" datetime={c.at}>{when(c.at)}</time>
							</div>
							<p class="whitespace-pre-wrap text-sm/6 text-fg-body [overflow-wrap:anywhere]">
								{c.body}
							</p>
						</li>
					{/each}
				</ul>
			{/if}

			{#if !stale}
				<form class="flex flex-col gap-2" onsubmit={postComment} novalidate>
					<label for="{uid}-comment" class="text-xs/5 font-medium text-fg">
						Add a shared comment
					</label>
					<textarea
						id="{uid}-comment"
						rows={3}
						bind:value={commentBody}
						disabled={commentBusy}
						aria-invalid={commentError ? 'true' : undefined}
						aria-describedby={commentError ? `${uid}-comment-err` : undefined}
						placeholder="Share an update, question or decision…"
						class={textareaClass}></textarea>
					{#if commentError}
						<p id="{uid}-comment-err" class={errorClass} role="alert">{commentError}</p>
					{/if}
					<div class="flex justify-end">
						<Button type="submit" size="sm" class={mobileButton} disabled={commentBusy}>
							{commentBusy ? 'Posting…' : 'Post shared comment'}
						</Button>
					</div>
				</form>
			{/if}
		{:else}
			<!-- Private task notes (separate store, never mixed into shared counts/history) -->
			<p class="text-xs/5 text-fg-muted" data-testid="private-notes-audience">
				Stored separately from shared history. Only the author edits or deletes a note. People who
				can read this work item can view notes read-only; agents never see another author's notes.
			</p>

			{#if notesRead.state === 'loading'}
				<p class="text-xs/5 text-fg-muted" role="status">Loading private notes…</p>
			{:else if notesRead.state === 'unavailable'}
				<p class="text-xs/5 text-fg-muted">Private notes aren't served on this server yet.</p>
			{:else if notesRead.state === 'unsupported'}
				<div data-testid="private-notes-unsupported">
					<p class="text-xs/5 font-medium text-fg">Not available in this version</p>
					<p class="mt-0.5 text-xs/5 text-fg-muted">
						{notesRead.message || "This version of the office doesn't support private task notes."}
					</p>
				</div>
			{:else if notesRead.state === 'error'}
				<div class="flex flex-col gap-2" role="alert">
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
							onclick={() => notesRead.reload()}
						>
							{notesRead.reloading ? 'Retrying…' : 'Retry'}
						</Button>
					</div>
				</div>
			{:else if notesRead.data}
				{#if notesRead.data.length === 0}
					<p class="text-sm/6 text-fg-muted">No private notes yet.</p>
				{:else}
					<ul role="list" class="flex flex-col gap-3" data-testid="private-notes-list">
						{#each notesRead.data as note (note.id)}
							<li
								class="flex flex-col gap-2 rounded-lg bg-surface p-3 outline-1 -outline-offset-1 outline-line-subtle"
								data-testid="private-note"
								data-mine={note.mine ? 'true' : 'false'}
							>
								<div class="flex flex-wrap items-baseline justify-between gap-2 text-xs/5">
									<div class="flex items-center gap-1.5">
										<span class="font-semibold text-fg">{note.author.name}</span>
										<span class="text-fg-muted">{note.author.kind}</span>
										{#if !note.mine}
											<span class="rounded bg-surface-overlay px-1.5 py-0.5 text-fg-muted">
												Read-only
											</span>
										{/if}
									</div>
									<time class="text-fg-muted" datetime={note.at}>{when(note.at)}</time>
								</div>

								{#if editingNote?.id === note.id}
									<form class="flex flex-col gap-2" onsubmit={saveEditedNote} novalidate>
										<textarea
											rows={3}
											bind:value={editingNote.body}
											disabled={editBusy}
											class={textareaClass}></textarea>
										{#if editError}
											<p class={errorClass} role="alert">{editError}</p>
										{/if}
										<div class="flex justify-end gap-2">
											<Button
												type="button"
												variant="outline"
												size="sm"
												class={mobileButton}
												disabled={editBusy}
												onclick={() => ((editingNote = null), (editError = undefined))}
												>Cancel</Button
											>
											<Button type="submit" size="sm" class={mobileButton} disabled={editBusy}>
												{editBusy ? 'Saving…' : 'Save note'}
											</Button>
										</div>
									</form>
								{:else}
									<p class="whitespace-pre-wrap text-sm/6 text-fg-body [overflow-wrap:anywhere]">
										{note.body}
									</p>
									{#if note.mine && !stale}
										<div class="flex flex-wrap gap-2 pt-1">
											<Button
												type="button"
												variant="outline"
												size="sm"
												class={mobileButton}
												onclick={() =>
													(editingNote = {
														id: note.id,
														body: note.body,
														revision: note.revision
													})}>Edit</Button
											>
											<Button
												type="button"
												variant="outline"
												size="sm"
												class={mobileButton}
												onclick={() => (publishTarget = note)}>Publish as shared comment…</Button
											>
											<Button
												type="button"
												variant="outline"
												size="sm"
												class={mobileButton}
												onclick={() => (deleteTarget = note)}>Delete</Button
											>
										</div>
									{/if}
								{/if}
							</li>
						{/each}
					</ul>
				{/if}

				{#if !stale}
					<form class="flex flex-col gap-2" onsubmit={addPrivateNote} novalidate>
						<label for="{uid}-note" class="text-xs/5 font-medium text-fg">
							Add a private note (hidden from agents)
						</label>
						<textarea
							id="{uid}-note"
							rows={3}
							bind:value={noteBody}
							disabled={noteBusy}
							aria-invalid={noteError ? 'true' : undefined}
							aria-describedby={noteError ? `${uid}-note-err` : undefined}
							placeholder="Working notes only you and people who can read this task see…"
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
				{/if}
			{/if}
		{/if}
	</div>
</section>

<!-- Explicit publish preview (PN-04 / §7.1): private original stays private -->
<ConfirmDialog
	open={publishTarget !== null}
	title="Publish note as a shared comment?"
	description="This posts a copy to Shared discussion where everyone who can read this work item (including agents) can see it. Your private note stays private."
	confirmLabel="Publish shared comment"
	busyLabel="Publishing…"
	onconfirm={confirmPublishNote}
>
	{#snippet body()}
		{#if publishTarget}
			<blockquote
				class="rounded-md border border-line bg-surface-sunken p-3 whitespace-pre-wrap text-sm/6 text-fg-body"
			>
				{publishTarget.body}
			</blockquote>
		{/if}
	{/snippet}
</ConfirmDialog>

<ConfirmDialog
	open={deleteTarget !== null}
	title="Delete this private note?"
	description="This removes the note from your private store."
	confirmLabel="Delete note"
	busyLabel="Deleting…"
	destructive
	onconfirm={confirmDeleteNote}
/>
