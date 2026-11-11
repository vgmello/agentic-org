<script lang="ts">
	import OfficeStatusBadge from '$lib/components/OfficeStatusBadge.svelte';
	const statuses = ['active', 'starting', 'pending', 'offline', 'unavailable', 'update_needed', 'unreachable', 'checking', 'status_unknown', undefined] as const;
</script>

<section aria-label="Office status contracts" class="fixed inset-4 z-[100] overflow-auto bg-gray-800 p-4">
	{#each statuses as status}
		<div data-office-status={status ?? 'undefined'} class="mb-3 w-40 max-w-full">
			<OfficeStatusBadge {status} />
		</div>
	{/each}
	<div data-office-status="future" class="w-40 max-w-full">
		<!-- A future runtime value must retain the existing unknown fallback. -->
		<OfficeStatusBadge status={'future' as never} />
	</div>
</section>
