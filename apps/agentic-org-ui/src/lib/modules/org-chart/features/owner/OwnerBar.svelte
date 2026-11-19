<!--
  @component
  The owner bar above the reporting tree (offices-and-teams-ux §7.1, OWNER-1): the person who holds the organization,
  "(you)" when it is the viewer, "Owner of <organization>" and "Full access to this organization.", a Profile link
  and, for the owner only, Hand over ownership…. The owner's person is also in the tree, once, where their recorded
  reporting line puts them; this bar is presentation, not a node.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import type { ChartEmployee } from '../../types';

	let {
		owner,
		organizationName,
		isViewer,
		onhandover
	}: {
		owner: ChartEmployee;
		organizationName: string;
		/** The viewer is the owner: "(you)" and Hand over ownership…. */
		isViewer: boolean;
		onhandover: (opener: HTMLElement) => void;
	} = $props();
</script>

<section
	aria-labelledby="owner-bar-name"
	data-owner-bar
	class="flex flex-wrap items-center justify-between gap-x-6 gap-y-4 rounded-lg border border-white/10 bg-black/10 p-4"
>
	<div class="flex min-w-0 flex-1 basis-64 items-center gap-x-4">
		<Avatar name={owner.name} size="lg" class="max-[359px]:hidden" />
		<div class="min-w-0">
			<h3
				id="owner-bar-name"
				tabindex="-1"
				class="text-sm/6 font-semibold text-white outline-none [overflow-wrap:anywhere]"
			>
				{owner.name}{isViewer ? ' (you)' : ''}
				<span class="font-normal text-gray-400">· Owner of {organizationName}</span>
			</h3>
			<p class="text-xs/5 text-gray-400">Full access to this organization.</p>
		</div>
	</div>
	<!-- Labels wrap between words rather than leave the card at 320 px and 200% text. -->
	<div
		class="flex flex-wrap gap-3 max-[639px]:w-full max-[639px]:[&>*]:flex-1 [&>*]:h-auto [&>*]:min-h-9 max-[639px]:[&>*]:min-h-11 [&>*]:whitespace-normal [&>*]:text-center"
	>
		<Button
			variant="outline"
			href={resolve(`/members/${encodeURIComponent(owner.id)}` as '/')}
			class={mobileButton}
			aria-label="Profile of {owner.name}">Profile</Button
		>
		{#if isViewer}
			<Button
				variant="outline"
				class={mobileButton}
				data-handover
				onclick={(event) => onhandover(event.currentTarget)}>Hand over ownership…</Button
			>
		{/if}
	</div>
</section>
