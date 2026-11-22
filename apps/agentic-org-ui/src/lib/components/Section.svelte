<!--
  @component
  A titled block within a page (a settings group, a profile tab's part). Renders a `section` named by its
  heading (h2 by default; h3 under a PageHeader's h2), optional description and actions, then the content.
  Usage: <Section title="Members" description="Who is in this team.">…rows…</Section>
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		title,
		description,
		level = 2,
		actions,
		children,
		class: className = ''
	}: {
		title: string;
		description?: string;
		level?: 2 | 3;
		actions?: Snippet;
		children: Snippet;
		class?: string;
	} = $props();

	const id = $props.id();
</script>

<section aria-labelledby={id} class="py-6 {className}">
	<div
		class="flex flex-wrap items-start justify-between gap-x-6 gap-y-3 px-4 min-[640px]:px-6 min-[1024px]:px-8"
	>
		<div class="min-w-0 flex-1 basis-56">
			<svelte:element
				this={level === 2 ? 'h2' : 'h3'}
				{id}
				class="text-base/7 font-semibold text-fg">{title}</svelte:element
			>
			{#if description}<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">{description}</p>{/if}
		</div>
		{#if actions}<div class="flex flex-wrap items-center gap-3">{@render actions()}</div>{/if}
	</div>
	<div class="mt-4">{@render children()}</div>
</section>
