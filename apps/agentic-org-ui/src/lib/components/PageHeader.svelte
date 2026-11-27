<!--
  @component
  The header of a page or a detail view: "Title | subtitle" inline on desktop with a 1px x 18px
  shared-divider separator (Prototype Release c773480f1), wrapping cleanly on narrow screens,
  plus an optional real count, right-aligned actions (at most one primary) and, on a detail page,
  one named back link above the heading instead of a breadcrumb (app-shell-ux §6a).

  Default heading level is `h2` (pass `level={1}` on standalone routes that own the page's primary `h1`).
  Usage:
    <PageHeader title="Members" count={12} countLabel="members" description="People and agents.">
      {#snippet actions()}<Button variant="outline">Add a person</Button>{/snippet}
    </PageHeader>
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import InteriorSearchBar from '$lib/components/shell/InteriorSearchBar.svelte';
	import { useShellCommand } from '$lib/platform/workspace/shell-command';

	let {
		title,
		level = 2,
		id = 'shell-page-title',
		count,
		countLabel,
		description,
		back,
		actions,
		showSearch = true,
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
		showSearch?: boolean;
		class?: string;
	} = $props();

	const shellCommand = useShellCommand();
</script>

<header
	class="page-header-lit border-b border-line-subtle px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8 {className}"
>
	{#if back}
		<a
			href={resolve(back.href as '/')}
			class="mb-2 inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
			>{back.label}</a
		>
	{/if}
	<div class="flex flex-wrap items-center justify-between gap-x-6 gap-y-4">
		<div class="ph-title-row min-w-0 flex-1 basis-64">
			<svelte:element
				this={level === 1 ? 'h1' : 'h2'}
				{id}
				tabindex="-1"
				class="flex max-w-full shrink-0 flex-wrap items-baseline gap-x-2.5 text-[clamp(1.375rem,2.2vw,1.875rem)]/[1.22] font-semibold tracking-[-0.03em] text-fg [overflow-wrap:anywhere] focus:outline-none"
			>
				{title}
				{#if count !== undefined}
					<span class="text-sm/6 font-normal text-fg-muted"
						><span aria-hidden="true">{count}</span><span class="sr-only"
							>, {count} {countLabel ?? ''}</span
						></span
					>
				{/if}
			</svelte:element>
			{#if description}
				<span class="ph-sep" aria-hidden="true"></span>
				{#if typeof description === 'string'}
					<p class="ph-sub max-w-3xl text-sm/6 text-fg-muted">{description}</p>
				{:else}
					<div class="ph-sub max-w-3xl text-sm/6 text-fg-muted">{@render description()}</div>
				{/if}
			{/if}
		</div>
		{#if actions || (showSearch && shellCommand)}
			<div class="flex max-w-full min-w-0 flex-wrap items-center justify-end gap-3">
				{#if actions}{@render actions()}{/if}
				{#if showSearch && shellCommand}
					<InteriorSearchBar class="w-full max-w-full sm:w-[clamp(18rem,28vw,25rem)]" />
				{/if}
			</div>
		{/if}
	</div>
</header>

<style>
	.page-header-lit {
		container-type: inline-size;
	}
	.ph-title-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 12px;
	}
	.ph-sep {
		display: inline-block;
		flex: none;
		align-self: center;
		width: 1px;
		height: 18px;
		background: var(--toolbar-divider-color, var(--line));
	}
	.ph-sub {
		flex: 1 1 16rem;
		margin: 0;
		min-width: 0;
	}
	@media (max-width: 639px) {
		.ph-title-row {
			flex-direction: column;
			align-items: stretch;
			gap: 4px;
		}
		.ph-sep {
			display: none;
		}
		.ph-sub {
			flex: none;
		}
	}
	@container (max-width: 30rem) {
		.ph-title-row {
			flex-direction: column;
			align-items: stretch;
			gap: 4px;
		}
		.ph-sep {
			display: none;
		}
		.ph-sub {
			flex: none;
		}
	}
</style>
