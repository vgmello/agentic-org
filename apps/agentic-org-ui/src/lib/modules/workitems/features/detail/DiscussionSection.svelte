<!--
  @component
  Distinct Shared discussion (public comments) and Private task notes tabs on /work/[ref]
  (work-items-ux §7.1, feature-requests-ux §3, ADR-0031, PN-01..06).
  Private notes render in their own visually distinct PrivateNotesPane component backed by the
  separate private-note store.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import { errorClass, mobileButton, textareaClass } from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import {
		cleanCommentDisplayBody,
		formatIdeationCommentBody,
		splitIdeationComments
	} from '../../ideation';
	import { when } from '../../present';
	import type { Comment } from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';
	import PrivateNotesPane from './PrivateNotesPane.svelte';

	let {
		organizationId,
		taskId,
		taskRef = '',
		isDraftIdeation = false,
		comments,
		canComment = true,
		stale = false,
		onsharedchanged
	}: {
		organizationId: string;
		taskId: string;
		taskRef?: string;
		isDraftIdeation?: boolean;
		comments: Comment[];
		canComment?: boolean;
		stale?: boolean;
		onsharedchanged: () => void;
	} = $props();

	const uid = $props.id();
	const panes = ['shared', 'private'] as const;
	type Pane = (typeof panes)[number];
	let activePane = $state<Pane>('shared');
	let showIdeation = $state(false);

	const split = $derived(splitIdeationComments(comments));
	const visibleComments = $derived(isDraftIdeation ? comments : split.execution);

	// Shared discussion state
	let commentBody = $state('');
	let commentBusy = $state(false);
	let commentError = $state<string | undefined>();

	function commentKindLabel(kind: Comment['kind'], isIdeation = false): string {
		if (isIdeation) return 'Ideation';
		if (kind === 'question') return 'Question';
		if (kind === 'answer') return 'Answer';
		return 'Shared comment';
	}

	function handleTabKey(event: KeyboardEvent, index: number) {
		if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
		let next: number | undefined;
		if (event.key === 'ArrowRight') next = (index + 1) % panes.length;
		else if (event.key === 'ArrowLeft') next = (index - 1 + panes.length) % panes.length;
		else if (event.key === 'Home') next = 0;
		else if (event.key === 'End') next = panes.length - 1;
		if (next === undefined) return;
		event.preventDefault();
		const targetPane = panes[next];
		activePane = targetPane;
		(event.currentTarget as HTMLElement | null)?.parentElement
			?.querySelector<HTMLButtonElement>(`#${CSS.escape(`${uid}-tab-${targetPane}`)}`)
			?.focus();
	}

	async function postComment(event: SubmitEvent) {
		event.preventDefault();
		if (commentBusy || stale || !canComment) return;
		const body = commentBody.trim();
		if (!body) {
			commentError = 'Write a comment first.';
			return;
		}
		commentBusy = true;
		commentError = undefined;
		try {
			const payload = isDraftIdeation ? formatIdeationCommentBody(body) : body;
			await workItemsApi.comment(organizationId, taskId, payload);
			commentBody = '';
			toast.success(isDraftIdeation ? 'Ideation comment posted.' : 'Shared comment posted.');
			onsharedchanged();
		} catch (e) {
			commentError = toAppError(e).message;
		} finally {
			commentBusy = false;
		}
	}
</script>

<section
	aria-labelledby="{uid}-heading"
	class="flex min-w-0 flex-col gap-4 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
	data-testid="discussion-section"
>
	<div class="flex flex-wrap items-center justify-between gap-3 border-b border-line-subtle pb-3">
		<h3 id="{uid}-heading" class="text-sm/6 font-semibold text-fg">Discussion and notes</h3>
		<div role="tablist" aria-label="Discussion and private notes" class="flex flex-wrap gap-1.5">
			<button
				id="{uid}-tab-shared"
				type="button"
				role="tab"
				aria-selected={activePane === 'shared'}
				aria-controls="{uid}-panel"
				tabindex={activePane === 'shared' ? 0 : -1}
				onclick={() => (activePane = 'shared')}
				onkeydown={(e) => handleTabKey(e, 0)}
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
				tabindex={activePane === 'private' ? 0 : -1}
				onclick={() => (activePane = 'private')}
				onkeydown={(e) => handleTabKey(e, 1)}
				class="inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs/5 font-medium focus-visible:outline-2 focus-visible:outline-focus max-[639px]:min-h-11 {activePane ===
				'private'
					? 'bg-surface-sunken text-fg ring-1 ring-brand/40'
					: 'text-fg-secondary hover:text-fg'}"
			>
				<WorkIcon name="lock" class="size-3.5 text-brand-text" />
				<span>Private notes</span>
			</button>
		</div>
	</div>

	<div
		id="{uid}-panel"
		role="tabpanel"
		aria-labelledby="{uid}-tab-{activePane}"
		class="flex min-w-0 flex-col gap-4"
	>
		{#if activePane === 'shared'}
			<p class="text-xs/5 text-fg-muted">
				{#if isDraftIdeation}
					Ideation comments — folded out of the default execution stream once this draft is
					published.
				{:else}
					Execution comments — visible to everyone who can read this work item, including agents.
				{/if}
			</p>

			{#if !isDraftIdeation && split.ideation.length > 0}
				<div
					class="flex flex-col gap-2 rounded-lg bg-surface p-3 outline-1 -outline-offset-1 outline-line-subtle"
					data-testid="ideation-archive-section"
				>
					<div class="flex flex-wrap items-center justify-between gap-2">
						<button
							type="button"
							aria-expanded={showIdeation}
							onclick={() => (showIdeation = !showIdeation)}
							class="inline-flex items-center gap-1.5 text-xs/5 font-semibold text-fg-secondary hover:text-fg"
							data-testid="toggle-ideation-comments"
						>
							<span
								>{showIdeation ? 'Hide ideation' : `Show ideation (${split.ideation.length})`}</span
							>
						</button>
						{#if taskRef}
							<code class="font-mono text-[11px]/4 text-fg-muted">
								agent-os task view {taskRef} --include-ideation
							</code>
						{/if}
					</div>
					{#if showIdeation}
						<ul role="list" class="flex flex-col gap-2 pt-1" data-testid="ideation-comments">
							{#each split.ideation as ic (ic.id)}
								<li
									class="flex flex-col gap-1 rounded-md bg-surface-raised p-2.5 outline-1 -outline-offset-1 outline-line-subtle"
								>
									<div class="flex flex-wrap items-baseline justify-between gap-2 text-xs/5">
										<div class="flex flex-wrap items-center gap-1.5">
											<span class="font-semibold text-fg">{ic.author.name}</span>
											<span class="rounded bg-surface-overlay px-1.5 py-0.5 text-fg-muted">
												{commentKindLabel(ic.kind, true)}
											</span>
										</div>
										<time class="text-fg-muted" datetime={ic.at}>{when(ic.at)}</time>
									</div>
									<p
										class="whitespace-pre-wrap text-sm/6 text-fg-secondary [overflow-wrap:anywhere]"
									>
										{cleanCommentDisplayBody(ic.body)}
									</p>
								</li>
							{/each}
						</ul>
					{/if}
				</div>
			{/if}

			{#if visibleComments.length === 0}
				<p class="text-sm/6 text-fg-muted">
					{isDraftIdeation ? 'No ideation comments yet.' : 'No shared comments yet.'}
				</p>
			{:else}
				<ul role="list" class="flex flex-col gap-3" data-testid="shared-comments">
					{#each visibleComments as c (c.id)}
						<li
							class="flex flex-col gap-1 rounded-lg bg-surface p-3 outline-1 -outline-offset-1 outline-line-subtle"
						>
							<div class="flex flex-wrap items-baseline justify-between gap-2 text-xs/5">
								<div class="flex flex-wrap items-center gap-1.5">
									<span class="font-semibold text-fg">{c.author.name}</span>
									<span class="text-fg-muted">
										{c.author.kind === 'agent' ? 'Agent' : 'Person'}
									</span>
									<span class="rounded bg-surface-overlay px-1.5 py-0.5 text-fg-secondary">
										{commentKindLabel(c.kind, isDraftIdeation)}
									</span>
								</div>
								<time class="text-fg-muted" datetime={c.at}>{when(c.at)}</time>
							</div>
							<p class="whitespace-pre-wrap text-sm/6 text-fg-body [overflow-wrap:anywhere]">
								{cleanCommentDisplayBody(c.body)}
							</p>
						</li>
					{/each}
				</ul>
			{/if}

			{#if !stale && canComment}
				<form class="flex flex-col gap-2" onsubmit={postComment} novalidate>
					<label for="{uid}-comment" class="text-xs/5 font-medium text-fg">
						{isDraftIdeation ? 'Add an ideation comment' : 'Add a shared comment'}
					</label>
					<textarea
						id="{uid}-comment"
						rows={3}
						bind:value={commentBody}
						disabled={commentBusy}
						aria-invalid={commentError ? 'true' : undefined}
						aria-describedby={commentError ? `${uid}-comment-err` : undefined}
						placeholder={isDraftIdeation
							? 'Discuss scope, tradeoffs, or open questions before publishing…'
							: 'Share an update, question or decision…'}
						class={textareaClass}></textarea>
					{#if commentError}
						<p id="{uid}-comment-err" class={errorClass} role="alert">{commentError}</p>
					{/if}
					<div class="flex justify-end">
						<Button type="submit" size="sm" class={mobileButton} disabled={commentBusy}>
							{commentBusy
								? 'Posting…'
								: isDraftIdeation
									? 'Post ideation comment'
									: 'Post shared comment'}
						</Button>
					</div>
				</form>
			{/if}
		{:else}
			<PrivateNotesPane {organizationId} {taskId} {stale} {onsharedchanged} />
		{/if}
	</div>
</section>
