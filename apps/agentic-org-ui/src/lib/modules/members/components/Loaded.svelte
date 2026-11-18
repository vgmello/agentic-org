<!--
  @component
  Renders one `resource` in its states: skeleton rows while loading, the honest unavailable line, an error with
  Retry, the empty state, or the content. The caller gives the words; nothing here invents data.
-->
<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import type { Resource } from '../resource.svelte';

	let {
		res,
		label,
		errorTitle,
		isEmpty,
		emptyTitle,
		emptyDescription,
		emptyLevel = 3,
		emptyAction,
		avatar = false,
		rows = 4,
		children
	}: {
		res: Resource<T>;
		/** Spoken while loading: "Loading {label}". */
		label: string;
		errorTitle: string;
		isEmpty?: (value: T) => boolean;
		emptyTitle?: string;
		emptyDescription?: string;
		emptyLevel?: 2 | 3;
		emptyAction?: Snippet;
		avatar?: boolean;
		rows?: number;
		children: Snippet<[T]>;
	} = $props();
</script>

{#if res.state === 'loading'}
	<SkeletonRows {label} {rows} {avatar} />
{:else if res.state === 'unavailable'}
	<div class="px-4 py-12 min-[640px]:px-6 min-[1024px]:px-8">
		<p class="text-sm/6 font-semibold text-white">Not available yet</p>
		<p class="mt-1 max-w-xl text-sm/6 text-gray-400">
			This server doesn't serve {label} yet. Nothing is shown rather than a guess.
		</p>
	</div>
{:else if res.state === 'error'}
	<ErrorState
		title={errorTitle}
		message={res.error}
		onretry={() => res.load()}
		retrying={res.retrying}
	/>
{:else if res.value !== undefined}
	{#if isEmpty?.(res.value) && emptyTitle}
		<EmptyState
			title={emptyTitle}
			description={emptyDescription}
			level={emptyLevel}
			action={emptyAction}
		/>
	{:else}
		{@render children(res.value)}
	{/if}
{/if}
