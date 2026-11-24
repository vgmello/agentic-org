<!--
  @component
  Actionable insight sentences (costs-and-insights-ux decision 7, INS-3):
  each sentence points at something to do with an authorized link and explicit sample/coverage limitation.
  Uses Lucide icons (never emoji) and neutral/amber look tokens.
-->
<script lang="ts">
	import AlertTriangleIcon from '@lucide/svelte/icons/triangle-alert';
	import InfoIcon from '@lucide/svelte/icons/info';
	import { resolve } from '$app/paths';
	import { gutters } from '$lib/components/classes';
	import type { InsightNotice } from '../../types';

	let { insights }: { insights: InsightNotice[] } = $props();

	const headingId = $props.id();
</script>

{#if insights.length > 0}
	<section
		aria-labelledby={headingId}
		class="border-t border-line-subtle py-6 {gutters}"
		data-testid="insights-list"
	>
		<h3 id={headingId} class="text-base/7 font-semibold text-fg">Insights</h3>
		<ul role="list" class="mt-3 flex flex-col gap-2">
			{#each insights as item (item.id)}
				<li
					class="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 rounded-lg border px-4 py-3 {item.tone ===
					'owner'
						? 'border-warning/30 bg-warning/10'
						: 'border-line-subtle bg-surface-raised/50'}"
					data-insight={item.id}
				>
					<div class="flex min-w-0 flex-1 basis-56 items-start gap-2.5">
						<span
							aria-hidden="true"
							class="mt-0.5 shrink-0 {item.tone === 'owner'
								? 'text-warning-text-strong'
								: 'text-fg-muted'}"
						>
							{#if item.tone === 'owner'}
								<AlertTriangleIcon class="size-4" />
							{:else}
								<InfoIcon class="size-4" />
							{/if}
						</span>
						<div class="min-w-0 flex-1">
							<p class="text-sm/6 text-fg [overflow-wrap:anywhere]">{item.text}</p>
							{#if item.limitation}
								<p class="mt-0.5 text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
									{item.limitation}
								</p>
							{/if}
						</div>
					</div>
					<a
						href={resolve(item.href as '/')}
						class="inline-flex min-h-8 shrink-0 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
						>{item.actionLabel}</a
					>
				</li>
			{/each}
		</ul>
	</section>
{/if}
