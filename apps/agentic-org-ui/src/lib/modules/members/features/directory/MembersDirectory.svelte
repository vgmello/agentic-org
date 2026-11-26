<!--
  @component
  Teams › Members (offices-and-teams-ux §7.2, PEOPLE-1, PEOPLE-2): the mixed directory of everyone in the organization,
  people and agents, filtered All · People · Agents. Each row opens the member's profile. Actions: Add a person (the
  organization's existing Add person flow) and Hire an agent (the Hire flow: agents come only from it,
  our-org-in-the-app §7). Former members are listed apart, below, with their records kept.
-->
<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import AddPersonSheet from '$lib/modules/organization/features/add-person/AddPersonSheet.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { orgApi } from '$lib/modules/organization/api';
	import { withoutFormer } from '$lib/modules/organization/model';
	import { sessionFor } from '$lib/modules/organization/session';
	import type { OrgSnapshot } from '$lib/proto';
	import { membersApi } from '../../api';
	import { rowPresence } from '../../present';
	import DirectoryRow from './DirectoryRow.svelte';
	import {
		DIRECTORY_FILTERS,
		directoryView,
		firstName,
		shownText,
		type DirectoryFilter
	} from './directory';

	let {
		organizationId,
		organizationName,
		snapshot,
		loads
	}: {
		organizationId: string;
		organizationName: string;
		/**
		 * The organization's structure, for the Add person flow's team and manager choices. `null` while the server
		 * doesn't serve it: Add a person is then left out rather than offered without its choices.
		 */
		snapshot: OrgSnapshot | null;
		/** The route's load count (`loadDirectory`): a change means the organization changed, so re-read the rows. */
		loads: number;
	} = $props();

	const members = new Resource(() => membersApi.directory(organizationId));
	// Changes made elsewhere (New › Add a person, a hire, a live office change) invalidate the route's org data; re-read
	// the rows then, keeping the shown ones until the new answer arrives (§7.1). The first load is the Resource's own.
	let seenLoad = untrack(() => loads);
	$effect(() => {
		const load = loads;
		if (load === seenLoad) return;
		seenLoad = load;
		// Synchronising with the server, not deriving: the read starts here and its answer lands in the Resource.
		untrack(() => void members.reload());
	});
	let destroyed = false;
	$effect(() => () => {
		destroyed = true;
	});
	const session = $derived(sessionFor(orgApi, organizationId));
	let filter = $state<DirectoryFilter>('all');
	let adding = $state(false);
	/**
	 * A person HQ accepted whose row hasn't been shown yet: focused and highlighted when it first appears (§7.4). Not
	 * state: the row arriving in the data is what wakes the effect below.
	 */
	let awaitingRow: string | undefined;
	/** Where focus was when HQ confirmed the add: the row takes focus only if the person hasn't moved it since. */
	let focusAtConfirm: Element | null = null;

	/**
	 * Focus may move to the new row only from where the add left it (WCAG 3.2.2): the element focused at confirmation,
	 * nothing (body), or the Add a person button the closing sheet returns focus to.
	 */
	function focusStillFromAdd(): boolean {
		const active = document.activeElement;
		return (
			!active ||
			active === document.body ||
			active === focusAtConfirm ||
			!!active.closest('[data-add-person]')
		);
	}
	/** The row tinted for two seconds after it appeared. */
	let justAdded = $state<string | undefined>();
	const view = $derived(directoryView(members.data ?? [], filter));
	const workforce = $derived.by(() => {
		const currentAll = (members.data ?? []).filter((r) => r.status !== 'inactive');
		let active = 0;
		let attention = 0;
		let idle = 0;
		for (const m of currentAll) {
			const p = rowPresence(m)?.id;
			if (p === 'working' || p === 'available' || p === 'with_you') active++;
			else if (p === 'needs_you' || p === 'waiting_answer') attention++;
			else if (p === 'paused' || p === 'offline') idle++;
			else if (m.kind === 'agent' && m.office === null) attention++;
		}
		return { active, attention, idle };
	});
	const hireHref = resolve('/teams/hire' as '/');
	const actionBtn = `min-h-9 ${mobileButton} [@media(pointer:coarse)]:min-h-11`;

	// When the list itself is at least 56rem wide the facts line up in columns: avatar · member · home team · working on
	// · presence. It is a container query in rem, so a narrow window or 200% text keeps the stacked rows. Rows are
	// subgrids of one grid, sharing the header's tracks; the presence pill wraps inside its 14rem.
	const GRID =
		'@min-[56rem]:grid @min-[56rem]:grid-cols-[auto_minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1.3fr)_fit-content(14rem)] @min-[56rem]:gap-x-5';
	const SUBGRID = '@min-[56rem]:col-span-full @min-[56rem]:grid @min-[56rem]:grid-cols-subgrid';

	// The new row is focused and highlighted when the data first shows it, whichever read brings it: a later read
	// (a live change arriving during the add) can supersede the one the add started. Effects render after the DOM
	// updates, so the link exists here; their cleanups stop the timers on unmount.
	$effect(() => {
		const shown = view.current;
		const id = awaitingRow;
		if (!id || !shown.some((m) => m.id === id)) return;
		awaitingRow = undefined;
		// Starting the two-second highlight is the event "the row appeared"; it has no source to derive from.
		justAdded = id;
		if (focusStillFromAdd())
			document.querySelector<HTMLElement>(`[data-member-link="${CSS.escape(id)}"]`)?.focus();
	});
	$effect(() => {
		const id = justAdded;
		if (!id) return;
		const timer = setTimeout(() => justAdded === id && (justAdded = undefined), 2000);
		return () => clearTimeout(timer);
	});

	/** After HQ accepted the new person: show the row once, highlight it for two seconds and focus it (§7.4). */
	async function added(r: { memberId: string; name: string; problems: string[] }) {
		if (filter === 'agents') filter = 'all';
		awaitingRow = r.memberId;
		// Give up if no read brings the row (it went away again, or HQ never served it): nothing jumps focus later.
		setTimeout(() => {
			if (awaitingRow === r.memberId) awaitingRow = undefined;
		}, 10_000);
		// The invalidation reloads the route and the effect above re-reads the rows. Read here only when the route
		// didn't reload and no read is already on its way.
		const before = seenLoad;
		await invalidate('app:org');
		if (destroyed) return;
		await tick();
		if (!destroyed && seenLoad === before && !members.reloading) void members.reload();
	}

	function confirmed(r: { memberId: string; name: string; problems: string[] }) {
		focusAtConfirm = document.activeElement;
		void added(r);
		// HQ accepted the person: that is the confirmed outcome the toast reports (§6a), whatever the reads do.
		if (r.problems.length === 0)
			toast.success(`Added ${r.name}. Agents can now reach ${firstName(r.name)}.`);
		else
			toast.warning(`Added ${r.name}, but not everything went through.`, {
				description: r.problems.join(' ')
			});
	}
</script>

{#snippet addPerson(primary: boolean)}
	{#if snapshot}<Button
			variant={primary ? 'default' : 'outline'}
			data-add-person
			class={actionBtn}
			onclick={() => (adding = true)}
		>
			<MiniIcon name="plus" class="-ml-0.5 size-5" /> Add a person
		</Button>{/if}
{/snippet}

{#snippet hireAgent()}
	<Button variant="outline" class={actionBtn} href={hireHref}>Hire an agent</Button>
{/snippet}

<div class="members-masthead">
	<PageHeader
		title="Members"
		count={members.data ? view.counts.all : undefined}
		countLabel="members"
		description="Everyone in {organizationName}: people and agents. Agents can reach every person added here. Adding a person gives no sign-in and no authority; a person gets an account only when invited to sign in."
	>
		{#snippet actions()}
			{@render hireAgent()}
			{@render addPerson(true)}
		{/snippet}
	</PageHeader>
</div>

<Loaded
	resource={members}
	label="members"
	error="Couldn't load the members"
	place="Members"
	area="Teams"
	avatar
	rows={6}
	isEmpty={(rows) => rows.length === 0}
	emptyTitle="No members yet"
	emptyDescription="Everyone in {organizationName} is listed here, people and agents. Its owner and the HR lead join when it's created; if no one shows, HQ hasn't sent the members yet."
>
	{#snippet emptyAction()}{@render addPerson(false)}{/snippet}

	{#if view.counts.all > 0}
		<section
			aria-label="Workforce summary"
			class="members-kpi-strip grid grid-cols-1 gap-3 px-4 pt-5 min-[640px]:grid-cols-2 min-[640px]:px-6 min-[1024px]:grid-cols-4 min-[1024px]:px-8"
		>
			<div class="members-kpi-tile flex flex-col gap-1">
				<span class="members-kpi-label">Workforce</span>
				<p class="members-kpi-num">{view.counts.all}</p>
				<p class="text-xs/5 text-fg-muted tabular-nums">
					{view.counts.people}
					{view.counts.people === 1 ? 'person' : 'people'} · {view.counts.agents}
					{view.counts.agents === 1 ? 'agent' : 'agents'}
				</p>
			</div>
			<div class="members-kpi-tile flex flex-col gap-1">
				<span class="members-kpi-label inline-flex items-center gap-1.5">
					<span class="m-dot dot-ok" aria-hidden="true"></span>Active now
				</span>
				<p class="members-kpi-num">{workforce.active}</p>
				<p class="text-xs/5 text-fg-muted">Working or available</p>
			</div>
			<div class="members-kpi-tile flex flex-col gap-1">
				<div class="flex items-center justify-between gap-2">
					<span class="members-kpi-label inline-flex items-center gap-1.5">
						<span class="m-dot dot-warn" aria-hidden="true"></span>Needs attention
					</span>
					{#if workforce.attention > 0}
						<span
							class="t-amber inline-flex items-center rounded-full px-2 py-0.5 text-xs/4 font-semibold"
							>Review</span
						>
					{/if}
				</div>
				<p class="members-kpi-num">{workforce.attention}</p>
				<p class="text-xs/5 text-fg-muted">Waiting or unplaced</p>
			</div>
			<div class="members-kpi-tile flex flex-col gap-1">
				<span class="members-kpi-label inline-flex items-center gap-1.5">
					<span class="m-dot dot-muted" aria-hidden="true"></span>Idle or paused
				</span>
				<p class="members-kpi-num">{workforce.idle}</p>
				<p class="text-xs/5 text-fg-muted">Asleep or paused</p>
			</div>
		</section>
	{/if}

	<div class="px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
		<fieldset class="exec-well inline-flex flex-wrap items-center gap-1 p-1">
			<legend class="sr-only">Show members</legend>
			{#each DIRECTORY_FILTERS as f (f.id)}
				<label
					class={[
						'dir-filter inline-flex min-h-9 cursor-pointer items-center gap-x-1.5 rounded-lg px-3 text-sm/6 font-semibold transition-colors has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-focus max-[639px]:min-h-11 [@media(pointer:coarse)]:min-h-11',
						filter === f.id ? 'is-on' : 'text-fg-muted hover:text-fg'
					]}
				>
					<input
						type="radio"
						name="members-filter"
						value={f.id}
						bind:group={filter}
						class="sr-only"
					/>
					{f.label}
					<span class="font-normal text-fg-muted tabular-nums">{view.counts[f.id]}</span>
				</label>
			{/each}
			<p class="sr-only" role="status">{shownText(view.current.length, filter)}</p>
		</fieldset>
	</div>

	{#if view.current.length === 0}
		<div
			class="border-t border-line-subtle px-4 py-8 min-[640px]:px-6 min-[1024px]:px-8"
			data-testid="members-filtered-empty"
		>
			{#if filter === 'agents'}
				<p class="text-sm/6 text-fg-muted">
					{view.former.length > 0
						? `No current agents in ${organizationName}.`
						: `No agents in ${organizationName} yet.`} An agent joins only through Hire.
				</p>
				<div class="mt-4">{@render hireAgent()}</div>
			{:else if filter === 'people'}
				<p class="text-sm/6 text-fg-muted">
					{view.former.length > 0
						? `No current people in ${organizationName}.`
						: `No people in ${organizationName} yet.`}
				</p>
				{#if snapshot}<div class="mt-4">{@render addPerson(false)}</div>{/if}
			{:else}
				<p class="text-sm/6 text-fg-muted">
					No current members: everyone listed here is a former member.
				</p>
				{#if snapshot}<div class="mt-4">{@render addPerson(false)}</div>{/if}
			{/if}
		</div>
	{:else}
		<div class="px-4 pb-5 min-[640px]:px-6 min-[1024px]:px-8">
			<div class="exec-card @container overflow-hidden" data-testid="members-list">
				<div class={GRID}>
					<div
						aria-hidden="true"
						class="hidden border-b border-line px-4 py-3 text-xs/5 font-semibold tracking-wider text-fg-muted uppercase min-[640px]:px-6 {SUBGRID}"
					>
						<span class="col-span-2">Member</span><span>Home team</span><span>Working on</span><span
							>Presence</span
						>
					</div>
					<ul role="list" class="divide-y divide-line-subtle {SUBGRID}" aria-label="Members">
						{#each view.current as member (member.id)}
							<DirectoryRow {member} highlight={justAdded === member.id} />
						{/each}
					</ul>
				</div>
			</div>
		</div>
	{/if}

	{#if view.former.length > 0}
		<div class="px-4 pb-6 min-[640px]:px-6 min-[1024px]:px-8">
			<details class="exec-card group overflow-hidden" data-testid="former-members">
				<summary
					class="flex min-h-9 cursor-pointer list-none items-center gap-x-1 px-4 py-3.5 text-sm/6 font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 min-[640px]:px-6 [@media(pointer:coarse)]:min-h-11 [&::-webkit-details-marker]:hidden"
				>
					<svg
						viewBox="0 0 20 20"
						fill="currentColor"
						aria-hidden="true"
						class="size-5 shrink-0 text-fg-muted transition-transform group-open:rotate-90 motion-reduce:transition-none"
						><path
							fill-rule="evenodd"
							d="M8.22 5.22a.75.75 0 0 1 1.06 0l4.25 4.25a.75.75 0 0 1 0 1.06l-4.25 4.25a.75.75 0 0 1-1.06-1.06L11.94 10 8.22 6.28a.75.75 0 0 1 0-1.06Z"
							clip-rule="evenodd"
						/></svg
					>
					Former members
					<span class="ml-1 font-normal text-fg-muted tabular-nums">{view.former.length}</span>
				</summary>
				<p class="-mt-1 px-4 pb-3 text-xs/5 text-fg-muted min-[640px]:px-6">
					Their profiles, handles and history stay.
				</p>
				<div class="@container border-t border-line-subtle">
					<div class={GRID}>
						<ul
							role="list"
							class="divide-y divide-line-subtle {SUBGRID}"
							aria-label="Former members"
						>
							{#each view.former as member (member.id)}
								<DirectoryRow {member} />
							{/each}
						</ul>
					</div>
				</div>
			</details>
		</div>
	{/if}
</Loaded>

{#if snapshot}
	<AddPersonSheet
		bind:open={adding}
		snapshot={withoutFormer(snapshot)}
		{session}
		organization={organizationName}
		ondone={confirmed}
	/>
{/if}

<style>
	.members-masthead :global(h2) {
		font-size: 1.25rem;
		line-height: 1.75rem;
		letter-spacing: -0.02em;
	}
	:global(html[data-ui-theme^='report']) .members-masthead :global(header) {
		border-bottom: 1.5px solid var(--report-masthead-rule);
	}
	.dir-filter.is-on {
		background: var(--exec-accent-soft);
		color: var(--exec-ink);
		box-shadow: inset 0 0 0 1px var(--exec-accent-ring);
	}
	.members-kpi-tile {
		background: var(--exec-panel-bg);
		-webkit-backdrop-filter: var(--exec-panel-blur);
		backdrop-filter: var(--exec-panel-blur);
		box-shadow: var(--exec-panel-shadow);
		border-radius: var(--exec-panel-radius);
		padding: 1rem 1.125rem;
		min-width: 0;
	}
	.members-kpi-label {
		font-size: 0.7rem;
		font-weight: 650;
		letter-spacing: 0.09em;
		text-transform: uppercase;
		color: var(--exec-muted);
	}
	.members-kpi-num {
		font-size: 1.5rem;
		line-height: 1.15;
		font-weight: 700;
		letter-spacing: -0.03em;
		color: var(--exec-ink);
		font-variant-numeric: tabular-nums;
	}
	.m-dot {
		width: 0.4375rem;
		height: 0.4375rem;
		border-radius: 999px;
		flex: none;
	}
	.dot-ok {
		background: var(--tone-ok);
	}
	.dot-warn {
		background: var(--tone-warn);
	}
	.dot-muted {
		background: var(--fg-muted);
	}
	:global(html[data-ui-theme^='report']) .members-kpi-strip {
		gap: 0;
		border-bottom: 1px solid var(--report-rule);
		padding-bottom: 0.75rem;
	}
	:global(html[data-ui-theme^='report']) .members-kpi-tile {
		padding: 0.625rem 0;
	}
	:global(html[data-ui-theme^='report']) .members-kpi-tile + .members-kpi-tile {
		border-top: 1px solid var(--report-rule-subtle);
	}
	@media (min-width: 1024px) {
		:global(html[data-ui-theme^='report']) .members-kpi-tile {
			padding: 0.5rem 1.25rem;
			border-top: 0;
			border-left: 1px solid var(--report-rule-subtle);
		}
		:global(html[data-ui-theme^='report']) .members-kpi-tile:first-child {
			border-left: 0;
			padding-left: 0;
		}
		:global(html[data-ui-theme^='report']) .members-kpi-tile + .members-kpi-tile {
			border-top: 0;
		}
	}
</style>
