<!--
  @component
  One person in a Members and access list: an avatar, the name (the row's one stretched link, to the member profile),
  the Owner pill, the handle, home team and Administrator as text, the sign-in state in words, and for the owner or the
  administrator the sign-in actions, with any refusal right under them. The row reports each action to the page, which
  owns the dialogs and where focus goes.

  Layout follows the Members directory: the list is the container. Stacked by default; side by side (text, then the
  actions) once the list is at least 44rem wide, a container query in rem, so a narrow window or 200% text keeps the
  stacked row. Under 20rem of list (320 px at 200% text) the avatar sits above the text, so the words keep the width.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { expiryUntil, type SignInAction } from '$lib/modules/members';
	import { stateWord } from '../../present';
	import type { AccessPerson } from '../../types';

	let {
		person,
		username,
		canManage,
		busy,
		waiting,
		refusal,
		onact
	}: {
		person: AccessPerson;
		/** Present only when the viewer may see this person's username (owner, administrator or self). */
		username?: string;
		canManage: boolean;
		/** The action this row's person is running right now, while its link is being made. */
		busy?: 'invite' | 'reset';
		/** Someone else's link is being made (their name): this row's actions wait, and say so. */
		waiting?: string;
		/** Why the last action on this person was refused, shown under the actions. */
		refusal?: string;
		onact: (kind: SignInAction, event: MouseEvent) => void;
	} = $props();

	const stateLine = $derived(
		!person.active
			? 'Deactivated · no access'
			: person.state === 'signed_in'
				? username
					? `${stateWord.signed_in} · username ${username}`
					: stateWord.signed_in
				: person.state === 'invited'
					? person.inviteExpiresAt
						? `${stateWord.invited}. The link works until ${expiryUntil(person.inviteExpiresAt)}.`
						: stateWord.invited
					: stateWord.no_account
	);
	const actions = $derived(canManage && person.active);
	// Under 20rem of list (320 px at 200% text) the buttons take the column's width and their labels wrap between
	// words, so they never run into the gutter (app-shell-ux §6a: labels wrap, they don't shrink or clip).
	const wrapLabel =
		'@max-[20rem]:h-auto @max-[20rem]:w-full @max-[20rem]:justify-start @max-[20rem]:py-2 @max-[20rem]:text-left @max-[20rem]:whitespace-normal';
</script>

<li
	data-access-row
	data-member-id={person.memberId}
	class="relative flex flex-col gap-y-3 px-4 py-4 transition-colors duration-150 hover:bg-surface-raised/60 motion-reduce:transition-none min-[640px]:px-6 min-[1024px]:px-8 @min-[44rem]:flex-row @min-[44rem]:items-start @min-[44rem]:justify-between @min-[44rem]:gap-x-8"
>
	<div class="flex min-w-0 flex-1 items-start gap-x-4 gap-y-2 @max-[20rem]:flex-col">
		<Avatar name={person.name} size="md" />
		<div class="min-w-0 flex-1">
			<p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm/6">
				<a
					href={resolve(`/members/${encodeURIComponent(person.memberId)}` as '/')}
					class="rounded-sm font-semibold text-fg [overflow-wrap:anywhere] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
					><span class="absolute inset-0" aria-hidden="true"></span>{person.name}</a
				>
				<!-- The member profile's Owner badge, so the owner reads the same everywhere; one pill per row. -->
				{#if person.owner}<StatusPill
						label="Owner"
						class="bg-brand/10 text-brand-text ring-brand/30"
					/>{/if}
			</p>
			<p class="flex flex-wrap gap-x-2 text-xs/5 text-fg-muted">
				<span class="font-mono [overflow-wrap:anywhere]">@{person.handle}</span>
				<span aria-hidden="true">·</span>
				{#if person.homeTeam}<span class="[overflow-wrap:anywhere]">{person.homeTeam}</span
					>{:else}<span class="text-danger-text"
						>No home team on record: this record is invalid</span
					>{/if}
				{#if person.administrator}<span aria-hidden="true">·</span><span class="text-fg-secondary"
						>Administrator</span
					>{/if}
			</p>
			<p class="mt-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">{stateLine}</p>
		</div>
	</div>
	{#if actions}
		<!-- Above the stretched name link, so each action stays its own target. -->
		<div class="relative flex min-w-0 flex-col gap-y-2 @min-[44rem]:items-end">
			<div class="flex flex-wrap items-center gap-x-6 gap-y-3 @min-[44rem]:justify-end">
				{#if person.state !== 'signed_in'}
					<Button
						variant="outline"
						class="{mobileButton} {wrapLabel}"
						aria-disabled={waiting ? 'true' : undefined}
						aria-busy={busy === 'invite'}
						onclick={(e) => onact('invite', e)}
						>{busy === 'invite' ? 'Inviting…' : 'Invite to sign in'}</Button
					>
				{:else}
					<Button
						variant="outline"
						class="{mobileButton} {wrapLabel}"
						aria-disabled={waiting ? 'true' : undefined}
						aria-busy={busy === 'reset'}
						onclick={(e) => onact('reset', e)}
						>{busy === 'reset' ? 'Making link…' : 'New sign-in link'}</Button
					>
					{#if !person.owner}
						<Button
							variant="outline"
							class="{mobileButton} {wrapLabel}"
							aria-disabled={waiting ? 'true' : undefined}
							onclick={(e) => onact('remove', e)}>Remove access…</Button
						>
					{/if}
				{/if}
			</div>
			{#if person.owner && person.state === 'signed_in'}
				<p class="text-xs/5 text-fg-muted @min-[44rem]:text-right">Hand over ownership first.</p>
			{/if}
			{#if person.state === 'invited'}
				<p class="text-xs/5 text-fg-muted @min-[44rem]:text-right">
					Inviting again cancels the previous link.
				</p>
			{/if}
			{#if waiting}
				<p class="text-xs/5 text-fg-muted @min-[44rem]:text-right">
					Wait for {waiting}'s link first.
				</p>
			{/if}
			{#if refusal}
				<p
					class="text-sm/6 text-danger-text [overflow-wrap:anywhere] @min-[44rem]:text-right"
					role="alert"
				>
					{refusal}
				</p>
			{/if}
		</div>
	{/if}
</li>
