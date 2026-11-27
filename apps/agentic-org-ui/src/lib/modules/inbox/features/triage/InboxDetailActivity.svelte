<script lang="ts">
	import { Tooltip } from 'bits-ui';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Clock from '@lucide/svelte/icons/clock';
	import Inbox from '@lucide/svelte/icons/inbox';
	import Send from '@lucide/svelte/icons/send';
	import {
		MAX_INBOX_COMMENT_LENGTH,
		type InboxCommentEntry,
		type TriageItem
	} from './triage-model';

	let {
		item,
		comments,
		commentDraft,
		commentStatus,
		snoozeReturnFull,
		commentInputEl = $bindable(null),
		onCommentDraftChange,
		onCommentSubmit
	}: {
		item: TriageItem;
		comments: readonly InboxCommentEntry[];
		commentDraft: string;
		commentStatus: string;
		snoozeReturnFull?: string;
		commentInputEl?: HTMLTextAreaElement | null;
		onCommentDraftChange: (value: string) => void;
		onCommentSubmit: (event: SubmitEvent) => void;
	} = $props();

	function formatCommentTime(at: number): string {
		return new Date(at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
	}
</script>

<section class="triage-activity" aria-labelledby="triage-activity-title">
	<h3 id="triage-activity-title">
		Activity <span class="font-normal text-fg-muted">(notes saved on this device only)</span>
	</h3>

	<form class="triage-comment-form" onsubmit={onCommentSubmit}>
		<label class="sr-only" for="triage-comment">Add a device-only note on this request</label>
		<textarea
			bind:this={commentInputEl}
			id="triage-comment"
			name="comment"
			rows="2"
			maxlength={MAX_INBOX_COMMENT_LENGTH}
			placeholder="Add a local note (saved on this device only)…"
			value={commentDraft}
			oninput={(e) => onCommentDraftChange(e.currentTarget.value)}
			onkeydown={(e) => {
				if (!e.isComposing && e.keyCode !== 229 && e.key === 'Escape') {
					e.preventDefault();
					document.getElementById('shell-page-title')?.focus({ preventScroll: true });
				}
			}}
			required></textarea>
		<Tooltip.Provider delayDuration={150}>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button {...props} type="submit" aria-label="Send comment" class="triage-send">
							<Send class="size-4" aria-hidden="true" />
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						side="top"
						sideOffset={6}
						class="z-50 rounded-md bg-surface-overlay px-2.5 py-1 text-xs/5 font-medium text-fg shadow-md ring-1 ring-line"
						>Send comment</Tooltip.Content
					>
				</Tooltip.Portal>
			</Tooltip.Root>
		</Tooltip.Provider>
	</form>
	<p id="triage-comment-status" class="sr-only" role="status">{commentStatus}</p>

	{#if comments.length > 0}
		<section class="triage-comments" aria-labelledby="triage-comments-title">
			<h4 id="triage-comments-title">Comments</h4>
			<ol class="triage-comment-list" role="list" aria-labelledby="triage-comments-title">
				{#each comments as entry (entry.id)}
					<li>
						<span class="triage-comment-av" aria-hidden="true">Y</span>
						<div>
							<p>
								<strong>You</strong>
								<time class="triage-event-time" datetime={new Date(entry.at).toISOString()}>
									{formatCommentTime(entry.at)}
								</time>
							</p>
							<p class="triage-comment-text">{entry.text}</p>
						</div>
					</li>
				{/each}
			</ol>
		</section>
	{/if}

	<div class="triage-history">
		<h4 id="triage-history-title">Request history</h4>
		<ol class="triage-activity-log" role="list" aria-labelledby="triage-history-title">
			<li>
				<span class="triage-event-marker" aria-hidden="true">
					<Inbox class="size-4" />
				</span>
				<p>
					<strong>{item.actorName}</strong> opened this request
					<span class="triage-event-time">{item.createdAgeLabel}</span>
				</p>
			</li>

			{#if item.state === 'open'}
				<li>
					<span class="triage-event-marker" aria-hidden="true">
						<Clock class="size-4" />
					</span>
					<p>
						{item.category === 'approval' ? 'Waiting for approval' : 'Waiting for your input'}
					</p>
				</li>
			{:else if item.state === 'snoozed'}
				<li>
					<span class="triage-event-marker" aria-hidden="true">
						<Clock class="size-4" />
					</span>
					<p class="[overflow-wrap:anywhere]">
						Snoozed until {snoozeReturnFull ?? 'later'}{item.snoozed?.note
							? ` · ${item.snoozed.note}`
							: ''}
					</p>
				</li>
			{/if}

			{#each item.auditChecks as chk, idx (`${idx}-${chk}`)}
				<li>
					<span class="triage-event-marker" aria-hidden="true">
						<CheckCircle2 class="size-4" />
					</span>
					<p><span class="sr-only">Audit entry. </span>{chk}</p>
				</li>
			{/each}

			{#if item.state === 'handled' && item.handled}
				<li>
					<span class="triage-event-marker" aria-hidden="true">
						<CheckCircle2 class="size-4" />
					</span>
					<p>
						{item.handled.outcome}{item.handled.by ? ` · by ${item.handled.by}` : ''}
						<time class="triage-event-time" datetime={item.handled.atIso}>
							{item.handled.atLabel}
						</time>
					</p>
				</li>
			{/if}
		</ol>
	</div>
</section>

<style>
	.triage-activity {
		padding-block-start: 24px;
		border-top: 1px solid var(--exec-border);
	}
	.triage-activity > h3 {
		margin-block-end: 14px;
		color: var(--exec-ink);
		font-size: 1.125rem;
		font-weight: 600;
	}
	.triage-activity h4 {
		margin-block-end: 8px;
		color: var(--exec-ink);
		font-size: 1rem;
		font-weight: 600;
	}
	.triage-comments,
	.triage-history {
		margin-block-start: 24px;
	}
	.triage-comment-form {
		display: grid;
		justify-items: end;
		gap: 6px;
		padding: 10px 12px;
		border: 1px solid var(--exec-border-hi);
		border-radius: var(--exec-inset-radius);
		background: var(--exec-well);
	}
	.triage-comment-form:has(textarea:focus-visible) {
		border-color: var(--focus);
		outline: 2px solid var(--focus);
		outline-offset: -1px;
	}
	.triage-comment-form textarea {
		width: 100%;
		min-width: 0;
		min-height: calc(2lh + 4px);
		resize: vertical;
		padding: 2px 0;
		border: 0;
		outline: none;
		background: transparent;
		-webkit-backdrop-filter: none;
		backdrop-filter: none;
		box-shadow: none;
		color: var(--exec-ink);
		font: inherit;
	}
	@supports (field-sizing: content) {
		.triage-comment-form textarea {
			field-sizing: content;
			resize: none;
		}
	}
	.triage-comment-form textarea::placeholder {
		color: var(--exec-muted);
		opacity: 1;
	}
	.triage-send {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 36px;
		min-height: 36px;
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.triage-send:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
	}
	.triage-activity-log li,
	.triage-comment-list li {
		position: relative;
		display: grid;
		grid-template-columns: 24px minmax(0, 1fr);
		align-items: start;
		gap: 10px;
		min-height: 42px;
		padding-block: 8px;
		color: var(--exec-body);
		font-size: 0.875rem;
		overflow-wrap: anywhere;
	}
	.triage-activity-log li:not(:last-child)::after {
		content: '';
		position: absolute;
		inset-block: 33px 1px;
		inset-inline-start: 11px;
		width: 1px;
		background: var(--exec-border-hi);
	}
	.triage-event-marker {
		display: grid;
		place-items: center;
		width: 24px;
		height: 24px;
		color: var(--exec-muted);
	}
	.triage-comment-av {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 24px;
		height: 24px;
		border-radius: 9999px;
		background: var(--exec-avatar-bg);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		font-size: 0.6875rem;
		font-weight: 700;
	}
	.triage-activity strong {
		color: var(--exec-ink);
		font-weight: 600;
	}
	.triage-event-time {
		margin-inline-start: 8px;
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.triage-comment-text {
		margin-block-start: 4px;
		white-space: pre-wrap;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.triage-send {
			width: 44px;
			min-width: 44px;
			min-height: 44px;
		}
	}
	.triage-send:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	@media (forced-colors: active) {
		.triage-send {
			border: 1px solid ButtonText;
		}
		.triage-comment-form {
			border: 1px solid CanvasText;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.triage-send {
			transition: none;
		}
	}
</style>
