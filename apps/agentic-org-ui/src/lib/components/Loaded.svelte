<!--
  @component
  Renders one `Resource` in its states: skeleton rows while loading, the unavailable page, an error with Retry, the
  empty state, or the content. The caller gives the words; nothing here invents data.
  Usage: <Loaded {resource} label="roles" error="Couldn't load the roles" place="Roles" area="Organization settings">{#snippet children(data)}…{/snippet}</Loaded>
-->
<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';
	import EmptyState from './EmptyState.svelte';
	import ErrorState from './ErrorState.svelte';
	import NotBuilt from './NotBuilt.svelte';
	import SkeletonRows from './SkeletonRows.svelte';
	import type { Resource } from './resource.svelte';

	let {
		resource,
		label,
		error,
		place,
		area,
		isEmpty,
		emptyTitle,
		emptyDescription,
		emptyLevel = 3,
		errorLevel = 2,
		emptyAction,
		avatar = false,
		rows = 4,
		children
	}: {
		resource: Resource<T>;
		/** What is loading, for the skeleton's name: "roles". */
		label: string;
		/** "Couldn't load the roles". */
		error: string;
		/** An unavailable read shows the route placeholder (NotBuilt) when both `place` and `area` are set. */
		place?: string;
		area?: string;
		isEmpty?: (value: T) => boolean;
		emptyTitle?: string;
		emptyDescription?: string;
		emptyLevel?: 2 | 3 | 4;
		/** The error heading's level, as for EmptyState. */
		errorLevel?: 2 | 3 | 4;
		emptyAction?: Snippet;
		avatar?: boolean;
		rows?: number;
		children: Snippet<[T]>;
	} = $props();
</script>

{#if resource.state === 'loading'}
	<SkeletonRows {label} {rows} {avatar} />
{:else if resource.state === 'unavailable'}
	{#if place && area}
		<NotBuilt title={place} {place} {area} />
	{:else}
		<div class="px-4 py-12 min-[640px]:px-6 min-[1024px]:px-8">
			<p class="text-sm/6 font-semibold text-fg">Not available yet</p>
			<p class="mt-1 max-w-xl text-sm/6 text-fg-muted">
				This server doesn't serve {label} yet. Nothing is shown rather than a guess.
			</p>
		</div>
	{/if}
{:else if resource.state === 'error'}
	<ErrorState
		level={errorLevel}
		title={error}
		message={resource.message}
		onretry={() => resource.reload()}
		retrying={resource.reloading}
	/>
{:else if resource.data !== undefined}
	{#if isEmpty?.(resource.data) && emptyTitle}
		<EmptyState
			title={emptyTitle}
			description={emptyDescription}
			level={emptyLevel}
			action={emptyAction}
		/>
	{:else}
		{@render children(resource.data)}
	{/if}
{/if}
