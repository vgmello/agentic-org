<!--
  @component
  A project's Work tab (work-items-ux §9, PRJ-1). The board and list are built with Work items (plan step 5); until
  then the tab says so honestly (app-shell-ux §0.1) and offers nothing that looks like it works.
-->
<script lang="ts">
	import { projectFrame } from '../frame/context';

	const frame = projectFrame();
</script>

<div class="px-4 py-16 min-[640px]:px-6 min-[1024px]:px-8" data-testid="work-not-built">
	<h4 class="text-base/7 font-semibold text-fg">Work isn't built yet</h4>
	<p class="mt-1 max-w-xl text-sm/6 text-fg-muted">
		It's coming in this release. {frame.project.name}'s tasks, bugs and requests will show here as a
		board and a list.
	</p>
</div>
