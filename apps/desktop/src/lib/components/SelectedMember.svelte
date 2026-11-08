<script lang="ts">
	import X from '@lucide/svelte/icons/x';
	import { teamsOfMember } from '$lib/org/model';
	import type { Member, OfficeInfo, OrgSnapshot } from '$lib/proto';

	let {
		snapshot,
		member,
		offices,
		onclear
	}: { snapshot: OrgSnapshot; member: Member | undefined; offices: OfficeInfo[]; onclear: () => void } = $props();

	const teams = $derived(member ? teamsOfMember(snapshot, member.id) : []);
	const office = $derived(member?.kind === 'agent' ? (offices.find((o) => o.id === member.office_id)?.name ?? 'Unplaced') : undefined);
</script>

<!-- The selected member's details in words: the other teams a "+N teams" badge stands for are listed here. -->
{#if member}
	<div class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg bg-white/5 px-4 py-2 text-sm/6 text-gray-200 outline-1 -outline-offset-1 outline-white/10" role="status" data-testid="selected-member">
		<span class="font-semibold text-white">{member.name}</span>
		<span class="text-gray-400">{member.kind === 'agent' ? 'Agent' : member.kind === 'person' ? 'Person' : 'Member'}</span>
		<span>{teams.length > 0 ? `In ${teams.map((t) => t.name).join(', ')}` : 'Not in a team'}</span>
		{#if office}<span class="text-gray-400">· {office}</span>{/if}
		<button type="button" class="ml-auto rounded p-1 text-gray-400 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-400" aria-label="Clear selection" onclick={onclear}>
			<X class="size-4" aria-hidden="true" />
		</button>
	</div>
{/if}
