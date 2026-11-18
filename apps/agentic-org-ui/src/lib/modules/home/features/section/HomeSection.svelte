<!--
  @component
  One Home section: a heading, its actions (outline: Home has no page primary), and the section's own
  loading skeleton, one-line error with Retry, "not available yet" line, or content. Sections never block each other.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { Button } from '$lib/components/ui/button';

	let {
		title,
		what,
		phase,
		message,
		retrying = false,
		onretry,
		skeleton = 2,
		unavailable,
		actions,
		children
	}: {
		title: string;
		/** What failed to load, for the error line: "your Inbox". */
		what: string;
		phase: 'loading' | 'ready' | 'error' | 'unavailable';
		message?: string;
		retrying?: boolean;
		onretry: () => void;
		skeleton?: number;
		/** What to say when the web API does not serve this yet. */
		unavailable?: string;
		actions?: Snippet;
		children: Snippet;
	} = $props();

	const id = $props.id();
</script>

<section aria-labelledby={id} class="py-6" data-testid="home-section" data-section={title}>
	<div
		class="flex flex-wrap items-center justify-between gap-x-6 gap-y-3 px-4 min-[640px]:px-6 min-[1024px]:px-8"
	>
		<h3 {id} class="text-base/7 font-semibold text-white">{title}</h3>
		{#if actions && phase === 'ready'}<div class="flex flex-wrap items-center gap-3">
				{@render actions()}
			</div>{/if}
	</div>
	<div class="mt-3">
		{#if phase === 'loading'}
			<SkeletonRows label={title.toLowerCase()} rows={skeleton} avatar />
		{:else if phase === 'error'}
			<div
				role="alert"
				class="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 min-[640px]:px-6 min-[1024px]:px-8"
			>
				<p class="min-w-0 text-sm/6 text-gray-300 [overflow-wrap:anywhere]">
					<span class="font-semibold text-red-300">Couldn't load {what}.</span>
					{message ?? ''}
				</p>
				<Button
					variant="outline"
					size="sm"
					class="min-w-20 max-[639px]:min-h-11"
					disabled={retrying}
					aria-busy={retrying}
					onclick={onretry}>{retrying ? 'Retrying…' : 'Retry'}</Button
				>
			</div>
		{:else if phase === 'unavailable'}
			<p class="px-4 text-sm/6 text-gray-400 min-[640px]:px-6 min-[1024px]:px-8">
				{unavailable ?? `${title} isn't available yet.`}
			</p>
		{:else}
			{@render children()}
		{/if}
	</div>
</section>
