<!--
  @component
  One person in a Members and access list: name, handle and home team, Owner and Administrator marks in words, the
  sign-in state in words, and, for the owner or the administrator, the sign-in actions (the members module's shared
  SignInAccessActions). Below 1024 px the actions sit under the text; from 1024 px they sit to the right.
-->
<script lang="ts">
	import { SignInAccessActions, expiryText, type SignInPerson } from '$lib/modules/members';
	import { tone } from '$lib/components/pills';
	import { stateWord } from '../../present';
	import type { AccessPerson } from '../../types';

	let {
		person,
		organizationName,
		canManage,
		issueLink,
		removeAccess,
		onchanged
	}: {
		person: AccessPerson;
		organizationName: string;
		canManage: boolean;
		issueLink: (
			memberId: string,
			kind: 'invite' | 'reset'
		) => Promise<{ url: string; expiresAt: string }>;
		removeAccess: (memberId: string) => Promise<void>;
		onchanged: () => Promise<void>;
	} = $props();

	const signInPerson: SignInPerson = $derived({
		id: person.memberId,
		name: person.name,
		owner: person.owner,
		canSignIn: person.state === 'signed_in',
		...(person.state === 'invited' && person.inviteExpiresAt
			? { invitePending: { expiresAt: person.inviteExpiresAt } }
			: {})
	});

	const stateLine = $derived(
		person.state === 'signed_in'
			? person.username
				? `${stateWord.signed_in} · username ${person.username}`
				: stateWord.signed_in
			: person.state === 'invited'
				? person.inviteExpiresAt
					? `${stateWord.invited}. The link works until ${expiryText(person.inviteExpiresAt).replace(/^at /, '')}.`
					: stateWord.invited
				: stateWord.no_account
	);
	const mark =
		'inline-flex items-center rounded-full px-2 py-1 text-xs/4 font-medium ring-1 ring-inset';
</script>

<li
	data-access-row
	data-testid="access-row"
	class="flex flex-col gap-y-3 px-4 py-4 min-[640px]:px-6 min-[1024px]:flex-row min-[1024px]:items-start min-[1024px]:justify-between min-[1024px]:gap-x-8 min-[1024px]:px-8"
>
	<div class="min-w-0">
		<p class="flex flex-wrap items-center gap-2 text-sm/6 font-semibold text-white">
			<span class="min-w-0 [overflow-wrap:anywhere]">{person.name}</span>
			{#if person.owner}<span class="{mark} {tone.ok}">Owner</span>{/if}
			{#if person.administrator}<span class="{mark} {tone.info}">Administrator</span>{/if}
		</p>
		<p class="text-xs/5 text-gray-400 [overflow-wrap:anywhere]">
			@{person.handle} · {person.homeTeam}
		</p>
		<p class="mt-1 text-sm/6 text-gray-300 [overflow-wrap:anywhere]">{stateLine}</p>
	</div>
	{#if canManage}
		<div class="min-w-0 max-w-full">
			<SignInAccessActions
				{organizationName}
				person={signInPerson}
				{issueLink}
				{removeAccess}
				{onchanged}
			/>
		</div>
	{/if}
</li>
