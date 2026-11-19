<!--
  @component
  The colleague page's frame (our-org-in-the-app §5, AC-IA06): one layout for people and agents. The header shows the
  name, role and handle, Agent or Person in words, the one Home team and the explicit Reports to; an agent has one
  truthful presence signal and one context line (its office and how fresh that is, or Unplaced). Below 640 px the
  avatar sits above the name, so the text column keeps the full width (app-shell-ux §2); facts wrap without a leading
  separator and the presence pill wraps whole words. Actions: Run now and Assign task, and More: Pause or Resume and
  Offboard for agents, Edit and Move home team for everyone. Edit carries identity only (PROF-4). Then the link tabs,
  each a URL (a Section menu below 640 px), and the current tab. The tabs read the profile from the context.
-->
<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import Avatar from '$lib/components/Avatar.svelte';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
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
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/office/api';
	import { offboardAgent, setAgentStatus } from '$lib/org/actions';
	import { offboardSummary } from '$lib/org/agents';
	import { orgApi } from '$lib/org/api';
	import { sessionFor } from '$lib/org/session';
	import type { OrgSnapshot } from '$lib/proto';
	import { membersApi } from '../../api';
	import PresenceBadge from '../../components/PresenceBadge.svelte';
	import { headerPresence, kindWord, reportedText, rowPresence } from '../../present';
	import type { MemberProfile } from '../../types';
	import AssignTaskDialog from '../assign-task/AssignTaskDialog.svelte';
	import RunNowDialog from '../run-now/RunNowDialog.svelte';
	import { runNowNotice } from '../run-now/outcome';
	import { setProfileContext, type ProfileAction } from './context.svelte';
	import { fact, factsList, factsWrap } from '../../components/facts';
	import { FIRST_POLL_MS, LatestRead, awaitingOffice, nextPollDelay } from './pending';
	import IdentityDialog from './IdentityDialog.svelte';
	import { offboardPreview } from './identity';
	import MoveHomeTeamDialog from './MoveHomeTeamDialog.svelte';
	import ProfileSkeleton from './ProfileSkeleton.svelte';
	import SectionMenu from './SectionMenu.svelte';
	import { tabStrip, tabStripFade } from './tab-strip';
	import { TAB_LABEL, profileBase, tabHref, tabOf, tabsFor, type ProfileTab } from './tabs';
	import type { RunNowResult } from '../../types';

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
	// Every read of the profile is numbered, so an older answer arriving late never replaces a newer one (the page's
	// load and reloads, the pending-turn checks and Run now's answer).
	const reads = new LatestRead();
	const profile: Resource<MemberProfile | null> = new Resource(async () => {
		const n = reads.start();
		const value = await membersApi.profile(organizationId, memberId).catch((e) => {
			if (toAppError(e).code === 'not_found') return null;
			throw e;
		});
		// Superseded: keep showing the newer profile.
		return reads.accept(n) ? value : (profile.data ?? value);
	});
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
			lines:
				record && record.kind === 'agent'
					? offboardPreview(
							offboardSummary(snapshot, record, false),
							record,
							record.agent?.messaging_name ?? ''
						)
					: []
		});

	/** Edit, Move and Run now start afresh each time they open: a new key mounts a fresh dialog. */
	let opening = $state(0);

	function act(action: ProfileAction, from?: HTMLElement | null) {
		opener = from ?? moreButton;
		if (action === 'resume') void resume();
		else {
			if (action === 'pause' || action === 'offboard') freeze();
			if (action === 'edit' || action === 'move' || action === 'run') opening++;
			dialog = action;
		}
	}

	/** Run now answered: show the profile it returned (presence, Health, the open turn) and say only what is confirmed. */
	function runRequested(result: RunNowResult) {
		reads.now();
		profile.data = result.profile;
		changes++;
		const notice = runNowNotice(result.turn, result.profile.name);
		// The shared toaster draws descriptions too dim on this surface; this one carries what happens next.
		const options = { description: notice.description, descriptionClass: '!text-gray-300' };
		if (notice.tone === 'success') toast.success(notice.title, options);
		else toast.info(notice.title, options);
	}

	// A requested turn waits for the office: check the profile again, backing off, until it starts or goes away. A
	// check is its own read: it shows its answer only when it succeeds and is the newest, and a failed one leaves the
	// page, its dialogs and what was typed alone, says so quietly and checks again later. The timer is cleared when
	// the profile changes, after each check, and when the page goes away.
	let pollDelay = FIRST_POLL_MS;
	let checks = $state(0);
	let refreshFailed = $state(false);
	async function checkPending() {
		const n = reads.start();
		try {
			const value = await membersApi.profile(organizationId, memberId);
			refreshFailed = false;
			if (reads.accept(n)) {
				profile.data = value;
				changes++;
			}
		} catch {
			refreshFailed = true;
		}
	}
	$effect(() => {
		void checks;
		if (!awaitingOffice(profile.data)) {
			pollDelay = FIRST_POLL_MS;
			refreshFailed = false;
			return;
		}
		const timer = setTimeout(async () => {
			pollDelay = nextPollDelay(pollDelay);
			await checkPending();
			checks++;
		}, pollDelay);
		return () => clearTimeout(timer);
	});

	/** The Section menu below 640 px: open the tab's URL, then move focus to its heading as a tab link does. */
	async function chooseSection(id: string) {
		await goto(resolve(tabHref(memberId, id as ProfileTab) as '/'));
		setTimeout(() => tabHeading?.focus({ preventScroll: true }), 0);
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

{#if profile.state === 'loading'}
	<ProfileSkeleton />
{:else}
	<Loaded
		resource={profile}
		label="profile"
		error="Couldn't load this member"
		place="Member"
		area="Teams"
		isEmpty={(value) => value === null}
		emptyLevel={2}
		emptyTitle="Member not found"
		emptyDescription="Nobody with this link is in this organization. They may have been removed, or the link is mistyped."
	>
		{#snippet emptyAction()}
			<a href={resolve('/teams/members')} class="text-sm/6 font-semibold {linkClass}"
				>Open Members</a
			>
		{/snippet}
		{#snippet children(found)}
			{@const p = found!}
			{@const presence = p.kind === 'agent' ? headerPresence(p) : rowPresence(p)}
			{@const active = p.status !== 'inactive'}
			{@const manage = p.viewer.isOwnerOrAdmin && active && !!record}
			{@const tabs = tabsFor(p)}
			<header
				class="border-b border-white/5 bg-black/10 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8"
			>
				<a
					href={resolve('/teams/members')}
					class="mb-3 inline-flex min-h-6 items-center text-sm/6 font-semibold max-[639px]:min-h-11 {linkClass}"
					>Members</a
				>
				<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-4">
					<div
						class="flex min-w-0 flex-1 basis-72 flex-col gap-3 min-[640px]:flex-row min-[640px]:gap-x-4"
					>
						<Avatar name={p.name} class="size-10 shrink-0" />
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
							<div class={factsWrap}>
								<ul role="list" class="{factsList} text-sm/6 text-gray-300">
									{#if p.role}<li class={fact}>{p.role}</li>{/if}
									<li class="{fact} font-mono text-gray-400">@{p.handle}</li>
								</ul>
							</div>
							<div class="mt-1 {factsWrap}">
								<ul role="list" class="{factsList} text-xs/5 text-gray-400">
									<li class={fact}>{kindWord(p.kind)}</li>
									{#if p.homeTeam}
										<li class={fact}>
											Home team <a
												href={resolve(`/teams/${encodeURIComponent(p.homeTeam.id)}` as '/')}
												class={linkClass}>{p.homeTeam.name}</a
											>{#if p.homeTeam.ref}<span class="whitespace-nowrap"
													>{` · ${p.homeTeam.ref}`}</span
												>{/if}
										</li>
									{:else if active}
										<li class="{fact} text-red-300">
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
									<ul role="list" class="{factsList} text-xs/5 text-gray-400">
										{#if p.office}
											<li class={fact}>Runs at {p.office.name}</li>
											<li class={fact}>{reportedText(p.office.observedAt)}</li>
										{:else}
											<li class={fact}>Unplaced</li>
										{/if}
									</ul>
								</div>
							{:else if p.contact}
								<p class="text-xs/5 text-gray-400 [overflow-wrap:anywhere]">{p.contact}</p>
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
							<p class={['text-xs/5 text-gray-400', refreshFailed && 'mt-1']} role="status">
								{refreshFailed ? "Couldn't refresh; checking again." : ''}
							</p>
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
											<Button
												{...props}
												bind:ref={moreButton}
												variant="outline"
												class={mobileButton}
												>More<span class="sr-only"> actions for {p.name}</span></Button
											>
										{/snippet}
									</DropdownMenuTrigger>
									<DropdownMenuContent align="end">
										{#if p.kind === 'agent'}
											{#if p.status === 'paused'}
												<DropdownMenuItem
													class="max-[639px]:min-h-11"
													onSelect={() => act('resume')}>Resume</DropdownMenuItem
												>
											{:else}
												<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => act('pause')}
													>Pause…</DropdownMenuItem
												>
											{/if}
										{/if}
										<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => act('edit')}
											>Edit…</DropdownMenuItem
										>
										{#if p.homeTeam}
											<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => act('move')}
												>Move home team…</DropdownMenuItem
											>
										{/if}
										{#if p.kind === 'agent'}
											<DropdownMenuSeparator />
											<DropdownMenuItem
												variant="destructive"
												class="max-[639px]:min-h-11"
												onSelect={() => act('offboard')}>Offboard…</DropdownMenuItem
											>
										{/if}
									</DropdownMenuContent>
								</DropdownMenu>
							{/if}
						</div>
					{/if}
				</div>
			</header>
			<div class="relative max-[639px]:hidden {tabStripFade}" {@attach tabStrip(page.url.pathname)}>
				<TabsNav
					label="{p.name}'s profile"
					tabs={tabs.map((t) => ({ label: TAB_LABEL[t], href: tabHref(p.id, t) }))}
				/>
			</div>
			<SectionMenu
				class="min-[640px]:hidden"
				label="{p.name}'s profile"
				current={hasTab ? tab : ''}
				sections={tabs.map((t) => ({ id: t, label: TAB_LABEL[t] }))}
				onchoose={(id) => void chooseSection(id)}
			/>
			<h3 bind:this={tabHeading} tabindex="-1" class="sr-only">
				{hasTab ? TAB_LABEL[tab] : `No such tab for ${p.name}`}
			</h3>
			{@render tabContent()}

			{#if p.kind === 'agent'}
				{#key opening}<RunNowDialog
						bind:open={runOpen.value}
						{organizationId}
						profile={p}
						onCloseAutoFocus={restore}
						onrefused={() => void changed()}
						onstarted={runRequested}
					/>{/key}
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
				{#key opening}
					<IdentityDialog
						bind:open={editOpen.value}
						member={record}
						{snapshot}
						{session}
						organization={organizationName}
						onCloseAutoFocus={restore}
						ondone={(r) => {
							if (!r.stale) toast.success(`Saved ${r.name}.`);
							void changed();
						}}
					/>
					{#if p.homeTeam}
						<MoveHomeTeamDialog
							bind:open={moveOpen.value}
							{organizationId}
							member={{ id: p.id, name: p.name }}
							fromTeamId={p.homeTeam.id}
							{snapshot}
							onCloseAutoFocus={restore}
							ondone={(r) => {
								if ('name' in r) toast.success(`Moved ${r.name} to ${r.to}.`);
								void changed();
							}}
						/>
					{/if}
				{/key}
			{/if}
		{/snippet}
	</Loaded>
{/if}
