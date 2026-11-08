<script lang="ts">
	import BuildingOffice from '@lucide/svelte/icons/building-2';
	import FolderOpen from '@lucide/svelte/icons/folder-open';
	import { Badge } from '$lib/components/ui/badge';

	const cardBase =
		'group relative flex flex-col gap-y-4 rounded-xl p-6 text-left outline-1 -outline-offset-1 outline-white/10';
</script>

<svelte:head><title>Welcome · agentic.org</title></svelte:head>

<h1 class="text-3xl/9 font-semibold tracking-tight text-white">Welcome to agentic.org</h1>
<p class="mt-2 text-base/7 text-gray-400">
	Run an organization of AI agents. Start a new one, or open one you already have.
</p>

<div class="mt-10 grid gap-4 sm:grid-cols-2">
	<a
		href="/create"
		class="{cardBase} bg-white/5 hover:bg-white/10 focus-visible:outline-2 focus-visible:outline-indigo-500"
	>
		<span class="flex size-11 items-center justify-center rounded-lg bg-indigo-500 text-white">
			<BuildingOffice class="size-6" aria-hidden="true" />
		</span>
		<span>
			<span class="block text-base/7 font-semibold text-white">Create organization</span>
			<span class="mt-1 block text-sm/6 text-gray-400">
				Set up a new organization with its own office on this Mac.
			</span>
		</span>
	</a>

	<button type="button" disabled class="{cardBase} cursor-not-allowed bg-white/[0.03] opacity-70">
		<span class="flex size-11 items-center justify-center rounded-lg bg-white/10 text-gray-300">
			<FolderOpen class="size-6" aria-hidden="true" />
		</span>
		<span>
			<span class="flex items-center gap-x-2 text-base/7 font-semibold text-white">
				Open organization
				<Badge variant="secondary">Coming soon</Badge>
			</span>
			<span class="mt-1 block text-sm/6 text-gray-400">
				Connect to an organization that already runs on an office.
			</span>
		</span>
	</button>
</div>
