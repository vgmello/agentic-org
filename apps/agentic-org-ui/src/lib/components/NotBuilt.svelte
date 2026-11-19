<!--
  @component
  The honest placeholder for a route (app-shell-ux §0.1): an included screen awaiting implementation says
  "{Title} isn't built yet" / "It's coming in this release."; a section outside this release says
  "Not in this release" and why (`notInRelease`). Never "Coming soon". It sets the document title.
  Builders replace the page that renders this; nothing else needs to change.
  Usage: <NotBuilt title="Inbox" area="Inbox" />  ·  <NotBuilt title="Offices" place="Offices" area="Organization settings" />
-->
<script lang="ts">
	import { pageTitle } from '$lib/nav';

	let {
		title,
		place,
		area,
		notInRelease
	}: {
		/** The screen's name in the heading. */
		title: string;
		/** The place inside the area, for the document title; omit when the screen is the area itself. */
		place?: string;
		area: string;
		/** For a section outside this release: the sentence explaining it. */
		notInRelease?: string;
	} = $props();
</script>

<svelte:head><title>{pageTitle(place, area)}</title></svelte:head>

<div class="px-4 py-16 min-[640px]:px-6 min-[1024px]:px-8">
	<h2 class="text-base/7 font-semibold text-white">
		{notInRelease ? 'Not in this release' : `${title} isn't built yet`}
	</h2>
	<p class="mt-1 max-w-xl text-sm/6 text-gray-400">
		{notInRelease ?? "It's coming in this release."}
	</p>
</div>
