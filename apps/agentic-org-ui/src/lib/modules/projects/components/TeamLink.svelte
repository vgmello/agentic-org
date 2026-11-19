<!--
  @component
  A team named inside a line of facts: its name links to the team page, its TEAM- reference follows in mono.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import type { TeamRef } from '../types';
	import { inlineLink } from './classes';

	let { team, size = 'sm' }: { team: TeamRef; size?: 'sm' | 'xs' } = $props();
</script>

<a href={resolve(`/teams/${encodeURIComponent(team.id)}` as '/')} class={inlineLink[size]}
	>{team.name}</a
>{#if team.ref}<span class="ml-1 font-mono text-xs/5 text-fg-muted">{team.ref}</span>{/if}
