<script lang="ts">
	import Pause from '@lucide/svelte/icons/pause';
	import X from '@lucide/svelte/icons/x';
	import AgentStatusIcon from '$lib/components/AgentStatusIcon.svelte';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { agentLine, agentStatus, isPaused } from '$lib/org/agents';
	import { kindWord, teamsOfMember } from '$lib/org/model';
	import type { Member, OfficeInfo, OrgSnapshot } from '$lib/proto';

	let {
		snapshot,
		member,
		offices,
		onclear,
		organization = 'the organization',
		onedit,
		onremove,
		onmanager,
		onteams,
		oneditagent,
		onpause,
		onresume,
		onhowitruns,
		onoffboard,
		onmove
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
		/** Change who they report to (agents and people). */
		onmanager?: (member: Member) => void;
		/** Edit the teams they are in, with a title in each. */
		onteams?: (member: Member) => void;
		/** An agent's record: title, role, focus, runtime, model, home and messaging name. */
		oneditagent?: (member: Member) => void;
		onpause?: (member: Member) => void;
		onresume?: (member: Member) => void;
		/** How it runs: always on, on call, scheduled, or started by a person. */
		onhowitruns?: (member: Member) => void;
		/** Offboard an agent: a state, not a deletion. */
		onoffboard?: (member: Member) => void;
		/** Move them from one team to another in one step (with Undo). */
		onmove?: (member: Member) => void;
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
		{#if !member.owner}
			{#if onteams}<Button variant="outline" size="xs" onclick={() => onteams(member)}>Edit teams…</Button>{/if}
			{#if onmove && teams.length > 0}<Button variant="outline" size="xs" onclick={() => onmove(member)}>Move to team…</Button>{/if}
			{#if onmanager}<Button variant="outline" size="xs" onclick={() => onmanager(member)}>Change manager…</Button>{/if}
		{/if}
		{#if member.kind === 'agent' && member.agent}
			{#if isPaused(member)}<Badge variant="secondary" data-testid="paused-badge"><Pause class="size-3" aria-hidden="true" /> Paused</Badge>{/if}
			<span class="basis-full text-xs/5 text-gray-300" data-testid="agent-line">{agentLine(member)}</span>
			{@const live = agentStatus(member.agent)}
			<span class="inline-flex basis-full items-center gap-x-1.5 text-xs/5 text-gray-300" data-testid="agent-status"><AgentStatusIcon status={live.id} />{live.word}{live.detail ? ` · ${live.detail}` : ''}</span>
			<!-- What it is doing now arrives with the Activity feed. -->
			<span class="basis-full text-xs/5 text-gray-400" data-testid="agent-now">Now: —</span>
			<span class="basis-full text-xs/5 text-gray-400">Messaging name <code class="font-mono text-gray-200" data-testid="messaging-name">{member.agent.messaging_name}</code></span>
			{#if isPaused(member)}
				{#if onresume}<Button variant="outline" size="xs" onclick={() => onresume(member)}>Resume</Button>{/if}
			{:else if onpause}<Button variant="outline" size="xs" onclick={() => onpause(member)}>Pause…</Button>{/if}
			{#if onhowitruns}<Button variant="outline" size="xs" onclick={() => onhowitruns(member)}>How it runs…</Button>{/if}
			{#if oneditagent}<Button variant="outline" size="xs" onclick={() => oneditagent(member)}>Edit…</Button>{/if}
			{#if onoffboard}<Button variant="outline" size="xs" onclick={() => onoffboard(member)}>Offboard…</Button>{/if}
		{/if}
		{#if member.kind === 'person' && !member.owner}
			{#if onedit}<Button variant="outline" size="xs" onclick={() => onedit(member)}>Edit…</Button>{/if}
			{#if onremove}<Button variant="outline" size="xs" onclick={() => onremove(member)}>Remove from {organization}…</Button>{/if}
		{/if}
		<button type="button" class="ml-auto rounded p-1 text-gray-400 hover:text-white focus-visible:outline-2 focus-visible:outline-indigo-400" aria-label="Clear selection" onclick={onclear}>
			<X class="size-4" aria-hidden="true" />
		</button>
	</div>
{/if}
