<script lang="ts" module>
	import Building2 from '@lucide/svelte/icons/building-2';
	import FolderKanban from '@lucide/svelte/icons/folder-kanban';
	import Plus from '@lucide/svelte/icons/plus';
	import Replace from '@lucide/svelte/icons/replace';
	import User from '@lucide/svelte/icons/user';
	import Users from '@lucide/svelte/icons/users';

	export const LEVEL_ICON = { organization: Building2, team: Users, project: FolderKanban, agent: User } as const;
	export const KIND_ICON = { override: Replace, addition: Plus } as const;
</script>

<script lang="ts">
	import { placeLabel } from '$lib/modules/processes/present';
	import type { Level } from '$lib/modules/processes/types';

	// A level is a word with an icon: "Team Web v2", never an icon and a name alone (PROC-18).
	let { level, scopeName, version }: { level: Level; scopeName?: string; version?: number } = $props();
	const Icon = $derived(LEVEL_ICON[level]);
</script>

<span class="inline-flex max-w-full items-center gap-x-1 rounded-md bg-white/5 px-1.5 py-0.5 text-xs/5 text-gray-200 outline-1 -outline-offset-1 outline-white/10 [overflow-wrap:anywhere]" data-testid="level-badge">
	<Icon class="size-3.5 shrink-0" aria-hidden="true" />
	{placeLabel(level, scopeName)}{version !== undefined ? ` v${version}` : ''}
</span>
