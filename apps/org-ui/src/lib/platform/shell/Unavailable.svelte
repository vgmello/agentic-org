<script lang="ts">
	import { page } from '$app/state';
	import { NAV, navFor } from '$lib/nav';
	import Logo from '$lib/components/Logo.svelte';

	const area = $derived(navFor(page.url.pathname)?.name);
	const heading = $derived(area ? `${area} isn't available yet` : "Agentic Org isn't ready in the browser yet");
</script>

<svelte:head><title>{area ? `${area} · Agentic Org` : 'Agentic Org'}</title></svelte:head>

<div class="min-h-screen bg-gray-800 text-gray-100">
	<header class="flex flex-wrap items-center gap-3 border-b border-white/10 px-6 py-5">
		<Logo class="size-8" /><a href="/" class="rounded-sm text-lg font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring">Agentic Org</a>
	</header>
	<div class="mx-auto grid max-w-6xl gap-8 px-6 py-10 md:grid-cols-[14rem_1fr]">
		<nav aria-label="Main">
			<ul class="flex flex-wrap gap-2 md:flex-col">
				{#each NAV as item (item.href)}
					<li><a href={item.href} aria-current={navFor(page.url.pathname) === item ? 'page' : undefined} class="block rounded-md px-3 py-2 text-sm font-medium text-gray-300 hover:bg-white/5 focus-visible:outline-2 focus-visible:outline-indigo-400 aria-[current=page]:bg-white/10 aria-[current=page]:text-white">{item.name}</a></li>
				{/each}
			</ul>
		</nav>
		<main aria-labelledby="unavailable-title" class="min-w-0 rounded-xl border border-white/10 bg-white/5 p-8" data-testid="area-unavailable">
			<h1 id="unavailable-title" class="text-2xl font-semibold">{heading}</h1>
			<p class="mt-3 text-base/7 text-gray-400">
				{#if area}This release of the server doesn't serve {area} in the browser. Nothing is wrong with your organization. To see what this server can do now, run
				{:else}This server is running, but this release doesn't serve its pages in the browser yet. To see what it can do now, run
				{/if}
				<code class="rounded bg-white/5 px-1 font-mono text-sm text-gray-200">agentic-org doctor</code> on the HQ machine.
			</p>
		</main>
	</div>
</div>
