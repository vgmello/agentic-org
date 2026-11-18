<!--
  @component
  The header of a page or a detail view: one heading, an optional real count, optional description, the
  page's actions on the right (at most one primary) and, on a detail page, one named back link above the
  heading instead of a breadcrumb (app-shell-ux §6a).

  The shell's top bar holds the page's `h1` (the area name), so a page header is an `h2` by default.
  Usage:
    <PageHeader title="Members" count={12} countLabel="members" description="People and agents.">
      {#snippet actions()}<Button variant="outline">Add a person</Button>{/snippet}
    </PageHeader>
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { resolve } from '$app/paths';

	let {
		title,
		level = 2,
		id,
		count,
		countLabel,
		description,
		back,
		actions,
		class: className = ''
	}: {
		title: string;
		level?: 1 | 2;
		/** The heading's id, for `aria-labelledby`. */
		id?: string;
		/** A real count shown beside the heading; omitted while unknown. */
		count?: number;
		/** Spoken after the count, plural-correct: "12 members". */
		countLabel?: string;
		description?: string | Snippet;
		back?: { href: string; label: string };
		actions?: Snippet;
		class?: string;
	} = $props();
</script>

<header class="border-b border-white/5 px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8 {className}">
	{#if back}
		<a
			href={resolve(back.href as '/')}
			class="mb-2 inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-indigo-400 hover:text-indigo-300 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
			>{back.label}</a
		>
	{/if}
	<div class="flex flex-wrap items-start justify-between gap-x-6 gap-y-4">
		<div class="min-w-0 flex-1 basis-64">
			<svelte:element
				this={level === 1 ? 'h1' : 'h2'}
				{id}
				class="flex flex-wrap items-baseline gap-x-2 text-base/7 font-semibold text-white [overflow-wrap:anywhere]"
			>
				{title}
				{#if count !== undefined}
					<span class="text-sm/6 font-normal text-gray-400"
						><span aria-hidden="true">{count}</span><span class="sr-only"
							>, {count} {countLabel ?? ''}</span
						></span
					>
				{/if}
			</svelte:element>
			{#if typeof description === 'string'}
				<p class="mt-1 max-w-3xl text-sm/6 text-gray-400">{description}</p>
			{:else if description}
				<div class="mt-1 max-w-3xl text-sm/6 text-gray-400">{@render description()}</div>
			{/if}
		</div>
		{#if actions}<div class="flex flex-wrap items-center gap-3">{@render actions()}</div>{/if}
	</div>
</header>
