<!--
  @component
  Invitations and requests that ended, listed after the offices (§6.1, §6.3): Invited with its expiry and Cancel invitation, and
  expired, declined or cancelled ones with New invitation and Remove. Every one has its words, not a color.
-->
<script lang="ts">
	import Mail from '@lucide/svelte/icons/mail';
	import MailX from '@lucide/svelte/icons/mail-x';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Button } from '$lib/components/ui/button';
	import { tone } from '$lib/components/pills';
	import { expiresText } from '../../time';
	import type { Invitation } from '../../types';

	let {
		invitations,
		now,
		oncancel,
		onrenew,
		onremove
	}: {
		invitations: Invitation[];
		now: number;
		oncancel: (invitation: Invitation, button: HTMLElement) => void;
		onrenew: (invitation: Invitation, button: HTMLElement) => void;
		onremove: (invitation: Invitation, button: HTMLElement) => void;
	} = $props();

	function label(i: Invitation): string {
		if (i.state === 'invited')
			return Date.parse(i.expiresAt) <= now ? 'Invitation expired' : 'Invited';
		if (i.state === 'expired') return 'Invitation expired';
		if (i.state === 'cancelled') return 'Invitation cancelled';
		return 'Join declined';
	}
	const live = (i: Invitation) => i.state === 'invited' && Date.parse(i.expiresAt) > now;
</script>

{#each invitations as invitation (invitation.id)}
	<li
		class="flex flex-wrap items-center justify-between gap-x-4 gap-y-3 border-b border-white/5 px-4 py-4 last:border-b-0 min-[640px]:px-6 min-[1024px]:px-8"
		data-testid="invitation-row"
		data-invitation={invitation.id}
	>
		<div class="min-w-0 flex-1 basis-56">
			<p
				class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm/6 font-semibold text-white [overflow-wrap:anywhere]"
			>
				{invitation.officeName}
				<StatusPill label={label(invitation)} class={live(invitation) ? '' : tone.owner}>
					{#snippet icon()}{#if live(invitation)}<Mail
								class="size-3.5 text-gray-400"
								aria-hidden="true"
							/>{:else}<MailX class="size-3.5 text-amber-400" aria-hidden="true" />{/if}{/snippet}
				</StatusPill>
			</p>
			<p class="mt-1 text-xs/5 text-gray-400 [overflow-wrap:anywhere]">
				{#if live(invitation)}invitation · {expiresText(
						invitation.expiresAt,
						now
					)}{:else if invitation.note}{invitation.note}{:else}The invitation for {invitation.officeName}
					expired before the join was approved. Nothing was added.{/if}
			</p>
		</div>
		<div class="flex flex-wrap items-center gap-2">
			{#if live(invitation)}
				<Button
					variant="outline"
					size="sm"
					class="max-[639px]:min-h-11"
					aria-label="Cancel invitation for {invitation.officeName}"
					onclick={(e) => oncancel(invitation, e.currentTarget)}>Cancel invitation</Button
				>
			{/if}
			<Button
				variant="outline"
				size="sm"
				class="max-[639px]:min-h-11"
				aria-label="New invitation for {invitation.officeName}"
				onclick={(e) => onrenew(invitation, e.currentTarget)}>New invitation</Button
			>
			{#if !live(invitation)}
				<Button
					variant="outline"
					size="sm"
					class="max-[639px]:min-h-11"
					aria-label="Remove the invitation for {invitation.officeName}"
					onclick={(e) => onremove(invitation, e.currentTarget)}>Remove</Button
				>
			{/if}
		</div>
	</li>
{/each}
