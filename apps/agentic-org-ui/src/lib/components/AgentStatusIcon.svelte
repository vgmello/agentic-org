<script lang="ts">
	import CircleHelp from '@lucide/svelte/icons/circle-help';
	import LogOut from '@lucide/svelte/icons/log-out';
	import Moon from '@lucide/svelte/icons/moon';
	import Pause from '@lucide/svelte/icons/pause';
	import UserRound from '@lucide/svelte/icons/user-round';
	import type { AgentStatus } from '$lib/org/agents';

	let { status }: { status: AgentStatus['id'] } = $props();
</script>

<!-- The icon that goes with an agent's status word (the word is always beside it). -->
{#if status === 'paused'}<Pause class="size-3.5 text-amber-200" aria-hidden="true" />
{:else if status === 'asleep'}<Moon class="size-3.5 text-gray-400" aria-hidden="true" />
{:else if status === 'by_person'}<UserRound class="size-3.5 text-gray-400" aria-hidden="true" />
{:else if status === 'former'}<LogOut class="size-3.5 text-gray-400" aria-hidden="true" />
{:else}<CircleHelp class="size-3.5 text-gray-400" aria-hidden="true" />{/if}
