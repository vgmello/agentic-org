<!--
  @component
  Breadcrumb trail for nested pages (TASK-180, Prototype Release 3f647b0f):
  Projects › project, Teams › team, Teams › Members › member, Settings › Agent setup › agent.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import ChevronRight from '@lucide/svelte/icons/chevron-right';

	export interface BreadcrumbItem {
		label: string;
		href?: string;
	}

	let { items, class: className = '' }: { items: readonly BreadcrumbItem[]; class?: string } =
		$props();
</script>

<nav aria-label="Breadcrumb" class={['ao-crumbs', className]} data-testid="breadcrumbs">
	<ol class="ao-crumbs-list">
		{#each items as item, index (`${index}-${item.label}`)}
			{@const isLast = index === items.length - 1}
			<li class="ao-crumb">
				{#if isLast || !item.href}
					<span aria-current={isLast ? 'page' : undefined} class="ao-crumb-cur">{item.label}</span>
				{:else}
					<a href={resolve(item.href as '/')} class="ao-crumb-link">{item.label}</a>
					<ChevronRight class="ao-crumb-sep size-3.5 shrink-0" aria-hidden="true" />
				{/if}
			</li>
		{/each}
	</ol>
</nav>

<style>
	.ao-crumbs {
		display: flex;
		align-items: center;
		padding: 0.75rem 1rem 0;
		font-size: 0.8125rem;
		line-height: 1.25rem;
	}
	@media (min-width: 640px) {
		.ao-crumbs {
			padding-inline: 1.5rem;
		}
	}
	@media (min-width: 1024px) {
		.ao-crumbs {
			padding-inline: 2rem;
		}
	}
	.ao-crumbs-list {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.25rem;
		margin: 0;
		padding: 0;
		list-style: none;
	}
	.ao-crumb {
		display: inline-flex;
		align-items: center;
		gap: 0.25rem;
		min-width: 0;
	}
	.ao-crumb-link {
		color: var(--exec-muted, var(--fg-muted));
		font-weight: 500;
		text-decoration: none;
		border-radius: 0.25rem;
		padding: 0.125rem 0.25rem;
		margin-inline: -0.25rem;
		transition: color 120ms ease;
	}
	.ao-crumb-link:hover {
		color: var(--exec-ink, var(--fg));
		text-decoration: underline;
		text-underline-offset: 2px;
	}
	.ao-crumb-link:focus-visible {
		outline: 2px solid var(--focus);
		outline-offset: 2px;
	}
	.ao-crumb-cur {
		color: var(--exec-body, var(--fg-secondary));
		font-weight: 550;
		overflow-wrap: anywhere;
	}
	:global(.ao-crumb-sep) {
		color: var(--exec-dim, var(--fg-subtle));
	}
	@media (prefers-reduced-motion: reduce) {
		.ao-crumb-link {
			transition: none;
		}
	}
</style>
