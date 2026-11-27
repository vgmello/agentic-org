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
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/proto';
	import { offboardAgent, removePerson, setAgentStatus } from '$lib/modules/organization/actions';
	import { offboardSummary } from '$lib/modules/organization/agents';
	import { orgApi } from '$lib/modules/organization/api';
	import { removePersonSummary } from '$lib/modules/organization/people-edit';
	import { sessionFor } from '$lib/modules/organization/session';
	import type { OrgSnapshot } from '$lib/proto';
	import { membersApi } from '../../api';
	import type { MemberProfile, RunNowResult } from '../../types';
	import { runNowNotice } from '../run-now/outcome';
	import { setProfileContext, type ProfileAction } from './context.svelte';
	import { FIRST_POLL_MS, LatestRead, awaitingOffice, nextPollDelay } from './pending';
	import AgentAnalysisOverlay from './AgentAnalysisOverlay.svelte';
	import AgentOrbRail from './AgentOrbRail.svelte';
	import { offboardPreview } from './identity';
	import ProfileActionDialogs from './ProfileActionDialogs.svelte';
	import ProfileHeader from './ProfileHeader.svelte';
	import ProfileSkeleton from './ProfileSkeleton.svelte';
	import { TAB_LABEL, profileBase, tabHref, tabOf, tabsFor, type ProfileTab } from './tabs';

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
	let analysisOpen = $state(false);
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
					: record
						? removePersonSummary(snapshot, record, organizationName)
						: []
		});

	/** Edit, Move and Run now start afresh each time they open: a new key mounts a fresh dialog. */
	let opening = $state(0);

	function act(action: ProfileAction, from?: HTMLElement | null) {
		opener = from ?? moreButton;
		if (action === 'resume') void resume();
		else {
			if (action === 'pause' || action === 'offboard' || action === 'remove') freeze();
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
		const options = { description: notice.description, descriptionClass: '!text-fg-secondary' };
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
	 * Focus goes back to what opened the dialog; if that control is gone or hidden after a responsive resize
	 * (Resume became Pause…, inline Pause collapsed into More, More went away after Offboard), fall through to the
	 * next visible focusable control, then the profile heading.
	 */
	function refocus() {
		const candidates: (HTMLElement | null | undefined)[] = [
			opener,
			document.querySelector<HTMLElement>('[data-lifecycle-control]'),
			moreButton,
			nameHeading
		];
		for (const candidate of candidates) {
			if (!candidate?.isConnected) continue;
			if (candidate !== nameHeading && candidate.offsetParent === null) continue;
			candidate.focus();
			if (document.activeElement === candidate) return;
		}
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

	async function remove() {
		const p = profile.data;
		if (!p) return;
		await removePerson(session, p.id, shown.version).catch((e) => refreshOnConflict(e, p.name));
		toast.success(`Removed ${p.name}. The record and handle stay.`);
		reloadOnClose = true;
	}

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
		'rounded-sm text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
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
			<ProfileHeader
				profile={p}
				{record}
				{tab}
				{hasTab}
				{refreshFailed}
				bind:nameHeading
				bind:moreButton
				onAct={act}
				onOpenAnalysis={() => (analysisOpen = true)}
				onChooseSection={(id) => void chooseSection(id)}
			/>
			<h3 bind:this={tabHeading} tabindex="-1" class="sr-only">
				{hasTab ? TAB_LABEL[tab] : `No such tab for ${p.name}`}
			</h3>
			{#if p.kind === 'agent'}
				<div class="agent-shell" data-testid="agent-profile-shell">
					<div class="agent-main min-w-0">
						{@render tabContent()}
					</div>
					<AgentOrbRail profile={p} onOpenAnalysis={() => (analysisOpen = true)} />
				</div>
				<AgentAnalysisOverlay
					bind:open={analysisOpen}
					{organizationId}
					profile={p}
					onstarted={runRequested}
				/>
			{:else}
				{@render tabContent()}
			{/if}

			<ProfileActionDialogs
				bind:dialog
				{opening}
				{organizationId}
				{organizationName}
				profile={p}
				{record}
				{snapshot}
				{session}
				shownLines={shown.lines}
				onRunRequested={runRequested}
				onChanged={changed}
				onPause={pause}
				onOffboard={offboard}
				onRemove={remove}
				onRestore={restore}
				onRestoreAfterChange={restoreAfterChange}
			/>
		{/snippet}
	</Loaded>
{/if}

<style>
	.agent-shell {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		align-items: start;
	}
	@media (min-width: 1024px) {
		.agent-shell {
			grid-template-columns: minmax(0, 1fr) 18.5rem;
		}
	}
</style>
