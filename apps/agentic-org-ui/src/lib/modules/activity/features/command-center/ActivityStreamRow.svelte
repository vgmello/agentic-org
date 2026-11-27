<script lang="ts">
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ChevronDown from '@lucide/svelte/icons/chevron-down';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import Clock from '@lucide/svelte/icons/clock';
	import Inbox from '@lucide/svelte/icons/inbox';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Play from '@lucide/svelte/icons/play';
	import type { StreamEntry } from './assembly-model';

	let {
		entry,
		when,
		isExpanded,
		onToggleRunLog
	}: {
		entry: StreamEntry;
		when: { text: string; full: string; iso?: string };
		isExpanded: boolean;
		onToggleRunLog: (id: string) => void;
	} = $props();

	const hasLog = $derived(Boolean(entry.run?.log && entry.run.log.length > 0));
</script>

<li class="stream-row">
	<div class="stream-row-avatar" data-tone={entry.tone} aria-hidden="true">
		<span>{entry.actorInitials}</span>
	</div>

	<div class="stream-row-content">
		<div class="stream-row-head">
			<span class="stream-actor">{entry.actorName}</span>
			<span class="stream-kind">
				{#if entry.tone === 'ok'}
					<CheckCircle2 class="size-3.5" style="color: var(--exec-ok);" aria-hidden="true" />
				{:else if entry.tone === 'bad'}
					<AlertTriangle class="size-3.5" style="color: var(--exec-bad);" aria-hidden="true" />
				{:else if entry.tone === 'warn'}
					<AlertTriangle class="size-3.5" style="color: var(--exec-warn);" aria-hidden="true" />
				{:else if entry.kind === 'conversation'}
					<MessageSquare class="size-3.5" style="color: var(--exec-sky);" aria-hidden="true" />
				{:else if entry.kind === 'run'}
					<Play class="size-3.5" style="color: var(--tone-accent-ink);" aria-hidden="true" />
				{:else}
					<Clock class="size-3.5" style="color: var(--fg-secondary);" aria-hidden="true" />
				{/if}
				<span>{entry.kindLabel}</span>
			</span>
			{#if entry.needsYou}
				<span class="stream-needs-pill">Needs you</span>
			{/if}
			{#each entry.projects as proj (proj)}
				<span class="stream-project-pill">{proj}</span>
			{/each}
		</div>

		<div class="stream-row-text">
			<a
				href={resolve(entry.href as '/work')}
				class="stream-row-link min-h-9 min-w-9 [@media(width<=720px)]:min-h-11 [@media(width<=720px)]:min-w-11 [@media(pointer:coarse)]:min-h-11 [@media(pointer:coarse)]:min-w-11"
			>
				{entry.text}
			</a>
		</div>

		{#if entry.preview}
			<p class="stream-row-preview">{entry.preview}</p>
		{/if}

		{#if entry.run}
			<p class="stream-run-meta">
				<span>{entry.run.trigger}</span>
				{#if entry.run.outcome}
					<span aria-hidden="true">·</span>
					<span class={entry.run.state === 'failed' ? 'is-bad' : 'is-ok'}>
						{entry.run.outcome}
					</span>
				{/if}
			</p>
		{/if}

		{#if entry.inboxHref}
			<div class="activity-triage">
				{#if entry.inboxTitle}
					<span class="activity-triage-title">{entry.inboxTitle}</span>
				{/if}
				<a
					href={resolve(entry.inboxHref as '/inbox')}
					class="stream-inbox-action min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					<Inbox class="size-3.5" aria-hidden="true" />
					<span>{entry.inboxLabel ?? 'Review in Inbox'}</span>
				</a>
			</div>
		{/if}

		{#if hasLog}
			<div class="stream-row-actions">
				<button
					type="button"
					aria-expanded={isExpanded}
					aria-controls={`run-log-${entry.id}`}
					onclick={() => onToggleRunLog(entry.id)}
					class="stream-log-btn min-h-9 [@media(width<=720px)]:min-h-11 [@media(pointer:coarse)]:min-h-11"
				>
					{#if isExpanded}
						<ChevronDown class="size-3.5" aria-hidden="true" />
						<span>Hide run log</span>
					{:else}
						<ChevronRight class="size-3.5" aria-hidden="true" />
						<span>Show run log</span>
					{/if}
				</button>
			</div>
		{/if}

		{#if hasLog && entry.run?.log}
			<ol id={`run-log-${entry.id}`} class="stream-run-log runlog" hidden={!isExpanded}>
				{#each entry.run.log as line, idx (`${entry.id}-log-${idx}`)}
					<li>{line}</li>
				{/each}
			</ol>
		{/if}
	</div>

	<time datetime={when.iso} title={when.full} class="stream-row-time tabular-nums">
		{when.text}
	</time>
</li>

<style>
	.stream-row {
		position: relative;
		display: flex;
		align-items: flex-start;
		gap: 12px;
		padding: var(--activity-entry-padding, 14px) var(--timeline-gutter, 8px);
		border-radius: var(--exec-sm-radius, 6px);
		border-bottom: 0;
	}
	.stream-row:not(:last-child)::before {
		content: '';
		position: absolute;
		inset-block: calc(var(--activity-entry-padding, 14px) + 34px)
			calc(2px - var(--activity-entry-padding, 14px));
		inset-inline-start: calc(var(--timeline-gutter, 8px) + 13.5px);
		border-inline-start: 1px solid var(--line-strong);
		pointer-events: none;
	}
	.stream-row:hover {
		background: var(--surface-raised);
	}
	.stream-row-avatar {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border-radius: 999px;
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--exec-ink);
		font-size: 0.6875rem;
		font-weight: 650;
		flex-shrink: 0;
		margin-top: 2px;
	}
	.stream-row-avatar[data-tone='warn'] {
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
	}
	.stream-row-avatar[data-tone='bad'] {
		background: var(--exec-bad-bg);
		box-shadow: inset 0 0 0 1px var(--exec-bad-ring);
		color: var(--exec-bad);
	}
	.stream-row-avatar[data-tone='ok'] {
		background: var(--exec-ok-bg);
		box-shadow: inset 0 0 0 1px var(--exec-ok-ring);
		color: var(--exec-ok);
	}
	.stream-row-content {
		flex: 1;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.stream-row-head {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		font-size: 0.75rem;
		min-width: 0;
	}
	.stream-actor {
		color: var(--exec-ink);
		font-weight: 650;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.stream-kind {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--fg-secondary);
	}
	.stream-needs-pill {
		padding: 1px 7px;
		border-radius: var(--exec-pill-radius, 999px);
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
		font-size: 0.6875rem;
		font-weight: 650;
	}
	.stream-project-pill {
		padding: 1px 7px;
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border);
		color: var(--fg-secondary);
		font-size: 0.6875rem;
		max-width: 100%;
		overflow-wrap: anywhere;
	}
	.stream-row-text {
		margin-top: 2px;
		font-size: 0.875rem;
		line-height: 1.45;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.stream-row-link {
		display: inline-flex;
		align-items: center;
		max-width: 100%;
		color: var(--exec-ink);
		font-weight: 550;
		text-decoration: none;
		overflow-wrap: anywhere;
	}
	.stream-row-link:hover {
		color: var(--tone-accent-ink);
		text-decoration: underline;
	}
	.stream-row-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.stream-row-preview {
		margin: 2px 0 0;
		color: var(--exec-body);
		font-size: 0.8125rem;
		line-height: 1.45;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.stream-run-meta {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		margin: 4px 0 0;
		color: var(--fg-secondary);
		font-size: 0.75rem;
		min-width: 0;
		overflow-wrap: anywhere;
	}
	.stream-run-meta .is-ok {
		color: var(--exec-ok);
		font-weight: 550;
	}
	.stream-run-meta .is-bad {
		color: var(--exec-bad);
		font-weight: 550;
	}
	.activity-triage {
		display: grid;
		justify-items: start;
		gap: 6px;
		width: min(100%, 560px);
		margin-top: 8px;
		padding: 8px 10px;
		border: 1px solid var(--line-strong);
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--surface-raised);
	}
	.activity-triage:focus-within {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.activity-triage-title {
		color: var(--exec-body);
		font-size: 0.8125rem;
		font-weight: 550;
		line-height: 1.4;
		overflow-wrap: anywhere;
	}
	.stream-row-actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		margin-top: 8px;
	}
	.stream-inbox-action {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 0 11px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-warn-bg);
		box-shadow: inset 0 0 0 1px var(--exec-warn-ring);
		color: var(--exec-warn);
		font-size: 0.75rem;
		font-weight: 600;
		text-decoration: none;
	}
	.stream-inbox-action:hover {
		box-shadow: inset 0 0 0 1.5px var(--exec-warn);
	}
	.stream-inbox-action:focus-visible,
	.stream-log-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 1px;
	}
	.stream-log-btn {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		padding: 0 10px;
		border: 0;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well);
		box-shadow: inset 0 0 0 1px var(--exec-border-hi);
		color: var(--exec-body);
		font-size: 0.75rem;
		font-weight: 550;
		cursor: pointer;
	}
	.stream-log-btn:hover {
		color: var(--exec-ink);
	}
	.stream-run-log {
		margin: 8px 0 0;
		padding: 10px 12px;
		list-style: none;
		overflow-x: auto;
		border: 1px solid var(--line);
		border-radius: var(--exec-sm-radius, 6px);
		background: var(--exec-well);
		color: var(--exec-body);
		font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
		font-size: 0.75rem;
		line-height: 1.5;
		white-space: pre-wrap;
		word-break: break-word;
	}
	.stream-run-log[hidden] {
		display: none;
	}
	.stream-row-time {
		color: var(--fg-secondary);
		font-size: 0.75rem;
		white-space: nowrap;
		flex-shrink: 0;
		padding-block-start: 2px;
	}
	@media (forced-colors: active) {
		.activity-triage,
		.stream-inbox-action,
		.stream-log-btn {
			border: 1px solid ButtonText;
		}
	}
</style>
