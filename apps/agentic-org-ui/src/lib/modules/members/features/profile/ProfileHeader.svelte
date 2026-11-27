<!--
  @component
  Member profile header: breadcrumbs, identity masthead, presence badge, profile tabs, and action controls.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import Avatar from '$lib/components/Avatar.svelte';
	import Breadcrumbs from '$lib/components/Breadcrumbs.svelte';
	import InteriorSearchBar from '$lib/components/shell/InteriorSearchBar.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import TabsNav from '$lib/components/TabsNav.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuSeparator,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import { isBuiltInAgent, type Member } from '$lib/proto';
	import PresenceBadge from '../../components/PresenceBadge.svelte';
	import { fact, factsList, factsWrap } from '../../components/facts';
	import { headerPresence, kindWord, reportedText, rowPresence } from '../../present';
	import type { MemberProfile } from '../../types';
	import type { ProfileAction } from './context.svelte';
	import SectionMenu from './SectionMenu.svelte';
	import { tabStrip, tabStripFade } from './tab-strip';
	import { TAB_LABEL, tabHref, tabsFor, type ProfileTab } from './tabs';

	let {
		profile: p,
		record,
		tab,
		hasTab,
		refreshFailed,
		nameHeading = $bindable<HTMLElement | null>(null),
		moreButton = $bindable<HTMLButtonElement | null>(null),
		onAct,
		onOpenAnalysis,
		onChooseSection
	}: {
		profile: MemberProfile;
		record: Member | undefined;
		tab: ProfileTab;
		hasTab: boolean;
		refreshFailed: boolean;
		nameHeading?: HTMLElement | null;
		moreButton?: HTMLButtonElement | null;
		onAct: (action: ProfileAction, opener?: HTMLElement | null) => void;
		onOpenAnalysis: () => void;
		onChooseSection: (id: string) => void;
	} = $props();

	const presence = $derived(p.kind === 'agent' ? headerPresence(p) : rowPresence(p));
	const active = $derived(p.status !== 'inactive');
	const manage = $derived(p.viewer.isOwnerOrAdmin && active && !!record);
	const tabs = $derived(tabsFor(p));

	const linkClass =
		'rounded-sm text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
	const actionBtn = `min-h-9 ${mobileButton} [@media(pointer:coarse)]:min-h-11`;
</script>

<div class="section-header profile-header">
	<div class="px-4 pt-5 min-[640px]:px-6 min-[1024px]:px-8">
		<Breadcrumbs
			items={p.owner
				? [{ label: 'Teams', href: '/teams' }, { label: p.name }]
				: [
						{ label: 'Teams', href: '/teams' },
						{ label: 'Members', href: '/teams/members' },
						{ label: p.name }
					]}
		/>
	</div>
	<header
		class="profile-masthead prof flex flex-wrap items-start justify-between gap-x-6 gap-y-3 px-4 pt-2 pb-3 min-[640px]:px-6 min-[1024px]:px-8"
	>
		<div
			class="flex min-w-0 flex-1 basis-64 flex-col gap-3 min-[640px]:flex-row min-[640px]:gap-x-4"
		>
			<Avatar name={p.name} class="size-10 shrink-0" />
			<div class="min-w-0 flex-1">
				<h2
					id="profile-heading"
					bind:this={nameHeading}
					tabindex="-1"
					class="flex flex-wrap items-center gap-x-2 gap-y-1 text-[clamp(1.375rem,2.2vw,1.875rem)]/[1.22] font-semibold tracking-[-0.03em] text-fg [overflow-wrap:anywhere]"
				>
					{p.name}{p.viewer.isSelf ? ' (you)' : ''}
					{#if p.owner}<StatusPill label="Owner" class="profile-owner-pill" />{/if}
				</h2>
				<div class={factsWrap}>
					<ul role="list" class="{factsList} text-sm/6 text-fg-secondary">
						{#if p.role}<li class={fact}>{p.role}</li>{/if}
						<li class="{fact} font-mono text-fg-muted">@{p.handle}</li>
					</ul>
				</div>
				<div class="mt-1 {factsWrap}">
					<ul role="list" class="{factsList} text-xs/5 text-fg-muted">
						<li class={fact}>{kindWord(p.kind)}</li>
						{#if p.homeTeam}
							<li class={fact}>
								Home team <a
									href={resolve(`/teams/${encodeURIComponent(p.homeTeam.id)}` as '/')}
									class={linkClass}>{p.homeTeam.name}</a
								>{#if p.homeTeam.ref}<span class="whitespace-nowrap">{` · ${p.homeTeam.ref}`}</span
									>{/if}
							</li>
						{:else if active}
							<li class="{fact} text-danger-text">
								No home team on record: this record is invalid
							</li>
						{:else}
							<li class={fact}>Former member, no team</li>
						{/if}
						{#if p.reportsTo}
							<li class={fact}>
								Reports to <a
									href={resolve(`/members/${encodeURIComponent(p.reportsTo.id)}` as '/')}
									class={linkClass}>{p.reportsTo.name}</a
								>
							</li>
						{:else}
							<li class={fact}>No explicit manager</li>
						{/if}
					</ul>
				</div>
				{#if p.kind === 'agent' && active}
					<div class={factsWrap}>
						<ul role="list" class="{factsList} text-xs/5 text-fg-muted">
							{#if p.office}
								<li class={fact}>Runs at {p.office.name}</li>
								<li class={fact}>{reportedText(p.office.observedAt)}</li>
							{:else}
								<li class={fact}>Unplaced</li>
							{/if}
						</ul>
					</div>
				{:else if p.contact}
					<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">{p.contact}</p>
				{/if}
				{#if presence || !active || p.canSignIn}
					<div class="mt-2 flex flex-wrap gap-2">
						{#if !active}
							<StatusPill label="Former member" />
						{:else if presence}
							<!-- The header's pill wraps whole words instead of cutting the time off (app-shell-ux §6a). -->
							<PresenceBadge
								{presence}
								class="items-start rounded-lg text-left [&>span]:overflow-visible [&>span]:whitespace-normal"
							/>
						{/if}
						{#if p.canSignIn}<StatusPill label="Can sign in" />{/if}
					</div>
				{/if}
				<!-- A pending-turn check that failed: quiet, in place; the page and any open dialog stay. -->
				<p class={['text-xs/5 text-fg-muted', refreshFailed && 'mt-1']} role="status">
					{refreshFailed ? "Couldn't refresh; checking again." : ''}
				</p>
			</div>
		</div>
		<InteriorSearchBar class="w-full shrink-0 sm:w-[clamp(18rem,28vw,25rem)]" />
	</header>
	<div class="section-toolbar profile-toolbar">
		<div class="primary-tab-row relative {tabStripFade}" {@attach tabStrip(page.url.pathname)}>
			<TabsNav
				label="{p.name}'s profile"
				tabs={tabs.map((t) => ({ label: TAB_LABEL[t], href: tabHref(p.id, t) }))}
				class="tabs"
			/>
		</div>
		{#if active}
			<div class="section-actions profile-actions flex flex-wrap items-center gap-2">
				{#if p.kind === 'agent'}
					<Button
						variant="outline"
						size="sm"
						class={actionBtn}
						data-testid="agent-analysis-btn"
						onclick={onOpenAnalysis}>Analysis Mode</Button
					>
					<Button
						variant="outline"
						size="sm"
						class={actionBtn}
						onclick={(e) => onAct('run', e.currentTarget)}>Run now</Button
					>
				{/if}
				<Button
					variant="outline"
					size="sm"
					class={actionBtn}
					onclick={(e) => onAct('assign', e.currentTarget)}>Assign task</Button
				>
				{#if manage}
					<DropdownMenu>
						<DropdownMenuTrigger>
							{#snippet child({ props })}
								<Button
									{...props}
									bind:ref={moreButton}
									variant="outline"
									size="sm"
									class={actionBtn}>More<span class="sr-only"> actions for {p.name}</span></Button
								>
							{/snippet}
						</DropdownMenuTrigger>
						<DropdownMenuContent align="end" class="triage-menu-surface">
							{#if p.kind === 'agent' && !isBuiltInAgent(record)}
								{#if p.status === 'paused'}
									<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => onAct('resume')}
										>Resume</DropdownMenuItem
									>
								{:else}
									<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => onAct('pause')}
										>Pause…</DropdownMenuItem
									>
								{/if}
							{/if}
							<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => onAct('edit')}
								>Edit…</DropdownMenuItem
							>
							{#if p.homeTeam && !isBuiltInAgent(record)}
								<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => onAct('move')}
									>Move home team…</DropdownMenuItem
								>
							{/if}
							{#if p.kind === 'agent' && !isBuiltInAgent(record)}
								<DropdownMenuSeparator />
								<DropdownMenuItem
									variant="destructive"
									class="max-[639px]:min-h-11"
									onSelect={() => onAct('offboard')}>Offboard…</DropdownMenuItem
								>
							{:else if p.kind !== 'agent' && !p.owner}
								<DropdownMenuSeparator />
								<DropdownMenuItem
									variant="destructive"
									class="max-[639px]:min-h-11"
									onSelect={() => onAct('remove')}>Remove person…</DropdownMenuItem
								>
							{/if}
						</DropdownMenuContent>
					</DropdownMenu>
				{/if}
			</div>
		{/if}
	</div>
	<SectionMenu
		class="min-[640px]:hidden"
		label="{p.name}'s profile"
		current={hasTab ? tab : ''}
		sections={tabs.map((t) => ({ id: t, label: TAB_LABEL[t] }))}
		onchoose={onChooseSection}
	/>
</div>

<style>
	.section-header {
		position: relative;
		z-index: 1;
		flex: none;
		container-type: inline-size;
		--profile-gutter: 1rem;
	}
	@media (min-width: 640px) {
		.section-header {
			--profile-gutter: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.section-header {
			--profile-gutter: 2rem;
		}
	}
	.profile-masthead {
		border: 0;
		background: none;
		box-shadow: none;
	}
	.section-toolbar {
		position: relative;
		display: flex;
		flex-wrap: wrap;
		align-items: stretch;
		gap: 6px 12px;
		padding: 4px var(--profile-gutter) 1px;
		border-block-end: 1px solid var(--line);
	}
	.section-toolbar > :not(.primary-tab-row) {
		align-self: center;
	}
	.primary-tab-row {
		position: relative;
		display: flex;
		align-items: stretch;
		min-inline-size: 0;
		max-inline-size: 100%;
	}
	@media (width < 640px) {
		.primary-tab-row {
			display: none;
		}
	}
	.primary-tab-row::after {
		content: '';
		position: absolute;
		inset-block-start: 100%;
		inset-inline-start: calc(-1 * var(--profile-gutter));
		inline-size: 100cqi;
		border-block-start: 1px solid var(--line);
		pointer-events: none;
	}
	.primary-tab-row > :global(nav) {
		min-width: 0;
		max-width: 100%;
		padding-inline: 0;
		border-bottom: 0;
	}
	.section-actions {
		margin-inline-start: auto;
		margin-block: 4px;
	}
	@container (max-width: 40rem) {
		.primary-tab-row {
			flex-basis: 100%;
		}
		.section-toolbar :global(.profile-pause) {
			display: none;
		}
	}
	:global(html[data-ui-theme^='report']) .profile-toolbar {
		border-block-end: 1.5px solid var(--report-masthead-rule);
	}
	:global(.profile-owner-pill) {
		background: var(--tone-accent-bg);
		color: var(--tone-accent-ink);
		box-shadow: inset 0 0 0 1px var(--tone-accent-ring);
	}
</style>
