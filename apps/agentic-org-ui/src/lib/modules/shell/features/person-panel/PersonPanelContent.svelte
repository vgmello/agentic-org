<!--
  @component
  Read-only member profile summary and current work list rendered inside PersonSidePanel.
  Built-in agents (HR & Tess) render an explicit Built-in badge and expose zero mutation controls.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import MessageSquare from '@lucide/svelte/icons/message-square';
	import Shield from '@lucide/svelte/icons/shield';
	import Sparkles from '@lucide/svelte/icons/sparkles';
	import Star from '@lucide/svelte/icons/star';
	import User from '@lucide/svelte/icons/user';
	import Users from '@lucide/svelte/icons/users';
	import X from '@lucide/svelte/icons/x';
	import Avatar from '$lib/components/Avatar.svelte';
	import { ModalTooltip } from '$lib/components/ui/tooltip';
	import { isBuiltInAgent, type OrgSnapshot } from '$lib/proto';
	import type { PersonPanelProfile, PersonPanelWorkItem } from './types';

	let {
		titleId,
		memberId,
		snapshot,
		profile,
		openWork,
		totalOpenWork,
		openWorkUnavailable = false,
		loading,
		error,
		canMessage,
		onClose,
		onRetry,
		onMessageMember
	}: {
		titleId: string;
		memberId: string | null;
		snapshot?: OrgSnapshot;
		profile?: PersonPanelProfile;
		openWork: readonly PersonPanelWorkItem[];
		totalOpenWork: number;
		openWorkUnavailable?: boolean;
		loading: boolean;
		error?: string;
		canMessage: boolean;
		onClose: (restoreFocus?: boolean) => void;
		onRetry: () => void;
		onMessageMember?: (memberId: string) => void;
	} = $props();

	const snapshotMember = $derived(
		memberId ? snapshot?.members.find((m) => m.id === memberId) : undefined
	);
	const builtIn = $derived(isBuiltInAgent(snapshotMember) || Boolean(profile?.isBuiltIn));
	const displayName = $derived(profile?.name ?? snapshotMember?.name ?? 'Member');
	const displayKind = $derived<'agent' | 'person'>(
		profile?.kind ?? (snapshotMember?.kind === 'agent' ? 'agent' : 'person')
	);
	const displayKindLabel = $derived(
		profile?.kindLabel ?? (displayKind === 'agent' ? 'Agent' : 'Person')
	);
	const displayOwner = $derived(Boolean(profile?.owner ?? snapshotMember?.owner));
	const displayRole = $derived(
		profile?.role ?? snapshotMember?.agent?.title ?? snapshotMember?.agent?.role
	);
	const homeTeam = $derived.by(() => {
		if (profile?.homeTeam) return profile.homeTeam;
		if (!snapshotMember?.home_team_id) return undefined;
		const team = snapshot?.teams.find((t) => t.id === snapshotMember.home_team_id);
		return team ? { id: team.id, name: team.name } : undefined;
	});
	const profileHref = $derived(
		profile?.profileHref ??
			(memberId ? `/members/${encodeURIComponent(memberId)}` : '/teams/members')
	);
	const workTabHref = $derived(
		profile?.workTabHref ??
			(memberId ? `/members/${encodeURIComponent(memberId)}/work` : '/teams/members')
	);

	function closeAfterLinkClick(event: MouseEvent) {
		if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0)
			return;
		queueMicrotask(() => onClose(false));
	}
</script>

<header class="pp-head">
	<Avatar name={displayName} size="md" class="mt-0.5" />
	<div class="pp-id">
		<h2 id={titleId} tabindex="-1">{displayName}</h2>
		<p class="pp-role">
			{#if displayRole}
				<span>{displayRole}</span>
			{/if}
			<span class="pp-kind" data-kind={displayKind}>
				{#if displayOwner}
					<Star class="size-3.5 shrink-0" aria-hidden="true" />
					<span>Owner</span>
				{:else if displayKind === 'agent'}
					<Sparkles class="pp-agent-ico size-3.5 shrink-0" aria-hidden="true" />
					<span>{displayKindLabel}</span>
				{:else}
					<User class="size-3.5 shrink-0" aria-hidden="true" />
					<span>{displayKindLabel}</span>
				{/if}
			</span>
			{#if builtIn}
				<span class="pp-builtin-badge" data-testid="person-panel-builtin-badge">
					<Shield class="size-3 shrink-0" aria-hidden="true" />
					<span>Built-in</span>
				</span>
			{/if}
		</p>
	</div>
	<ModalTooltip label="Close" side="left">
		<button
			type="button"
			onclick={() => onClose(true)}
			aria-label="Close"
			data-testid="person-panel-close"
			class="pp-close inline-flex size-9 shrink-0 items-center justify-center rounded-md focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus [@media(pointer:coarse)]:size-11 [@media(width<=720px)]:size-11"
		>
			<X class="size-4" aria-hidden="true" />
		</button>
	</ModalTooltip>
</header>

<div class="pp-body">
	{#if error}
		<div class="pp-error" role="alert">
			<p>{error}</p>
			{#if memberId}
				<button
					type="button"
					onclick={onRetry}
					class="pp-retry mt-2 inline-flex min-h-9 items-center rounded-md px-2.5 text-xs font-semibold"
				>
					Retry
				</button>
			{/if}
		</div>
	{:else if profile?.presenceLabel && profile.presenceTone}
		<p class="pp-status">
			<span class={['pp-pill', `tone-${profile.presenceTone}`]}>
				{profile.presenceLabel}
			</span>
			{#if profile.currentTaskTitle}
				<span class="pp-muted">Working on {profile.currentTaskTitle}</span>
			{/if}
		</p>
	{:else if loading && !profile}
		<p class="pp-empty" role="status">Loading profile…</p>
	{/if}

	{#if !displayOwner}
		<p class="pp-team">
			<Users class="pp-team-ico size-4 shrink-0" aria-hidden="true" />
			{#if homeTeam}
				<a
					href={resolve(`/teams/${encodeURIComponent(homeTeam.id)}` as '/teams')}
					onclick={closeAfterLinkClick}
					class="pp-link truncate"
				>
					{homeTeam.name}
				</a>
			{:else}
				<span class="pp-muted">No team</span>
			{/if}
		</p>
	{/if}

	{#if memberId}
		<div class="pp-actions">
			{#if canMessage}
				<button
					type="button"
					onclick={() => onMessageMember?.(memberId)}
					class="pp-btn-primary inline-flex min-h-9 flex-1 items-center justify-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
				>
					<MessageSquare class="size-4 shrink-0" aria-hidden="true" />
					<span>Message</span>
				</button>
			{/if}
			<a
				href={resolve(profileHref as '/teams/members')}
				onclick={closeAfterLinkClick}
				class="pp-btn-outline inline-flex min-h-9 flex-1 items-center justify-center rounded-lg px-3 py-1.5 text-sm font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
			>
				View full profile
			</a>
		</div>

		<section class="pp-section" aria-labelledby="pp-work-heading">
			<h3 id="pp-work-heading" class="pp-h">Current work</h3>
			{#if loading && openWork.length === 0}
				<p class="pp-empty">Loading open work…</p>
			{:else if openWork.length > 0}
				<ul class="pp-list">
					{#each openWork as item (item.id)}
						<li>
							<a href={resolve(item.href as '/work')} onclick={closeAfterLinkClick} class="pp-row">
								<span class="pp-row-main">{item.title}</span>
								<span class="pp-row-meta">{item.ref} · {item.statusLabel}</span>
							</a>
						</li>
					{/each}
				</ul>
				{#if totalOpenWork > openWork.length}
					<a
						href={resolve(workTabHref as '/teams/members')}
						onclick={closeAfterLinkClick}
						class="pp-more pp-link"
					>
						All {totalOpenWork} open items
					</a>
				{/if}
			{:else if openWorkUnavailable || Boolean(error)}
				<p class="pp-empty">Could not load open work.</p>
			{:else}
				<p class="pp-empty">No open work assigned.</p>
			{/if}
		</section>
	{/if}
</div>

<style>
	.pp-head {
		display: grid;
		grid-template-columns: auto minmax(0, 1fr) auto;
		align-items: start;
		gap: 12px;
		padding: 20px 20px 8px;
	}
	.pp-id {
		display: grid;
		gap: 2px;
		min-width: 0;
		padding-top: 2px;
	}
	.pp-id h2 {
		margin: 0;
		font-size: 1.0625rem;
		font-weight: 600;
		line-height: 1.3;
		color: var(--exec-ink);
		overflow-wrap: anywhere;
	}
	.pp-id h2:focus:not(:focus-visible) {
		outline: none;
	}
	.pp-role {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 4px 8px;
		margin: 0;
		font-size: 0.8125rem;
		color: var(--exec-muted);
	}
	.pp-kind {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--exec-body);
	}
	.pp-kind :global(.pp-agent-ico) {
		color: var(--tone-accent-ink);
	}
	.pp-builtin-badge {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		padding: 1px 6px;
		border-radius: 999px;
		font-size: 0.7rem;
		font-weight: 600;
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.pp-close {
		color: var(--exec-muted);
		cursor: pointer;
		transition:
			background-color 0.14s ease,
			color 0.14s ease;
	}
	.pp-close:hover {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
	}
	.pp-body {
		display: grid;
		gap: 14px;
		padding: 12px 20px 28px;
	}
	.pp-status,
	.pp-team {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 8px;
		margin: 0;
		font-size: 0.8125rem;
		line-height: 1.45;
		color: var(--exec-body);
		overflow-wrap: anywhere;
	}
	.pp-team :global(.pp-team-ico) {
		color: var(--exec-muted);
	}
	.pp-pill {
		display: inline-flex;
		align-items: center;
		padding: 2px 8px;
		border-radius: 999px;
		font-size: 0.75rem;
		font-weight: 600;
	}
	.pp-pill.tone-ok {
		background: var(--tone-ok-bg);
		color: var(--tone-ok-ink);
		box-shadow: inset 0 0 0 1px var(--tone-ok-ring);
	}
	.pp-pill.tone-warn {
		background: var(--tone-warn-bg);
		color: var(--tone-warn-ink);
		box-shadow: inset 0 0 0 1px var(--tone-warn-ring);
	}
	.pp-pill.tone-accent {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
	.pp-pill.tone-muted {
		background: var(--exec-well);
		color: var(--exec-muted);
		box-shadow: inset 0 0 0 1px var(--exec-border);
	}
	.pp-muted,
	.pp-empty {
		margin: 0;
		font-size: 0.8125rem;
		color: var(--exec-muted);
	}
	.pp-link {
		color: var(--exec-accent-ink);
		font-weight: 500;
	}
	.pp-link:hover {
		text-decoration: underline;
	}
	.pp-actions {
		display: flex;
		flex-wrap: wrap;
		gap: 8px;
		padding-top: 2px;
	}
	.pp-btn-primary {
		background: var(--exec-cta-bg);
		color: var(--exec-on-accent);
		cursor: pointer;
		transition: background-color 0.14s ease;
	}
	.pp-btn-primary:hover {
		background: var(--exec-cta-fill);
	}
	.pp-btn-outline,
	.pp-retry {
		background: var(--exec-btn-outline-bg);
		color: var(--exec-btn-outline-ink);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring);
		transition:
			background-color 0.14s ease,
			box-shadow 0.14s ease;
	}
	.pp-btn-outline:hover,
	.pp-retry:hover {
		background: var(--exec-btn-outline-bg-hover);
		box-shadow: inset 0 0 0 1px var(--exec-btn-outline-ring-hover);
	}
	.pp-section {
		display: grid;
		gap: 4px;
		padding-top: 10px;
		border-top: 1px solid var(--exec-border);
	}
	.pp-h {
		margin: 0 0 2px;
		font-size: 0.75rem;
		font-weight: 600;
		line-height: 1.125rem;
		color: var(--exec-muted);
	}
	.pp-list {
		display: grid;
		gap: 2px;
		margin: 0 -10px;
		padding: 0;
		list-style: none;
	}
	.pp-row {
		display: grid;
		gap: 2px;
		min-height: 40px;
		padding: 7px 10px;
		border-radius: var(--exec-sm-radius, 6px);
		color: var(--exec-ink);
		text-decoration: none;
		transition: background-color 0.14s ease;
	}
	.pp-row:hover {
		background: var(--exec-accent-soft);
	}
	.pp-row:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: -2px;
	}
	.pp-row-main {
		min-width: 0;
		font-size: 0.8125rem;
		font-weight: 500;
		line-height: 1.35;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.pp-row-meta {
		font-size: 0.75rem;
		line-height: 1rem;
		color: var(--exec-muted);
	}
	.pp-more {
		justify-self: start;
		margin-top: 4px;
		font-size: 0.8125rem;
	}
	.pp-error {
		padding: 10px 12px;
		border-radius: var(--exec-ctrl-radius, 8px);
		background: var(--tone-bad-bg);
		color: var(--tone-bad-ink);
		box-shadow: inset 0 0 0 1px var(--tone-bad-ring);
		font-size: 0.8125rem;
	}
	@media (max-width: 720px), (pointer: coarse) {
		.pp-row,
		.pp-btn-primary,
		.pp-btn-outline,
		.pp-retry {
			min-height: 44px;
		}
		.pp-more {
			display: inline-flex;
			align-items: center;
			min-height: 44px;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.pp-close,
		.pp-btn-primary,
		.pp-btn-outline,
		.pp-retry,
		.pp-row {
			transition: none;
		}
	}
</style>
