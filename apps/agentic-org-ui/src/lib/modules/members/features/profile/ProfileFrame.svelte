<!--
  @component
  The colleague page's frame (our-org-in-the-app §5, AC-IA06): one layout for people and agents. The header shows the
  name, role and handle, Agent or Person in words, the one Home team and the explicit Reports to; an agent has one
  truthful presence signal and one context line (its office and how fresh that is, or Unplaced). Actions: Run now and
  Assign task, and More: Pause or Resume and Offboard for agents, Edit and Move home team for everyone. Edit and Move home team (both kinds) reuse the
  org chart's dialogs, declared in the gate manifest's allowedCoupling. Then the link tabs, each a URL, and the current tab. The tabs read the profile from the context.
-->
<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { afterNavigate, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import Avatar from '$lib/components/Avatar.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import EditPersonDialog from '$lib/components/EditPersonDialog.svelte';
	import MoveMemberDialog from '$lib/components/MoveMemberDialog.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import TabsNav from '$lib/components/TabsNav.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuSeparator,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import { EditAgentDialog } from '$lib/modules/employees';
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/office/api';
	import { offboardAgent, setAgentStatus } from '$lib/org/actions';
	import { offboardSummary } from '$lib/org/agents';
	import { orgApi } from '$lib/org/api';
	import { sessionFor } from '$lib/org/session';
	import type { OrgSnapshot } from '$lib/proto';
	import { membersApi } from '../../api';
	import PresenceBadge from '../../components/PresenceBadge.svelte';
	import { ageText, headerPresence, kindWord, rowPresence } from '../../present';
	import type { MemberProfile } from '../../types';
	import AssignTaskDialog from '../assign-task/AssignTaskDialog.svelte';
	import RunNowDialog from '../run-now/RunNowDialog.svelte';
	import { setProfileContext, type ProfileAction } from './context.svelte';
	import { TAB_LABEL, profileBase, tabHref, tabOf, tabsFor } from './tabs';

	let {
		organizationId,
		organizationName,
		memberId,
		snapshot,
		children: tabContent
	}: {
		organizationId: string;
		organizationName: string;
		memberId: string;
		/** The organization's structure, for the shared Edit, Move and Offboard dialogs. */
		snapshot: OrgSnapshot;
		children: Snippet;
	} = $props();

	// Not found is a state of its own (no Retry): the read answers null for it.
	const profile = new Resource(() =>
		membersApi.profile(organizationId, memberId).catch((e) => {
			if (toAppError(e).code === 'not_found') return null;
			throw e;
		})
	);
	const session = $derived(sessionFor(orgApi, organizationId));
	const record = $derived(snapshot.members.find((m) => m.id === memberId));
	let changes = $state(0);

	const tab = $derived(tabOf(page.url.pathname, memberId));
	/** A tab this member has: a person on an agent-only URL gets no tab named in the title or heading. */
	const hasTab = $derived(!!profile.data && tabsFor(profile.data).includes(tab));
	const title = $derived.by(() => {
		if (profile.data === null) return pageTitle('Member not found', 'Teams');
		const name = profile.data?.name ?? 'Member';
		return tab === 'overview' || !hasTab
			? pageTitle(name, 'Teams')
			: pageTitle(TAB_LABEL[tab], name, 'Teams');
	});

	// A tab change is a view switch: focus its heading (offices-and-teams-ux §7.2). The router resets focus and the
	// shell then focuses the top bar's h1 a tick later, so this runs after both, in the next task.
	let tabHeading = $state<HTMLElement | null>(null);
	afterNavigate((move) => {
		if (move.type !== 'link' && move.type !== 'popstate') return;
		const from = move.from?.url.pathname;
		const to = move.to?.url.pathname;
		const base = profileBase(memberId);
		const inside = (path: string | undefined) =>
			!!path && (path === base || path.startsWith(`${base}/`)) && !path.startsWith(`${base}/runs/`);
		if (from === to || !inside(from) || !inside(to)) return;
		setTimeout(() => tabHeading?.focus({ preventScroll: true }), 0);
	});

	let dialog = $state<ProfileAction | undefined>();
	let opener: HTMLElement | null = null;
	let moreButton = $state<HTMLButtonElement | null>(null);

	/**
	 * What a lifecycle confirmation acts on, fixed when it opens: the version it was shown and its preview lines, so the
	 * dialog never rewrites itself while the change and the reload after it run. The preview names the owner rather
	 * than "you": the reader may be the administrator.
	 */
	let shown = $state.raw<{ version?: number; lines: string[] }>({ lines: [] });
	const freeze = () =>
		(shown = {
			version: record?.version,
			lines: record && record.kind === 'agent' ? offboardSummary(snapshot, record, false) : []
		});

	function act(action: ProfileAction, from?: HTMLElement | null) {
		opener = from ?? moreButton;
		if (action === 'resume') void resume();
		else {
			if (action === 'pause' || action === 'offboard') freeze();
			dialog = action;
		}
	}

	let nameHeading = $state<HTMLElement | null>(null);

	/**
	 * Focus goes back to what opened the dialog; if that control is gone (Resume became Pause…, More went away after
	 * Offboard), to the control that replaced it, else the More button, else the profile's heading.
	 */
	function refocus() {
		const target =
			(opener?.isConnected && opener) ||
			document.querySelector<HTMLElement>('[data-lifecycle-control]') ||
			(moreButton?.isConnected && moreButton) ||
			nameHeading;
		target?.focus();
	}
	function restore(event: Event) {
		event.preventDefault();
		refocus();
	}

	/** After a confirmed pause or offboard: reload once the dialog has closed, then put focus back. */
	let reloadOnClose = false;
	async function restoreAfterChange(event: Event) {
		event.preventDefault();
		if (reloadOnClose) {
			reloadOnClose = false;
			await changed();
			await tick();
		}
		refocus();
	}

	async function changed() {
		await invalidate('app:org');
		await profile.reload();
		changes++;
	}

	async function resume() {
		const p = profile.data;
		if (!p) return;
		try {
			await setAgentStatus(session, p.id, 'active', record?.version);
			const other = (p.supervision?.holds ?? []).filter((h) => !/manual/i.test(h));
			if (other.length)
				toast.warning(`Resumed ${p.name}, but it still can't start.`, {
					description: other.join(' · ')
				});
			else toast.success(`Resumed ${p.name}.`);
		} catch (e) {
			toast.error(`Couldn't resume ${p.name}`, { description: toAppError(e).message });
		}
		await changed();
		await tick();
		refocus();
	}

	/** HQ refused a stale version: show the latest, so a retry acts on what the person now sees (§9). */
	async function refreshOnConflict(e: unknown, name: string): Promise<never> {
		if (toAppError(e).code === 'conflict') {
			await changed();
			freeze();
			throw new Error(
				`${name} changed at HQ while you were looking. Here's the latest; try again.`,
				{
					cause: e
				}
			);
		}
		throw e;
	}

	async function pause() {
		const p = profile.data;
		if (!p) return;
		await setAgentStatus(session, p.id, 'paused', shown.version).catch((e) =>
			refreshOnConflict(e, p.name)
		);
		toast.success(`Pause requested for ${p.name}. Stop pending until the office confirms it.`);
		reloadOnClose = true;
	}

	async function offboard() {
		const p = profile.data;
		if (!p) return;
		await offboardAgent(session, p.id, shown.version).catch((e) => refreshOnConflict(e, p.name));
		toast.success(`Offboarded ${p.name}. The record and handle stay.`);
		reloadOnClose = true;
	}

	const open = (action: ProfileAction) => ({
		get value() {
			return dialog === action;
		},
		set value(next: boolean) {
			dialog = next ? action : dialog === action ? undefined : dialog;
		}
	});
	const runOpen = open('run');
	const assignOpen = open('assign');
	const pauseOpen = open('pause');
	const offboardOpen = open('offboard');
	const editOpen = open('edit');
	const moveOpen = open('move');

	setProfileContext({
		get organizationId() {
			return organizationId;
		},
		get organizationName() {
			return organizationName;
		},
		get profile() {
			// Tabs render only once a profile has loaded.
			return profile.data as MemberProfile;
		},
		get snapshot() {
			return snapshot;
		},
		get changes() {
			return changes;
		},
		act,
		changed
	});

	const linkClass =
		'rounded-sm text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400';
</script>

<svelte:head><title>{title}</title></svelte:head>

<Loaded
	resource={profile}
	label="profile"
	error="Couldn't load this member"
	place="Member"
	area="Teams"
	avatar
	rows={3}
	isEmpty={(value) => value === null}
	emptyLevel={2}
	emptyTitle="Member not found"
	emptyDescription="Nobody with this link is in this organization. They may have been removed, or the link is mistyped."
>
	{#snippet emptyAction()}
		<a href={resolve('/teams/members')} class="text-sm/6 font-semibold {linkClass}">Open Members</a>
	{/snippet}
	{#snippet children(found)}
		{@const p = found!}
		{@const presence = p.kind === 'agent' ? headerPresence(p) : rowPresence(p)}
		{@const active = p.status !== 'inactive'}
		{@const manage = p.viewer.isOwnerOrAdmin && active && !!record}
		<header
			class="border-b border-white/5 bg-black/10 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<a
				href={resolve('/teams/members')}
				class="mb-3 inline-flex min-h-6 items-center text-sm/6 font-semibold {linkClass}">Members</a
			>
			<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-4">
				<div class="flex min-w-0 flex-1 basis-72 gap-x-4">
					<Avatar name={p.name} class="size-10" />
					<div class="min-w-0 flex-1">
						<h2
							bind:this={nameHeading}
							tabindex="-1"
							class="flex flex-wrap items-center gap-x-2 gap-y-1 text-base/7 font-semibold text-white [overflow-wrap:anywhere]"
						>
							{p.name}{p.viewer.isSelf ? ' (you)' : ''}
							{#if p.owner}<StatusPill
									label="Owner"
									class="bg-indigo-400/10 text-indigo-300 ring-indigo-400/30"
								/>{/if}
						</h2>
						<p class="text-sm/6 text-gray-300 [overflow-wrap:anywhere]">
							{#if p.role}{p.role} <span aria-hidden="true">·</span>{/if}
							<span class="font-mono text-gray-400">@{p.handle}</span>
						</p>
						<p class="mt-1 flex flex-wrap gap-x-2 text-xs/5 text-gray-400">
							<span>{kindWord(p.kind)}</span>
							<span aria-hidden="true">·</span>
							{#if p.homeTeam}
								<span
									>Home team <a
										href={resolve(`/teams/${encodeURIComponent(p.homeTeam.id)}` as '/')}
										class={linkClass}>{p.homeTeam.name}</a
									>{p.homeTeam.ref ? ` · ${p.homeTeam.ref}` : ''}</span
								>
							{:else}
								{#if active}
									<span class="text-red-300">No home team on record: this record is invalid</span>
								{:else}
									<span>Former member · no team</span>
								{/if}
							{/if}
							<span aria-hidden="true">·</span>
							{#if p.reportsTo}
								<span
									>Reports to <a
										href={resolve(`/members/${encodeURIComponent(p.reportsTo.id)}` as '/')}
										class={linkClass}>{p.reportsTo.name}</a
									></span
								>
							{:else}
								<span>No explicit manager</span>
							{/if}
						</p>
						<p class="text-xs/5 text-gray-400 [overflow-wrap:anywhere]">
							{#if p.kind === 'agent' && active}
								{#if p.office}Runs at {p.office.name} · reported {ageText(
										p.office.observedAt
									)}{:else}Unplaced{/if}
							{:else if p.contact}{p.contact}{/if}
						</p>
						{#if presence || !active || p.canSignIn}
							<div class="mt-2 flex flex-wrap gap-2">
								{#if !active}
									<StatusPill label="Former member" />
								{:else if presence}
									<PresenceBadge {presence} />
								{/if}
								{#if p.canSignIn}<StatusPill label="Can sign in" />{/if}
							</div>
						{/if}
					</div>
				</div>
				{#if active}
					<div class="flex flex-wrap items-center gap-3">
						{#if p.kind === 'agent'}
							<Button
								variant="outline"
								class={mobileButton}
								onclick={(e) => act('run', e.currentTarget)}>Run now</Button
							>
						{/if}
						<Button
							variant="outline"
							class={mobileButton}
							onclick={(e) => act('assign', e.currentTarget)}>Assign task</Button
						>
						{#if manage}
							<DropdownMenu>
								<DropdownMenuTrigger>
									{#snippet child({ props })}
										<Button {...props} bind:ref={moreButton} variant="outline" class={mobileButton}
											>More<span class="sr-only"> actions for {p.name}</span></Button
										>
									{/snippet}
								</DropdownMenuTrigger>
								<DropdownMenuContent align="end">
									{#if p.kind === 'agent'}
										{#if p.status === 'paused'}
											<DropdownMenuItem onSelect={() => act('resume')}>Resume</DropdownMenuItem>
										{:else}
											<DropdownMenuItem onSelect={() => act('pause')}>Pause…</DropdownMenuItem>
										{/if}
									{/if}
									<DropdownMenuItem onSelect={() => act('edit')}>Edit…</DropdownMenuItem>
									<DropdownMenuItem onSelect={() => act('move')}>Move home team…</DropdownMenuItem>
									{#if p.kind === 'agent'}
										<DropdownMenuSeparator />
										<DropdownMenuItem variant="destructive" onSelect={() => act('offboard')}
											>Offboard…</DropdownMenuItem
										>
									{/if}
								</DropdownMenuContent>
							</DropdownMenu>
						{/if}
					</div>
				{/if}
			</div>
		</header>
		<TabsNav
			label="{p.name}'s profile"
			tabs={tabsFor(p).map((t) => ({ label: TAB_LABEL[t], href: tabHref(p.id, t) }))}
		/>
		<h3 bind:this={tabHeading} tabindex="-1" class="sr-only">
			{hasTab ? TAB_LABEL[tab] : `No such tab for ${p.name}`}
		</h3>
		{@render tabContent()}

		{#if p.kind === 'agent'}
			<RunNowDialog
				bind:open={runOpen.value}
				{organizationId}
				profile={p}
				onCloseAutoFocus={restore}
				onrefused={() => void changed()}
				onstarted={() => {
					toast.success(`Started a turn for ${p.name}.`);
					void changed();
				}}
			/>
		{/if}
		<AssignTaskDialog
			bind:open={assignOpen.value}
			{organizationId}
			profile={p}
			onCloseAutoFocus={restore}
			onassigned={(task) => {
				toast.success(`Assigned ${task.ref} to ${p.name}.`);
				void changed();
			}}
		/>
		{#if record}
			<ConfirmDialog
				bind:open={pauseOpen.value}
				title="Pause {p.name}?"
				description="HQ will record the request. {p.name} stays in {p.homeTeam?.name ??
					'its home team'}; new admissions stop when the authoritative control is applied."
				confirmLabel="Pause"
				busyLabel="Pausing…"
				onconfirm={pause}
				onCloseAutoFocus={restoreAfterChange}
			/>
			<ConfirmDialog
				bind:open={offboardOpen.value}
				title="Offboard {p.name}?"
				description="{p.name} becomes a former member. This is what changes:"
				confirmLabel="Offboard"
				busyLabel="Offboarding…"
				destructive
				onconfirm={offboard}
				onCloseAutoFocus={restoreAfterChange}
			>
				{#snippet body()}
					<ul role="list" class="list-disc space-y-1 pl-5">
						{#each shown.lines as line, i (i)}<li>
								{line}
							</li>{/each}
					</ul>
				{/snippet}
			</ConfirmDialog>
			{#if p.kind === 'agent'}
				<EditAgentDialog
					bind:open={editOpen.value}
					agent={record}
					{snapshot}
					{session}
					ondone={(r) => {
						if (!r.stale) toast.success(`Saved ${r.name}.`);
						void changed();
					}}
				/>
			{:else}
				<EditPersonDialog
					bind:open={editOpen.value}
					person={record}
					{snapshot}
					{session}
					organization={organizationName}
					ondone={(r) => {
						if (!r.stale) toast.success(`Saved ${r.name}.`);
						void changed();
					}}
				/>
			{/if}
			<MoveMemberDialog
				bind:open={moveOpen.value}
				member={record}
				{snapshot}
				{session}
				ondone={(r) => {
					if ('name' in r) toast.success(`Moved ${r.name} to ${r.to}.`);
					void changed();
				}}
			/>
		{/if}
	{/snippet}
</Loaded>
