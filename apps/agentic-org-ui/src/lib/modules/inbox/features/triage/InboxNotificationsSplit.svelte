<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import AlertTriangle from '@lucide/svelte/icons/alert-triangle';
	import Bell from '@lucide/svelte/icons/bell';
	import Check from '@lucide/svelte/icons/check';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import ExternalLink from '@lucide/svelte/icons/external-link';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Trash2 from '@lucide/svelte/icons/trash-2';
	import {
		formatRelativeNotificationTime,
		useNotifications,
		type BackgroundJob,
		type ShellNotification
	} from '$lib/platform/workspace/notifications.svelte';
	import { splitHref } from './triage-model';

	let {
		searchQuery = '',
		nowMs = Date.now()
	}: {
		searchQuery?: string;
		nowMs?: number;
	} = $props();

	const notifications = useNotifications();
	let selectedKey = $state<string | undefined>(undefined);

	const needle = $derived(searchQuery.trim().toLowerCase());
	const runningJobs = $derived(
		(notifications?.runningJobs ?? []).filter((job) =>
			needle ? `${job.title} ${job.step}`.toLowerCase().includes(needle) : true
		)
	);
	const allNotices = $derived(
		(notifications?.items ?? []).filter((n) =>
			needle ? `${n.title} ${n.summary}`.toLowerCase().includes(needle) : true
		)
	);
	const unreadNotices = $derived(allNotices.filter((n) => !n.read));
	const earlierNotices = $derived(allNotices.filter((n) => n.read));
	const totalVisible = $derived(runningJobs.length + allNotices.length);

	type SelectedEntry =
		{ kind: 'job'; job: BackgroundJob } | { kind: 'notice'; notice: ShellNotification };

	const selectedEntry = $derived.by<SelectedEntry | undefined>(() => {
		if (selectedKey?.startsWith('job:')) {
			const id = selectedKey.slice(4);
			const job = runningJobs.find((j) => j.id === id);
			if (job) return { kind: 'job', job };
		}
		if (selectedKey?.startsWith('notice:')) {
			const id = selectedKey.slice(7);
			const notice = allNotices.find((n) => n.id === id);
			if (notice) return { kind: 'notice', notice };
		}
		if (runningJobs[0]) return { kind: 'job', job: runningJobs[0] };
		if (unreadNotices[0]) return { kind: 'notice', notice: unreadNotices[0] };
		if (earlierNotices[0]) return { kind: 'notice', notice: earlierNotices[0] };
		return undefined;
	});

	const activeKey = $derived(
		selectedEntry
			? selectedEntry.kind === 'job'
				? `job:${selectedEntry.job.id}`
				: `notice:${selectedEntry.notice.id}`
			: undefined
	);

	function openNoticeTarget(notice: ShellNotification) {
		notifications?.markRead(notice.id);
		const { pathname, suffix } = splitHref(notice.href);
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- pathname passes through resolve() while query suffix stays intact
		void goto(`${resolve(pathname as '/work')}${suffix}`);
	}
</script>

{#snippet noticeIcon(icon: ShellNotification['icon'])}
	{#if icon === 'check'}
		<CheckCircle2 class="size-3.5 shrink-0" aria-hidden="true" />
	{:else if icon === 'pencil'}
		<Pencil class="size-3.5 shrink-0" aria-hidden="true" />
	{:else if icon === 'sparkle'}
		<Sparkles class="size-3.5 shrink-0" aria-hidden="true" />
	{:else if icon === 'alert'}
		<AlertTriangle class="size-3.5 shrink-0" aria-hidden="true" />
	{:else}
		<Bell class="size-3.5 shrink-0" aria-hidden="true" />
	{/if}
{/snippet}

<div class="notif-split min-h-0 flex-1">
	<section class="notif-queue flex min-h-0 min-w-0 flex-col" aria-label="Notifications list">
		<div class="notif-queue-summary">
			<span class="tabular-nums">
				{totalVisible === 0
					? 'No notifications'
					: totalVisible === 1
						? '1 notification'
						: `${totalVisible} notifications`}
			</span>
			<div class="notif-queue-actions">
				{#if unreadNotices.length > 0}
					<button
						type="button"
						onclick={() => notifications?.markAllRead()}
						class="notif-action-link"
					>
						<Check class="size-3" aria-hidden="true" />
						<span>Mark all read</span>
					</button>
				{/if}
				{#if earlierNotices.length > 0}
					<button
						type="button"
						onclick={() => notifications?.clearRead()}
						class="notif-action-link"
					>
						<Trash2 class="size-3" aria-hidden="true" />
						<span>Clear read</span>
					</button>
				{/if}
			</div>
		</div>

		<div class="notif-queue-scroll flex-1 overflow-y-auto">
			{#if totalVisible === 0}
				<div class="notif-empty">
					<span class="notif-empty-ico flex size-10 items-center justify-center">
						<Bell class="size-5" aria-hidden="true" />
					</span>
					<h2 class="notif-empty-title">
						{needle ? 'No matching notifications' : 'All quiet here'}
					</h2>
					<p class="notif-empty-copy">
						{needle
							? `Nothing matched “${searchQuery.trim()}”.`
							: 'Background work like Tess task intake and FYI updates will show up here without cluttering your decisions.'}
					</p>
				</div>
			{:else}
				{#if runningJobs.length > 0}
					<section class="notif-group" aria-label="In progress">
						<div class="notif-group-head">
							<h2>In progress</h2>
						</div>
						<ul role="list" class="notif-list">
							{#each runningJobs as job (job.id)}
								{@const key = `job:${job.id}`}
								<li>
									<button
										type="button"
										aria-current={activeKey === key ? 'true' : undefined}
										onclick={() => (selectedKey = key)}
										class={['notif-row w-full text-left', activeKey === key && 'is-selected']}
									>
										<span class="notif-row-head">
											<span class="notif-ico tone-accent">
												<LoaderCircle class="size-3.5 animate-spin" aria-hidden="true" />
											</span>
											<strong class="notif-row-title">{job.title}</strong>
											<time class="notif-row-time"
												>{formatRelativeNotificationTime(job.startedAt, nowMs)}</time
											>
										</span>
										<span class="notif-row-sub">{job.step}</span>
									</button>
								</li>
							{/each}
						</ul>
					</section>
				{/if}

				{#if unreadNotices.length > 0}
					<section class="notif-group" aria-label="Unread">
						<div class="notif-group-head">
							<h2>Unread</h2>
						</div>
						<ul role="list" class="notif-list">
							{#each unreadNotices as notice (notice.id)}
								{@const key = `notice:${notice.id}`}
								<li>
									<button
										type="button"
										aria-current={activeKey === key ? 'true' : undefined}
										onclick={() => (selectedKey = key)}
										class={[
											'notif-row w-full text-left is-unread',
											activeKey === key && 'is-selected'
										]}
									>
										<span class="notif-row-head">
											<span class={['notif-ico', `tone-${notice.tone}`]}>
												{@render noticeIcon(notice.icon)}
											</span>
											<strong class="notif-row-title">{notice.title}</strong>
											<span class="notif-unread-dot" aria-hidden="true"></span>
											<time class="notif-row-time"
												>{formatRelativeNotificationTime(notice.createdAt, nowMs)}</time
											>
										</span>
										<span class="notif-row-sub">{notice.summary}</span>
									</button>
								</li>
							{/each}
						</ul>
					</section>
				{/if}

				{#if earlierNotices.length > 0}
					<section class="notif-group" aria-label="Earlier">
						<div class="notif-group-head">
							<h2>Earlier</h2>
						</div>
						<ul role="list" class="notif-list">
							{#each earlierNotices as notice (notice.id)}
								{@const key = `notice:${notice.id}`}
								<li>
									<button
										type="button"
										aria-current={activeKey === key ? 'true' : undefined}
										onclick={() => (selectedKey = key)}
										class={['notif-row w-full text-left', activeKey === key && 'is-selected']}
									>
										<span class="notif-row-head">
											<span class={['notif-ico', `tone-${notice.tone}`]}>
												{@render noticeIcon(notice.icon)}
											</span>
											<strong class="notif-row-title">{notice.title}</strong>
											<time class="notif-row-time"
												>{formatRelativeNotificationTime(notice.createdAt, nowMs)}</time
											>
										</span>
										<span class="notif-row-sub">{notice.summary}</span>
									</button>
								</li>
							{/each}
						</ul>
					</section>
				{/if}
			{/if}
		</div>
	</section>

	<section class="notif-detail min-h-0 min-w-0 overflow-y-auto" aria-label="Notification details">
		{#if !selectedEntry}
			<div class="notif-detail-empty">
				<Bell class="size-6 opacity-50" aria-hidden="true" />
				<p>Select a notification to inspect its details.</p>
			</div>
		{:else if selectedEntry.kind === 'job'}
			<div class="notif-detail-inner">
				<div class="notif-detail-kicker">
					<span class="notif-pill tone-accent">In progress</span>
					<span class="tabular-nums"
						>Started {formatRelativeNotificationTime(selectedEntry.job.startedAt, nowMs)}</span
					>
				</div>
				<h2 class="notif-detail-title">{selectedEntry.job.title}</h2>
				<p class="notif-detail-body">{selectedEntry.job.step}</p>
				<p class="notif-detail-hint">
					Running asynchronously in the background. You can keep working anywhere in the workspace;
					this entry will convert into a completion link as soon as it finishes.
				</p>
			</div>
		{:else}
			{@const n = selectedEntry.notice}
			<div class="notif-detail-inner">
				<div class="notif-detail-kicker">
					<span class={['notif-pill', `tone-${n.tone}`]}>
						{n.read ? 'Read' : 'Unread'}
					</span>
					<span class="tabular-nums">{formatRelativeNotificationTime(n.createdAt, nowMs)}</span>
				</div>
				<h2 class="notif-detail-title">{n.title}</h2>
				<p class="notif-detail-body">{n.summary}</p>
				<div class="notif-detail-actions">
					<button
						type="button"
						onclick={() => openNoticeTarget(n)}
						class="exec-cta inline-flex min-h-9 items-center gap-1.5"
					>
						<span class="exec-cta-fill" aria-hidden="true"></span>
						<span class="exec-cta-inner">
							<span>Open item</span>
							<ExternalLink class="size-3.5" aria-hidden="true" />
						</span>
					</button>
					{#if !n.read}
						<button
							type="button"
							onclick={() => notifications?.markRead(n.id)}
							class="notif-secondary-btn"
						>
							<Check class="size-3.5" aria-hidden="true" />
							<span>Mark as read</span>
						</button>
					{/if}
				</div>
			</div>
		{/if}
	</section>
</div>

<style>
	.notif-split {
		display: grid;
		grid-template-columns: clamp(19rem, 34%, 31rem) minmax(0, 1fr);
		grid-template-rows: minmax(0, 1fr);
		min-height: 0;
	}
	@media (max-width: 959px) {
		.notif-split {
			grid-template-columns: minmax(0, 1fr);
		}
	}
	.notif-queue {
		border-inline-end: 1px solid var(--exec-border);
		background: var(--exec-well);
	}
	.notif-queue-summary {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 8px 16px;
		border-bottom: 1px solid var(--exec-border);
		color: var(--exec-muted);
		font-size: 0.75rem;
	}
	.notif-queue-actions {
		display: inline-flex;
		align-items: center;
		gap: 10px;
	}
	.notif-action-link {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--exec-accent-ink);
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
	}
	.notif-action-link:hover {
		text-decoration: underline;
	}
	.notif-group-head {
		padding: 6px 16px 4px;
	}
	.notif-group-head h2 {
		color: var(--exec-muted);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.notif-list {
		display: grid;
		gap: 4px;
		padding: 4px 8px 8px;
	}
	.notif-row {
		display: grid;
		gap: 4px;
		padding: 10px 10px;
		border-radius: var(--exec-sm-radius);
		background: transparent;
		color: var(--exec-body);
		cursor: pointer;
		transition: background-color 120ms ease-out;
	}
	.notif-row:hover {
		background: var(--triage-row-hover, var(--exec-surface-hi));
	}
	.notif-row.is-selected {
		background: var(--triage-row-selected, var(--exec-accent-soft));
	}
	.notif-row-head {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.notif-row-title {
		flex: 1 1 auto;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--exec-ink);
		font-size: 0.875rem;
		font-weight: 600;
	}
	.notif-row-time {
		flex: none;
		color: var(--exec-muted);
		font-size: 0.75rem;
		font-variant-numeric: tabular-nums;
	}
	.notif-row-sub {
		padding-inline-start: 24px;
		color: var(--exec-muted);
		font-size: 0.8125rem;
		line-height: 1.4;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.notif-ico {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		flex: none;
	}
	.notif-ico.tone-ok {
		color: var(--tone-ok-ink);
	}
	.notif-ico.tone-accent {
		color: var(--tone-accent-ink);
	}
	.notif-ico.tone-warn {
		color: var(--tone-warn-ink);
	}
	.notif-ico.tone-bad {
		color: var(--tone-bad-ink);
	}
	.notif-unread-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--tone-accent);
		flex: none;
	}
	.notif-empty {
		display: grid;
		justify-items: start;
		gap: 6px;
		padding: 24px;
	}
	.notif-empty-ico {
		border-radius: var(--exec-inset-radius);
		background: var(--exec-accent-soft);
		color: var(--exec-accent-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.notif-empty-title {
		color: var(--exec-ink);
		font-size: 0.9375rem;
		font-weight: 600;
	}
	.notif-empty-copy {
		color: var(--exec-muted);
		font-size: 0.8125rem;
		line-height: 1.5;
	}
	.notif-detail {
		padding: 28px 32px;
	}
	.notif-detail-empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 8px;
		height: 100%;
		min-height: 220px;
		color: var(--exec-muted);
		font-size: 0.875rem;
	}
	.notif-detail-inner {
		max-width: 42rem;
		display: flex;
		flex-direction: column;
		gap: 14px;
	}
	.notif-detail-kicker {
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 0.75rem;
		color: var(--exec-muted);
	}
	.notif-pill {
		padding: 2px 8px;
		border-radius: var(--exec-pill-radius);
		font-size: 0.7rem;
		font-weight: 600;
	}
	.notif-pill.tone-ok {
		background: var(--tone-ok-bg);
		color: var(--tone-ok-ink);
		box-shadow: inset 0 0 0 1px var(--tone-ok-ring);
	}
	.notif-pill.tone-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	.notif-pill.tone-warn {
		background: var(--tone-warn-bg);
		color: var(--tone-warn-ink);
		box-shadow: inset 0 0 0 1px var(--tone-warn-ring);
	}
	.notif-pill.tone-bad {
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring);
	}
	.notif-detail-title {
		color: var(--exec-ink);
		font-size: 1.25rem;
		font-weight: 600;
		line-height: 1.3;
		letter-spacing: -0.02em;
	}
	.notif-detail-body {
		color: var(--exec-body);
		font-size: 0.9375rem;
		line-height: 1.6;
	}
	.notif-detail-hint {
		color: var(--exec-muted);
		font-size: 0.8125rem;
		line-height: 1.5;
	}
	.notif-detail-actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
		padding-top: 6px;
	}
	.notif-secondary-btn {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-height: 36px;
		padding: 0 14px;
		border-radius: var(--exec-ctrl-radius);
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		font-size: 0.8125rem;
		font-weight: 600;
		cursor: pointer;
	}
	.notif-secondary-btn:hover {
		background: var(--exec-btn-outline-bg-hover);
		color: var(--exec-btn-outline-ink-hover);
	}
</style>
