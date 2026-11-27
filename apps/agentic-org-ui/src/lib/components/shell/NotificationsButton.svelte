<!--
  @component
  Top-Bar Notifications & Live Operations Center trigger + portaled fixed popover (TASK-145).
  Anchored at top-right beside Search/⌘K across Home and every interior page.
  Shows live "In progress" background jobs (with spinning ring on bell) and "Recent"
  non-Inbox FYI completions with accent unread badge, Mark all read, Clear read, and 1-click links.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { Tooltip } from 'bits-ui';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import AlertCircle from '@lucide/svelte/icons/alert-circle';
	import Bell from '@lucide/svelte/icons/bell';
	import CheckCircle2 from '@lucide/svelte/icons/check-circle-2';
	import Pencil from '@lucide/svelte/icons/pencil';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import WifiOff from '@lucide/svelte/icons/wifi-off';
	import X from '@lucide/svelte/icons/x';
	import {
		formatRelativeNotificationTime,
		useNotifications,
		type NotificationIcon
	} from '$lib/platform/workspace/notifications.svelte';

	let {
		offline = false,
		class: className = ''
	}: {
		offline?: boolean;
		class?: string;
	} = $props();

	const notifications = useNotifications();
	let triggerEl = $state<HTMLButtonElement | null>(null);
	let popoverEl = $state<HTMLDivElement | null>(null);
	let closeBtnEl = $state<HTMLButtonElement | null>(null);
	let popTop = $state(60);
	let popRight = $state(16);

	const open = $derived(notifications?.open ?? false);
	const runningJobs = $derived(notifications?.runningJobs ?? []);
	const items = $derived(notifications?.items ?? []);
	const unreadCount = $derived(notifications?.unreadCount ?? 0);
	const hasRead = $derived(items.some((item) => item.read));

	const ariaLabel = $derived.by(() => {
		const parts = ['Notifications'];
		if (runningJobs.length > 0) parts.push(`${runningJobs.length} running`);
		if (unreadCount > 0) parts.push(`${unreadCount} unread`);
		return parts.join(', ');
	});

	const titleText = $derived(
		runningJobs.length > 0
			? `${runningJobs.length} background operation${runningJobs.length > 1 ? 's' : ''} running`
			: ariaLabel
	);

	function updatePosition() {
		if (!triggerEl || typeof window === 'undefined') return;
		const rect = triggerEl.getBoundingClientRect();
		popTop = Math.round(rect.bottom + 8);
		popRight = Math.max(12, Math.round(window.innerWidth - rect.right));
	}

	async function togglePopover() {
		if (!notifications) return;
		if (notifications.open) {
			notifications.close();
			triggerEl?.focus({ preventScroll: true });
			return;
		}
		updatePosition();
		notifications.toggle();
		await tick();
		updatePosition();
		closeBtnEl?.focus({ preventScroll: true });
	}

	function closePopover(restoreFocus = false) {
		if (!notifications?.open) return;
		notifications.close();
		if (restoreFocus) triggerEl?.focus({ preventScroll: true });
	}

	function handleWindowKeydown(event: KeyboardEvent) {
		if (!open || event.key !== 'Escape') return;
		event.preventDefault();
		event.stopPropagation();
		closePopover(true);
	}

	function handleWindowPointerDown(event: PointerEvent) {
		if (!open) return;
		const target = event.target;
		if (target instanceof Node) {
			if (triggerEl?.contains(target) || popoverEl?.contains(target)) return;
		}
		closePopover(false);
	}

	function handleWindowResize() {
		if (open) updatePosition();
	}

	async function openItem(id: string, href: string) {
		notifications?.markRead(id);
		await goto(resolve(href as '/'));
	}

	function iconComponent(icon: NotificationIcon) {
		switch (icon) {
			case 'pencil':
				return Pencil;
			case 'sparkle':
				return Sparkles;
			case 'alert':
				return AlertCircle;
			case 'bell':
				return Bell;
			default:
				return CheckCircle2;
		}
	}
</script>

<svelte:window
	onkeydown={handleWindowKeydown}
	onpointerdown={handleWindowPointerDown}
	onresize={handleWindowResize}
/>

{#if notifications}
	<div class={['notif-wrap', className]}>
		<Tooltip.Provider delayDuration={150}>
			<Tooltip.Root>
				<Tooltip.Trigger>
					{#snippet child({ props })}
						<button
							{...props}
							bind:this={triggerEl}
							type="button"
							data-testid="notif-trigger"
							onclick={() => void togglePopover()}
							aria-label={ariaLabel}
							aria-expanded={open}
							aria-haspopup="dialog"
							class={[
								'util-btn util-notif',
								runningJobs.length > 0 && 'is-busy',
								unreadCount > 0 && 'has-unread'
							]}
						>
							{#if runningJobs.length > 0}
								<span class="notif-ring" aria-hidden="true"></span>
							{/if}
							<Bell class="size-4 shrink-0" aria-hidden="true" />
							{#if unreadCount > 0}
								<span class="notif-badge tabular-nums" aria-hidden="true">
									{unreadCount > 9 ? '9+' : unreadCount}
								</span>
							{/if}
						</button>
					{/snippet}
				</Tooltip.Trigger>
				<Tooltip.Portal>
					<Tooltip.Content
						role="tooltip"
						side="bottom"
						sideOffset={6}
						class="z-[100] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
					>
						{titleText}
					</Tooltip.Content>
				</Tooltip.Portal>
			</Tooltip.Root>
		</Tooltip.Provider>

		{#if open}
			<div
				bind:this={popoverEl}
				role="dialog"
				aria-label="Notifications"
				data-testid="notif-popover"
				class="notif-popover"
				style="top: {popTop}px; right: {popRight}px;"
			>
				<div class="notif-hd">
					<div class="notif-hd-title">
						<strong>Notifications</strong>
						{#if runningJobs.length > 0}
							<span class="notif-running-pill tabular-nums">{runningJobs.length} running</span>
						{/if}
					</div>
					<div class="notif-hd-acts">
						{#if unreadCount > 0}
							<button
								type="button"
								class="notif-text-btn"
								onclick={() => notifications.markAllRead()}
							>
								Mark all read
							</button>
						{/if}
						<Tooltip.Provider delayDuration={150}>
							<Tooltip.Root>
								<Tooltip.Trigger>
									{#snippet child({ props })}
										<button
											{...props}
											bind:this={closeBtnEl}
											type="button"
											class="notif-close-btn"
											aria-label="Close notifications"
											onclick={() => closePopover(true)}
										>
											<X class="size-3.5" aria-hidden="true" />
										</button>
									{/snippet}
								</Tooltip.Trigger>
								<Tooltip.Portal>
									<Tooltip.Content
										role="tooltip"
										side="bottom"
										sideOffset={6}
										class="z-[110] rounded-md bg-tooltip px-2.5 py-1.5 text-xs text-on-emphasis"
									>
										Close notifications · Esc
									</Tooltip.Content>
								</Tooltip.Portal>
							</Tooltip.Root>
						</Tooltip.Provider>
					</div>
				</div>

				{#if offline}
					<div class="notif-offline-row" role="status">
						<WifiOff class="size-3.5 shrink-0" aria-hidden="true" />
						<span>HQ office offline · Live updates paused</span>
					</div>
				{/if}

				<div class="notif-body">
					{#if runningJobs.length > 0}
						<div class="notif-sec">
							<div class="notif-sec-hd">In progress</div>
							<ul class="notif-list" role="list">
								{#each runningJobs as job (job.id)}
									<li class="notif-job-row">
										<span class="notif-job-spin" aria-hidden="true"></span>
										<span class="notif-row-copy">
											<span class="notif-row-title">{job.title}</span>
											<span class="notif-row-sub">{job.step}</span>
										</span>
										<span class="notif-job-pill">Active</span>
									</li>
								{/each}
							</ul>
						</div>
					{/if}

					<div class="notif-sec">
						<div class="notif-sec-hd">Recent</div>
						{#if items.length === 0}
							<p class="notif-empty">All caught up — no recent system notifications.</p>
						{:else}
							<ul class="notif-list" role="list">
								{#each items as item (item.id)}
									{@const RowIcon = iconComponent(item.icon)}
									<li>
										<button
											type="button"
											class={['notif-row', !item.read && 'is-unread']}
											onclick={() => void openItem(item.id, item.href)}
										>
											<span class={['notif-row-ic', `tone-${item.tone}`]} aria-hidden="true">
												<RowIcon class="size-3.5" />
											</span>
											<span class="notif-row-copy">
												<span class="notif-row-top">
													<span class="notif-row-title">{item.title}</span>
													<span class="notif-row-time tabular-nums">
														{formatRelativeNotificationTime(item.createdAt)}
													</span>
												</span>
												<span class="notif-row-sub">{item.summary}</span>
											</span>
											{#if !item.read}
												<span class="notif-unread-dot" aria-hidden="true"></span>
											{/if}
										</button>
									</li>
								{/each}
							</ul>
						{/if}
					</div>
				</div>

				<div class="notif-foot">
					<a href={resolve('/activity')} onclick={() => closePopover(false)}>View all activity</a>
					{#if hasRead}
						<button type="button" class="notif-text-btn" onclick={() => notifications.clearRead()}>
							Clear read
						</button>
					{/if}
				</div>
			</div>
		{/if}
	</div>
{/if}

<style>
	.notif-wrap {
		position: relative;
		display: inline-flex;
		flex: none;
	}
	.util-btn {
		position: relative;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: max(2.5rem, var(--triage-control-height, 36px));
		height: max(2.5rem, var(--triage-control-height, 36px));
		padding: 0;
		border: 1px solid var(--exec-border-hi, var(--line-strong));
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--exec-well, var(--surface-raised));
		color: var(--exec-muted, var(--fg-muted));
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			border-color 0.14s ease,
			color 0.14s ease;
	}
	.util-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		border-color: var(--tone-accent-ring, var(--line-strong));
		color: var(--exec-ink, var(--fg));
	}
	.util-btn:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.util-notif.has-unread,
	.util-notif.is-busy {
		color: var(--exec-ink, var(--fg));
	}
	.util-notif.is-busy {
		border-color: var(--tone-accent-ring, var(--brand));
	}
	.notif-badge {
		position: absolute;
		top: -4px;
		right: -4px;
		min-width: 17px;
		height: 17px;
		padding: 0 4px;
		border-radius: 999px;
		background: var(--primary);
		color: var(--on-emphasis);
		font-size: 0.6875rem;
		font-weight: 700;
		line-height: 17px;
		text-align: center;
		box-shadow: 0 0 0 2px var(--exec-bg, var(--surface));
	}
	.notif-ring {
		position: absolute;
		inset: -2px;
		border-radius: inherit;
		border: 2px solid transparent;
		border-top-color: var(--tone-accent, var(--brand));
		border-right-color: var(--tone-accent-ring, var(--brand));
		pointer-events: none;
		animation: notif-spin 0.9s linear infinite;
	}
	@keyframes notif-spin {
		to {
			transform: rotate(360deg);
		}
	}
	.notif-popover {
		position: fixed;
		z-index: 75;
		width: min(376px, calc(100vw - 24px));
		max-height: min(520px, 72dvh);
		display: flex;
		flex-direction: column;
		overflow: hidden;
		border: 1px solid var(--exec-notif-border, var(--exec-border-hi, var(--line-strong)));
		border-radius: 12px;
		background: var(--exec-notif-bg, var(--exec-cmdk-menu-bg, var(--surface)));
		-webkit-backdrop-filter: var(--exec-notif-blur, var(--exec-cmdk-menu-blur, none));
		backdrop-filter: var(--exec-notif-blur, var(--exec-cmdk-menu-blur, none));
		color: var(--exec-ink, var(--fg));
		box-shadow: var(--exec-notif-shadow, var(--exec-cmdk-menu-shadow, none));
	}
	.notif-hd {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 12px 14px;
		border-bottom: 1px solid var(--exec-border, var(--line));
		flex: none;
	}
	.notif-hd-title {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		font-size: 0.875rem;
	}
	.notif-hd-acts {
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}
	.notif-running-pill {
		padding: 2px 7px;
		border-radius: 999px;
		background: var(--tone-accent-bg, var(--exec-accent-soft));
		border: 1px solid var(--tone-accent-ring, var(--exec-accent-ring));
		color: var(--tone-accent-ink, var(--link));
		font-size: 0.75rem;
		font-weight: 600;
	}
	.notif-text-btn {
		padding: 4px 7px;
		border-radius: 6px;
		color: var(--tone-accent-ink, var(--link));
		font-size: 0.75rem;
		font-weight: 600;
		cursor: pointer;
	}
	.notif-text-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	.notif-close-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 28px;
		height: 28px;
		border-radius: 6px;
		color: var(--exec-muted, var(--fg-muted));
		cursor: pointer;
	}
	.notif-close-btn:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
		color: var(--exec-ink, var(--fg));
	}
	.notif-offline-row {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 14px;
		border-bottom: 1px solid var(--exec-border, var(--line));
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
		font-size: 0.75rem;
		font-weight: 550;
	}
	.notif-body {
		flex: 1 1 auto;
		overflow-y: auto;
		padding: 6px 0;
	}
	.notif-sec + .notif-sec {
		border-top: 1px solid var(--exec-border, var(--line));
		margin-top: 4px;
		padding-top: 4px;
	}
	.notif-sec-hd {
		padding: 6px 14px 4px;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.75rem;
		font-weight: 650;
		letter-spacing: 0.06em;
		text-transform: uppercase;
	}
	.notif-list {
		display: grid;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.notif-job-row,
	.notif-row {
		display: flex;
		width: 100%;
		align-items: flex-start;
		gap: 10px;
		padding: 9px 14px;
		text-align: left;
		transition: background-color 0.12s ease;
	}
	.notif-row {
		cursor: pointer;
	}
	.notif-row:hover {
		background: var(--exec-btn-ghost-bg-hover, var(--surface-overlay));
	}
	.notif-row.is-unread {
		background: color-mix(in srgb, var(--tone-accent-bg, transparent) 60%, transparent);
	}
	.notif-row-ic {
		width: 24px;
		height: 24px;
		border-radius: 7px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		flex: none;
		margin-top: 1px;
		background: var(--exec-well, var(--surface-raised));
		color: var(--tone-accent-ink, var(--link));
	}
	.notif-row-ic.tone-ok {
		background: var(--tone-ok-bg);
		color: var(--tone-ok-ink);
	}
	.notif-row-ic.tone-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
	}
	.notif-row-ic.tone-warn {
		background: var(--tone-warn-bg);
		color: var(--tone-warn-ink);
	}
	.notif-row-ic.tone-bad {
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
	}
	.notif-row-copy {
		flex: 1 1 auto;
		min-width: 0;
		display: grid;
		gap: 2px;
	}
	.notif-row-top {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 8px;
	}
	.notif-row-title {
		font-size: 0.8125rem;
		font-weight: 600;
		color: var(--exec-ink, var(--fg));
		overflow-wrap: anywhere;
	}
	.notif-row-time {
		flex: none;
		font-size: 0.75rem;
		color: var(--exec-muted, var(--fg-muted));
	}
	.notif-row-sub {
		font-size: 0.75rem;
		line-height: 1.4;
		color: var(--exec-muted, var(--fg-muted));
	}
	.notif-unread-dot {
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--tone-accent, var(--brand));
		flex: none;
		margin-top: 6px;
	}
	.notif-job-spin {
		width: 15px;
		height: 15px;
		margin-top: 3px;
		border-radius: 50%;
		border: 2px solid var(--tone-accent-ring, var(--line-strong));
		border-top-color: var(--tone-accent, var(--brand));
		flex: none;
		animation: notif-spin 0.85s linear infinite;
	}
	.notif-job-pill {
		flex: none;
		padding: 2px 6px;
		border-radius: 999px;
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		font-size: 0.75rem;
		font-weight: 600;
	}
	.notif-empty {
		padding: 24px 16px;
		text-align: center;
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.8125rem;
	}
	.notif-foot {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
		padding: 10px 14px;
		border-top: 1px solid var(--exec-border, var(--line));
		font-size: 0.75rem;
		flex: none;
	}
	.notif-foot a {
		color: var(--exec-body, var(--fg-secondary));
		font-weight: 550;
		text-decoration: none;
	}
	.notif-foot a:hover {
		color: var(--exec-ink, var(--fg));
		text-decoration: underline;
	}
	:global(html[data-ui-theme='cozy']) .util-btn {
		border-radius: 999px;
		background: var(--cozy-secondary);
		border-color: var(--cozy-secondary-edge);
		box-shadow: var(--exec-cta-shadow, none);
		color: var(--exec-ink);
	}
	@media (prefers-reduced-motion: reduce) {
		.notif-ring,
		.notif-job-spin {
			animation: none;
		}
	}
</style>
