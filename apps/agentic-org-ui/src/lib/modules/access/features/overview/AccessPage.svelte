<!--
  @component
  Organization settings › Members and access (server-installation-and-access-ux §4a, offices-and-teams-ux §5b, §7.1,
  §7.4): who can sign in to this organization and with what authority. The owner and the administrator are shown as
  facts; then the lists of people: Can sign in, Invited but not signed in yet, No account yet, and Deactivated when
  anyone is. Agents never appear. The owner and the administrator see the sign-in actions; everyone else sees the lists
  and why they have no actions.

  The page hosts the members module's sign-in dialogs once, so they survive a list reload. After an action focus goes
  back to the button that opened it, or, when the person moved to another list, to that person's first action there.
  A refusal is shown in the person's row, under the action; a person who has left is said in the list they were in,
  and focus moves to the next row (app-shell-ux §6a). The list re-reads when the organization changes (`loads`).
-->
<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import { Button } from '$lib/components/ui/button';
	import DescriptionList from '$lib/components/DescriptionList.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { focusHeadingAfterShell, focusIsFree, neighbourFocus } from '$lib/components/focus';
	import { Resource } from '$lib/components/resource.svelte';
	import { toAppError } from '$lib/proto';
	import {
		NOT_IN_ORGANIZATION,
		SignInAccessDialogs,
		type SignInAction,
		type SignInOutcome,
		type SignInPerson
	} from '$lib/modules/members';
	import { accessApi } from '../../api';
	import {
		MANAGERS_ONLY,
		groupPeople,
		listOf,
		visibleAdminUsername,
		visibleUsername,
		type AccessList
	} from '../../present';
	import type { AccessPerson } from '../../types';
	import AccessRow from './AccessRow.svelte';

	let {
		organizationId,
		loads
	}: {
		organizationId: string;
		/** The route's load count: a change means the organization changed (a hand-over, a new person), so re-read. */
		loads: number;
	} = $props();

	const resource = new Resource(() => accessApi.overview(organizationId));
	// Changes made elsewhere invalidate the route's org data; re-read then, keeping the shown lists until the new answer
	// arrives (offices-and-teams-ux §7.1: "both people's open pages show the change without a reload").
	let seenLoad = untrack(() => loads);
	$effect(() => {
		const load = loads;
		if (load === seenLoad) return;
		seenLoad = load;
		// The organization changed elsewhere: a message about the last action no longer describes the list.
		untrack(dismiss);
		// Synchronising with the server, not deriving: the read starts here and its answer lands in the Resource.
		untrack(() => void resource.reload());
	});

	let host = $state<ReturnType<typeof SignInAccessDialogs>>();
	/** The link being made for one person, so its button can say so. */
	let busy = $state<{ memberId: string; kind: 'invite' | 'reset' } | undefined>();
	/** The last refusal, shown in that person's row under the action. */
	let refusal = $state<{ memberId: string; message: string } | undefined>();
	/** A person who left since the list loaded: said in the list they were in. */
	let gone = $state<AccessList | undefined>();
	/** Set on teardown: a reload that outlives the page never moves focus on the next one. */
	let destroyed = false;
	$effect(() => () => {
		destroyed = true;
	});

	/** A refusal or a "has left" line is read once; the next interaction anywhere clears it. */
	function dismiss() {
		if (busy) return;
		refusal = undefined;
		gone = undefined;
	}

	// After a client navigation the shell focuses the area's h1 on the next tick; this page's heading takes focus after
	// that, so the person lands on what the page is for (app-shell-ux §6a).
	onMount(() => focusHeadingAfterShell('access-heading'));

	function asSignIn(person: AccessPerson): SignInPerson {
		return {
			id: person.memberId,
			name: person.name,
			owner: person.owner,
			canSignIn: person.state === 'signed_in',
			...(person.state === 'invited' && person.inviteExpiresAt
				? { invitePending: { expiresAt: person.inviteExpiresAt } }
				: {})
		};
	}

	async function act(person: AccessPerson, kind: SignInAction, event: MouseEvent) {
		const opener = event.currentTarget as HTMLElement;
		if (busy) {
			// One link at a time: point at the one being made rather than ignoring the press silently.
			document
				.querySelector<HTMLElement>(
					`[data-access-row][data-member-id="${CSS.escape(busy.memberId)}"] button[aria-busy="true"]`
				)
				?.focus();
			return;
		}
		refusal = undefined;
		gone = undefined;
		if (kind !== 'remove') busy = { memberId: person.memberId, kind };
		try {
			await host?.start(kind, asSignIn(person), opener);
		} catch (e) {
			const failure = toAppError(e);
			if (failure.code === 'not_found') await left(person.memberId, opener);
			else refusal = { memberId: person.memberId, message: failure.message };
		} finally {
			busy = undefined;
		}
	}

	/** The person has left since the list loaded: say so in their list, re-read, and focus the next row. */
	async function left(memberId: string, opener: HTMLElement | null) {
		const person = resource.data?.people.find((p) => p.memberId === memberId);
		gone = person ? listOf(person) : 'noAccount';
		const moveFocus = neighbourFocus(opener, headingTarget);
		await resource.reload();
		await tick();
		if (!destroyed && focusIsFree(opener)) await moveFocus();
	}

	/** The page heading, focusable as a place to read from when no row is left to focus. */
	function headingTarget() {
		const heading = document.getElementById('access-heading');
		if (heading) heading.tabIndex = -1;
		return heading;
	}

	async function settled(memberId: string, outcome: SignInOutcome, from: HTMLElement | null) {
		if (outcome === 'gone') return left(memberId, from);
		if (outcome !== 'cancelled') {
			await resource.reload();
			await tick();
		}
		if (destroyed || !focusIsFree(from)) return;
		if (from?.isConnected) return from.focus();
		// The person moved to another list (invited, or access removed): their first action there, else their row.
		const row = document.querySelector<HTMLElement>(
			`[data-access-row][data-member-id="${CSS.escape(memberId)}"]`
		);
		const target = row?.querySelector<HTMLElement>('button') ?? row?.querySelector('a[href]');
		(target ?? headingTarget())?.focus();
	}
</script>

<svelte:document onpointerdown={dismiss} onkeydown={dismiss} />

<!-- The heading takes focus programmatically; it shows no ring of its own (app-shell-ux §6a). -->
{#if resource.state !== 'unavailable'}
	<PageHeader
		title="Members and access"
		level={2}
		id="access-heading"
		class="[&_h2:focus]:outline-none"
		description="Who can sign in to this organization, and with what authority. Accounts are given person by person; nothing here lists every account on this server."
	/>
{/if}

<Loaded
	{resource}
	label="members and access"
	error="Couldn't load who can sign in"
	place="Members and access"
	area="Organization settings"
	isEmpty={(o) => o.people.length === 0}
	emptyTitle="No people yet"
	emptyDescription="People are added from Teams. Only people can sign in: agents never do."
	emptyLevel={3}
	errorLevel={3}
>
	{#snippet emptyAction()}
		<Button variant="outline" class={mobileButton} href={resolve('/teams/members' as '/')}
			>Open Members</Button
		>
	{/snippet}

	{#snippet children(overview)}
		{@const groups = groupPeople(overview.people)}
		{@const canManage = overview.viewer.canManage}
		{@const owner = overview.people.find((p) => p.owner)}
		{@const admin = overview.administrator}

		<SignInAccessDialogs
			bind:this={host}
			{organizationId}
			organizationName={overview.organization.name}
			onsettled={settled}
		/>

		<!-- Each fact's hand-over link sits under its value, so the value keeps the row's width at every size. -->
		{#snippet handOver(path: '/teams/org-chart' | '/settings', label: string)}
			<a
				href={resolve(path)}
				class="mt-1 inline-flex items-center rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
				>{label}</a
			>
		{/snippet}
		{#snippet ownerFact()}
			{#if owner}
				<span class="block" data-fact="owner"
					>{owner.name}{overview.viewer.isOwner ? ' · you' : ''}</span
				>
				{#if overview.viewer.isOwner}{@render handOver(
						'/teams/org-chart',
						'Hand over ownership on the org chart'
					)}{/if}
			{:else}
				<span class="block text-danger-text" data-fact="owner"
					>No owner on record: this record is invalid</span
				>
			{/if}
		{/snippet}
		{#snippet adminFact()}
			{@const adminUsername = visibleAdminUsername(admin, overview.viewer)}
			<span class="block" data-fact="administrator"
				>{admin.name
					? `${admin.name}${adminUsername ? ` (${adminUsername})` : ''}`
					: `${adminUsername ?? 'An account'} · no person in this organization yet`}{admin.isYou
					? ' · you'
					: ''}</span
			>
			{#if overview.viewer.isAdministrator}{@render handOver(
					'/settings',
					'Hand over the administrator role in General'
				)}{/if}
		{/snippet}

		{#snippet list(name: AccessList, people: AccessPerson[], empty: string)}
			{#if gone === name}
				<p
					class="px-4 pb-4 text-sm/6 text-danger-text [overflow-wrap:anywhere] min-[640px]:px-6 min-[1024px]:px-8"
					role="alert"
				>
					{NOT_IN_ORGANIZATION}
				</p>
			{/if}
			{#if people.length === 0}
				<p class="px-4 py-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">{empty}</p>
			{:else}
				<ul role="list" class="@container divide-y divide-line-subtle border-y border-line-subtle">
					{#each people as person (person.memberId)}
						<AccessRow
							{person}
							username={visibleUsername(person, overview.viewer)}
							{canManage}
							busy={busy?.memberId === person.memberId ? busy.kind : undefined}
							waiting={busy && busy.memberId !== person.memberId
								? (resource.data?.people.find((p) => p.memberId === busy?.memberId)?.name ??
									'someone')
								: undefined}
							refusal={refusal?.memberId === person.memberId ? refusal.message : undefined}
							onact={(kind, event) => act(person, kind, event)}
						/>
					{/each}
				</ul>
			{/if}
		{/snippet}

		<Section
			level={3}
			title="Owner and administrator"
			description="The owner has full access to the organization. The administrator creates organizations and collects recovery codes. Both invite people to sign in, make new sign-in links and remove access. The administrator role alone opens no organization."
		>
			<DescriptionList
				items={[
					{ term: 'Owner', content: ownerFact },
					{ term: 'Administrator', content: adminFact }
				]}
			/>
		</Section>

		{#if !canManage}
			<p class="px-4 pt-6 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
				{MANAGERS_ONLY}
			</p>
		{/if}

		<Section
			level={3}
			title="Can sign in"
			description="People with an account in this organization. Each account is tied to one person."
		>
			{@render list('signedIn', groups.signedIn, 'Nobody has an account here yet.')}
		</Section>

		<Section
			level={3}
			title="Invited, not signed in yet"
			description="A registration link works once and expires after 24 hours. The product sends no email: copy the link and pass it on."
		>
			{@render list('invited', groups.invited, 'No one is waiting for an invitation.')}
		</Section>

		<Section
			level={3}
			title="No account yet"
			description="People who can't sign in. Agents are not listed: they never sign in."
		>
			{@render list(
				'noAccount',
				groups.noAccount,
				'Everyone here has an account or an invitation.'
			)}
		</Section>

		{#if groups.deactivated.length > 0}
			<Section
				level={3}
				title="Deactivated"
				description="Deactivating a person removed their access. They can't be invited while deactivated."
			>
				{@render list('deactivated', groups.deactivated, 'No one is deactivated.')}
			</Section>
		{/if}

		<Section
			level={3}
			title="Who can do what"
			description="The rules behind these lists, in words."
		>
			<DescriptionList
				items={[
					{
						term: 'Owner',
						value:
							'Changes structure, invites people to sign in, makes new sign-in links, removes access, and hands ownership to someone who has an account.'
					},
					{
						term: 'Administrator',
						value:
							'Creates organizations, collects recovery codes, hands the administrator role to another account, invites people to sign in, makes new sign-in links and removes access.'
					},
					{
						term: 'People with an account',
						value: 'Read the organization, work on tasks and talk to any person or agent.'
					},
					{
						term: 'Hiring',
						value:
							'The owner and the administrator hire into any team. An employee with Allow hiring hires into the teams they lead and the teams under them.'
					},
					{
						term: 'Agents',
						value: 'Never sign in. An account is never tied to an agent.'
					}
				]}
			/>
		</Section>
	{/snippet}
</Loaded>
