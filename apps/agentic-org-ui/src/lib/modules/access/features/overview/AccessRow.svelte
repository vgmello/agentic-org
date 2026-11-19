<!--
  @component
  One person in a Members and access list: an avatar, the name (a link to the member profile), the handle and home team,
  Owner and Administrator marks in words, the sign-in state in words, and for the owner or the administrator the sign-in
  actions. The row reports each action to the page, which owns the dialogs, the refusals and where focus goes.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { expiryText } from '$lib/modules/members';
	import { stateWord } from '../../present';
	import type { AccessPerson } from '../../types';

	let {
		person,
		canManage,
		busy,
		onact
	}: {
		person: AccessPerson;
		canManage: boolean;
		/** The action this row's person is running right now, while its link is being made. */
		busy?: 'invite' | 'reset';
		onact: (kind: 'invite' | 'reset' | 'remove', event: MouseEvent) => void;
	} = $props();

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
</script>

<li
	data-access-row
	data-member-id={person.memberId}
	data-testid="access-row"
	class="flex flex-col gap-y-3 px-4 py-4 min-[640px]:px-6 min-[1024px]:flex-row min-[1024px]:items-start min-[1024px]:justify-between min-[1024px]:gap-x-8 min-[1024px]:px-8"
>
	<div class="flex min-w-0 flex-1 items-start gap-3">
		<Avatar name={person.name} size="md" />
		<div class="min-w-0">
			<p class="flex flex-wrap items-center gap-2 text-sm/6 font-semibold text-fg">
				<a
					href={resolve(`/members/${person.memberId}` as '/')}
					class="min-w-0 rounded-sm [overflow-wrap:anywhere] hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
					>{person.name}</a
				>
				{#if person.owner}
					<StatusPill label="Owner" class="bg-brand/10 text-brand-text ring-brand/30" />
				{/if}
				{#if person.administrator}<StatusPill label="Administrator" />{/if}
			</p>
			<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
				<span class="font-mono">@{person.handle}</span> · {person.homeTeam}
			</p>
			<p class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">{stateLine}</p>
		</div>
	</div>
	{#if canManage}
		<div class="flex min-w-0 shrink-0 flex-col gap-y-2 min-[1024px]:items-end">
			<div class="flex flex-wrap items-center gap-x-6 gap-y-3">
				{#if person.state !== 'signed_in'}
					<Button
						variant="outline"
						class={mobileButton}
						aria-busy={busy === 'invite'}
						onclick={(e) => onact('invite', e)}
						>{busy === 'invite' ? 'Inviting…' : 'Invite to sign in'}</Button
					>
				{:else}
					<Button
						variant="outline"
						class={mobileButton}
						aria-busy={busy === 'reset'}
						onclick={(e) => onact('reset', e)}
						>{busy === 'reset' ? 'Making link…' : 'New sign-in link'}</Button
					>
					{#if !person.owner}
						<Button variant="outline" class={mobileButton} onclick={(e) => onact('remove', e)}
							>Remove access…</Button
						>
					{/if}
				{/if}
			</div>
			{#if person.owner && person.state === 'signed_in'}
				<p class="text-xs/5 text-fg-muted min-[1024px]:text-right">Hand over ownership first.</p>
			{/if}
			{#if person.state === 'invited'}
				<p class="text-xs/5 text-fg-muted min-[1024px]:text-right">
					Inviting again cancels the previous link.
				</p>
			{/if}
		</div>
	{/if}
</li>
