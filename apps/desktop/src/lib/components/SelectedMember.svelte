<script lang="ts">
	import X from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { kindWord, teamsOfMember } from '$lib/org/model';
	import type { Member, OfficeInfo, OrgSnapshot } from '$lib/proto';

	let {
		snapshot,
		member,
		offices,
		onclear,
		organization = 'the organization',
		onedit,
		onremove
	}: {
		snapshot: OrgSnapshot;
		member: Member | undefined;
		offices: OfficeInfo[];
		onclear: () => void;
		organization?: string;
		/** Edit a person's name and contact (people only: an agent is edited in its own flow). */
		onedit?: (member: Member) => void;
		/** Remove a person (an agent leaves through offboarding, not here). */
		onremove?: (member: Member) => void;
	} = $props();

	const teams = $derived(member ? teamsOfMember(snapshot, member.id) : []);
	const office = $derived(member?.kind === 'agent' ? (offices.find((o) => o.id === member.office_id)?.name ?? 'Unplaced') : undefined);
</script>

<!-- The selected member's details in words: the other teams a "+N teams" badge stands for are listed here. -->
{#if member}
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg bg-white/5 px-4 py-2 text-sm/6 text-gray-200 outline-1 -outline-offset-1 outline-white/10" role="status" data-testid="selected-member">
		<span class="font-semibold text-white">{member.name}</span>
		<span class="text-gray-400">{kindWord(member.kind)}</span>
		<span>{teams.length > 0 ? `In ${teams.map((t) => t.name).join(', ')}` : 'Not in a team'}</span>
		{#if office}<span class="text-gray-400">· {office}</span>{/if}
		{#if member.kind === 'person' && !member.owner}
			{#if onedit}<Button variant="outline" size="xs" onclick={() => onedit(member)}>Edit…</Button>{/if}
			{#if onremove}<Button variant="outline" size="xs" onclick={() => onremove(member)}>Remove from {organization}…</Button>{/if}
		{/if}
		<button type="button" class="ml-auto rounded p-1 text-gray-400 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-400" aria-label="Clear selection" onclick={onclear}>
			<X class="size-4" aria-hidden="true" />
		</button>
	</div>
{/if}
