<script lang="ts">
	import UserPlus from '@lucide/svelte/icons/user-plus';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
</script>

<svelte:head><title>Agents · agentic.org</title></svelte:head>

<div class="flex flex-col items-center px-8 py-24 text-center">
	<span class="flex size-12 items-center justify-center rounded-full bg-white/5 text-gray-300">
		<UserPlus class="size-6" aria-hidden="true" />
	</span>
	<h2 class="mt-4 text-base/7 font-semibold text-white">No agents yet</h2>
	<p class="mt-1 max-w-sm text-sm/6 text-gray-400">
		Agents are the AI employees of your organization. Hire the first one to start giving them work.
	</p>
	<div class="mt-6 flex items-center gap-x-3">
		<Button disabled>Hire your first agent</Button>
		<Badge variant="secondary">Coming soon</Badge>
	</div>
</div>
