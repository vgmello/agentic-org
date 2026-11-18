<!--
  @component
  An effective set of processes (processes-ux §2): each with the level it comes from and how it relates to the
  level above, as words (never colour alone).
-->
<script lang="ts">
	import type { ProcessRow } from '../types';

	let { rows }: { rows: ProcessRow[] } = $props();
	const LEVEL = { organization: 'Organization', team: 'Team', project: 'Project' } as const;
	const relation = (row: ProcessRow) =>
		row.relation === 'replaces'
			? 'replaces Organization'
			: row.relation === 'added'
				? 'added here'
				: row.relation === 'not_overridable'
					? 'not overridable'
					: 'inherited';
</script>

<ul role="list" class="divide-y divide-white/5">
	{#each rows as row (row.key)}
		<li
			class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1 px-4 py-4 min-[640px]:px-6 min-[1024px]:px-8"
		>
			<p class="min-w-0 text-sm/6 font-semibold text-white">{row.name}</p>
			<p class="flex flex-wrap items-center gap-x-2 text-xs/5 text-gray-400">
				<span
					class="rounded-md bg-white/5 px-2 py-0.5 text-gray-300 ring-1 ring-white/10 ring-inset"
					>{LEVEL[row.level]}{row.from ? ` ${row.from}` : ''}</span
				>
				<span>rev {row.revision}</span>
				<span aria-hidden="true">·</span>
				<span>{relation(row)}</span>
			</p>
		</li>
	{/each}
</ul>
