<!--
  @component
  The four states of a screen's first read: a skeleton, the unavailable page, an error with Retry, then the content.
  Usage: <Loaded {resource} label="roles" error="Couldn't load the roles" place="Roles">{#snippet children(data)}…{/snippet}</Loaded>
-->
<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import NotBuilt from '$lib/components/NotBuilt.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import type { Resource } from '../resource.svelte';

	let {
		resource,
		label,
		error,
		place,
		avatar = false,
		children
	}: {
		resource: Resource<T>;
		/** What is loading, for the skeleton's name: "roles". */
		label: string;
		/** "Couldn't load the roles". */
		error: string;
		place: string;
		avatar?: boolean;
		children: Snippet<[T]>;
	} = $props();
</script>

{#if resource.state === 'loading'}
	<SkeletonRows {label} {avatar} />
{:else if resource.state === 'unavailable'}
	<NotBuilt title={place} {place} area="Organization settings" />
{:else if resource.state === 'error'}
	<ErrorState
		title={error}
		message={resource.message}
		onretry={() => resource.reload()}
		retrying={resource.reloading}
	/>
{:else if resource.data !== undefined}
	{@render children(resource.data)}
{/if}
