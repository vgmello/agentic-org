<script lang="ts" module>
	import Building2 from '@lucide/svelte/icons/building-2';
	import FolderKanban from '@lucide/svelte/icons/folder-kanban';
	import Lock from '@lucide/svelte/icons/lock';
	import Plus from '@lucide/svelte/icons/plus';
	import PowerOff from '@lucide/svelte/icons/power-off';
	import Replace from '@lucide/svelte/icons/replace';
	import User from '@lucide/svelte/icons/user';
	import Users from '@lucide/svelte/icons/users';
	import { LEVEL_LABEL, type EntryKind, type Level } from '$lib/processes/types';

	export const LEVEL_ICON = { organization: Building2, team: Users, project: FolderKanban, agent: User } as const;
	export const KIND_WORD: Record<EntryKind, string> = { inherited: '', override: 'Override', addition: 'Addition' };
	export { LEVEL_LABEL };
	export type { Level, EntryKind };
	export const KIND_ICON = { override: Replace, addition: Plus } as const;
	export const SEALED_ICON = Lock;
	export const OFF_ICON = PowerOff;
</script>

<script lang="ts">
	// A level or a kind is always a word with an icon, never a color alone (PROC-18).
	let { level, version, label }: { level: Level; version?: number; label?: string } = $props();
	const Icon = $derived(LEVEL_ICON[level]);
</script>

<span class="inline-flex items-center gap-x-1 rounded-md bg-white/5 px-1.5 py-0.5 text-xs/5 text-gray-200 outline-1 -outline-offset-1 outline-white/10" data-testid="level-badge">
	<Icon class="size-3.5" aria-hidden="true" />
	{label ?? LEVEL_LABEL[level]}{version !== undefined ? ` v${version}` : ''}
</span>
