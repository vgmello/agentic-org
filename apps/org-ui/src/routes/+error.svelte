<script lang="ts">
	import { page } from '$app/state';
</script>

<main class="flex h-full flex-col items-center justify-center gap-y-2 px-6 text-center">
	<p class="text-base/7 font-semibold text-indigo-400">{page.status}</p>
	<h1 class="text-2xl font-semibold tracking-tight text-white">Something went wrong</h1>
	<p class="text-sm/6 text-gray-400">{page.error?.message}</p>
	<a href="/" class="mt-4 text-sm/6 font-semibold text-white hover:text-gray-300">Back to the start</a>
</main>
