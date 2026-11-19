<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';
	import { initialsOf } from '$lib/format';
	import type { Member } from '$lib/proto';

	let { owner, organization, sample = false }: { owner: Member | undefined; organization: string; /** The data is invented: say so where the owner is shown. */ sample?: boolean } = $props();
</script>

<!-- The owner stays above every view, outside team membership and counts (OWNER-1). -->
{#if owner}
	<section aria-label="Owner" class="flex min-w-0 items-center gap-x-3">
		<span aria-hidden="true" class="inline-flex size-10 flex-none items-center justify-center rounded-full bg-surface text-sm font-semibold text-fg-secondary outline-1 -outline-offset-1 outline-line">{initialsOf(owner.name)}</span>
		<span class="min-w-0 text-sm/6 text-fg-muted">
			<span class="font-semibold text-fg">{owner.name}</span> · Owner
			{#if sample} <Badge variant="secondary" data-testid="owner-sample">Sample data</Badge>{/if}
			<span class="sr-only"> of {organization}. Full access to every team, member and project. Any agent can message, ask or assign you.</span>
		</span>
	</section>
{/if}
