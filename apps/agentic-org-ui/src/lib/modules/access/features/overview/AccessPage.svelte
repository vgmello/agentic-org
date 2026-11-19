<!--
  @component
  Organization settings › Members and access (server-installation-and-access-ux §4a, offices-and-teams-ux §5b, §7.1,
  §7.4): who can sign in to this organization and with what authority. The owner and the administrator are shown as
  facts; then three lists of people: Can sign in, Invited but not signed in yet, No account yet. Agents never appear.
  The owner and the administrator see the sign-in actions; everyone else sees the lists and why they have no actions.
  States: loading skeleton, unavailable, error with Retry, no people, and per action: busy labels and inline refusals.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import DescriptionList from '$lib/components/DescriptionList.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { accessApi } from '../../api';
	import { MANAGERS_ONLY, groupPeople } from '../../present';
	import type { AccessOverview, AccessPerson } from '../../types';
	import AccessRow from './AccessRow.svelte';

	let { organizationId }: { organizationId: string } = $props();

	const resource = new Resource(() => accessApi.overview(organizationId));

	// After a client navigation the shell focuses the area's h1 on the next tick; this page's own heading takes focus
	// after that, so the person lands on what the page is for (app-shell-ux §6a).
	onMount(() => {
		const timer = setTimeout(() => {
			const active = document.activeElement;
			if (active && active !== document.body && !active.matches('main h1')) return;
			const heading = document.getElementById('access-heading');
			if (heading) {
				heading.tabIndex = -1;
				heading.focus({ preventScroll: true });
			}
		}, 0);
		return () => clearTimeout(timer);
	});

	function ownerName(o: AccessOverview) {
		return o.people.find((p) => p.owner)?.name;
	}
</script>

<svelte:head><title>{pageTitle('Members and access', 'Organization settings')}</title></svelte:head>

<PageHeader
	title="Members and access"
	level={2}
	id="access-heading"
	description="Who can sign in to this organization, and with what authority. Accounts are given person by person; nothing here lists every account on this server."
/>

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
	{#snippet children(overview)}
		{@const groups = groupPeople(overview.people)}
		{@const canManage = overview.viewer.canManage}

		{#snippet ownerNameSnippet()}
			{ownerName(overview) ?? 'No owner is recorded for this organization'}{overview.viewer.isOwner
				? ' · you'
				: ''}
		{/snippet}
		{#snippet ownerAction()}
			<a
				href={resolve('/teams/org-chart' as '/')}
				class="text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
				>Hand over ownership on the org chart</a
			>
		{/snippet}
		{#snippet adminSnippet()}
			{overview.administrator.name ?? 'An account without a person in this organization'}
			({overview.administrator.username}){overview.administrator.isYou ? ' · you' : ''}
		{/snippet}
		{#snippet adminAction()}
			<a
				href={resolve('/settings' as '/')}
				class="text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
				>Hand over the administrator role in General</a
			>
		{/snippet}

		{#snippet list(people: AccessPerson[], empty: string)}
			{#if people.length === 0}
				<p class="px-4 py-4 text-sm/6 text-gray-400 min-[640px]:px-6 min-[1024px]:px-8">{empty}</p>
			{:else}
				<ul role="list" class="divide-y divide-white/5 border-y border-white/5">
					{#each people as person (person.memberId)}
						<AccessRow
							{person}
							organizationName={overview.organization.name}
							{canManage}
							issueLink={(memberId, kind) => accessApi.issueLink(organizationId, memberId, kind)}
							removeAccess={(memberId) => accessApi.removeAccess(organizationId, memberId)}
							onchanged={() => resource.reload()}
						/>
					{/each}
				</ul>
			{/if}
		{/snippet}

		<Section
			level={3}
			title="Owner and administrator"
			description="The owner has full access to the organization. The administrator creates organizations and collects recovery codes, and opens no organization by itself."
		>
			<DescriptionList
				items={[
					{
						term: 'Owner',
						content: ownerNameSnippet,
						...(overview.viewer.isOwner ? { action: ownerAction } : {})
					},
					{
						term: 'Administrator',
						content: adminSnippet,
						...(overview.viewer.isAdministrator ? { action: adminAction } : {})
					}
				]}
			/>
		</Section>

		{#if !canManage}
			<p class="px-4 pt-6 text-sm/6 text-gray-400 min-[640px]:px-6 min-[1024px]:px-8">
				{MANAGERS_ONLY}
			</p>
		{/if}

		<Section
			level={3}
			title="Can sign in"
			description="People with an account in this organization. Each account is tied to one person."
		>
			{@render list(groups.signedIn, 'Nobody has an account here yet.')}
		</Section>

		<Section
			level={3}
			title="Invited, not signed in yet"
			description="A registration link works once and expires after 24 hours. The product sends no email: copy the link and pass it on."
		>
			{@render list(groups.invited, 'No one is waiting for an invitation.')}
		</Section>

		<Section
			level={3}
			title="No account yet"
			description="People who can't sign in. Agents are not listed: they never sign in."
		>
			{@render list(groups.noAccount, 'Everyone here has an account or an invitation.')}
		</Section>

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
							'Changes structure, invites and removes access, and hands ownership to someone who has an account.'
					},
					{
						term: 'Administrator',
						value:
							'Creates organizations, collects recovery codes and hands the administrator role to another account.'
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
