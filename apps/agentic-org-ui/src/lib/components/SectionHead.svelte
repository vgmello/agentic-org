<!--
  @component
  Shared section header (Prototype Release 3f647b0f): title, muted count, optional Report-theme
  title override, and right-aligned action snippet. Used across Home and Project Overview.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { uiPrefs } from '$lib/platform/shell/ui-preferences.svelte';

	let {
		id,
		title,
		reportTitle,
		count = '',
		right,
		action,
		class: className = ''
	}: {
		id?: string;
		title: string;
		reportTitle?: string;
		count?: string | number;
		right?: Snippet;
		action?: Snippet;
		class?: string;
	} = $props();

	const isReport = $derived(uiPrefs.theme === 'report' || uiPrefs.theme === 'report-dark');
	const displayTitle = $derived(isReport && reportTitle ? reportTitle : title);
	const displayCount = $derived(count === '' || count === undefined ? '' : String(count));
	const trailing = $derived(action ?? right);
</script>

<div class={['ao-sec-head', className]}>
	<div class="ao-sec-head-l">
		<h2 {id} class="ao-sec-head-title">{displayTitle}</h2>
		{#if displayCount}
			<span class="ao-sec-head-count">{displayCount}</span>
		{/if}
	</div>
	{#if trailing}
		<div class="ao-sec-head-r">{@render trailing()}</div>
	{/if}
</div>

<style>
	.ao-sec-head {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		justify-content: space-between;
		gap: 0.5rem 0.75rem;
	}
	.ao-sec-head-l {
		display: flex;
		flex-wrap: wrap;
		align-items: baseline;
		gap: 0.25rem 0.625rem;
		min-width: 0;
	}
	.ao-sec-head-title {
		margin: 0;
		color: var(--exec-ink, var(--fg));
		font-size: var(--exec-section-title-size, 1rem);
		font-weight: var(--exec-section-title-weight, 600);
		letter-spacing: var(--exec-section-title-tracking, -0.015em);
		line-height: 1.3;
	}
	.ao-sec-head-count {
		color: var(--exec-muted, var(--fg-muted));
		font-size: 0.8125rem;
		font-weight: 450;
		font-variant-numeric: tabular-nums;
	}
	.ao-sec-head-r {
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
		flex-shrink: 0;
	}
</style>
