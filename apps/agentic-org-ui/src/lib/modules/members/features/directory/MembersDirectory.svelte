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
	import AddPersonSheet from '$lib/components/AddPersonSheet.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { orgApi } from '$lib/org/api';
	import { withoutFormer } from '$lib/org/model';
	import { sessionFor } from '$lib/org/session';
	import type { OrgSnapshot } from '$lib/proto';
	import { membersApi } from '../../api';
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
	const hireHref = resolve('/teams/hire' as '/');

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
			class={mobileButton}
			onclick={() => (adding = true)}
		>
			<MiniIcon name="plus" class="-ml-0.5 size-5" /> Add a person
		</Button>{/if}
{/snippet}

{#snippet hireAgent()}
	<Button variant="outline" class={mobileButton} href={hireHref}>Hire an agent</Button>
{/snippet}

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
	<fieldset class="flex flex-wrap items-center gap-2 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8">
		<legend class="sr-only">Show members</legend>
		{#each DIRECTORY_FILTERS as f (f.id)}
			<label
				class="inline-flex min-h-9 cursor-pointer items-center gap-x-1.5 rounded-md px-3 text-sm/6 font-semibold ring-1 ring-inset has-checked:bg-surface-overlay has-checked:text-fg has-checked:ring-line-strong has-checked:underline has-checked:underline-offset-4 has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-focus max-[639px]:min-h-11 {filter ===
				f.id
					? ''
					: 'text-fg-muted ring-line hover:text-fg'}"
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
		<div class="@container border-t border-line-subtle" data-testid="members-list">
			<div class={GRID}>
				<div
					aria-hidden="true"
					class="hidden border-b border-line px-4 py-3 text-xs/5 font-semibold text-fg-muted min-[640px]:px-6 min-[1024px]:px-8 {SUBGRID}"
				>
					<span class="col-span-2">Member</span><span>Home team</span><span>Working on</span><span
						>Presence</span
					>
				</div>
				<ul
					role="list"
					class="divide-y divide-line-subtle border-b border-line-subtle {SUBGRID}"
					aria-label="Members"
				>
					{#each view.current as member (member.id)}
						<DirectoryRow {member} highlight={justAdded === member.id} />
					{/each}
				</ul>
			</div>
		</div>
	{/if}

	{#if view.former.length > 0}
		<details class="group border-b border-line-subtle" data-testid="former-members">
			<summary
				class="flex min-h-6 cursor-pointer list-none items-center gap-x-1 px-4 py-4 text-sm/6 font-semibold text-fg focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 min-[640px]:px-6 min-[1024px]:px-8 [&::-webkit-details-marker]:hidden"
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
				Former members <span class="ml-1 font-normal text-fg-muted">{view.former.length}</span>
			</summary>
			<p class="-mt-2 px-4 pb-3 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				Their profiles, handles and history stay.
			</p>
			<div class="@container border-t border-line-subtle">
				<div class={GRID}>
					<ul role="list" class="divide-y divide-line-subtle {SUBGRID}" aria-label="Former members">
						{#each view.former as member (member.id)}
							<DirectoryRow {member} />
						{/each}
					</ul>
				</div>
			</div>
		</details>
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
