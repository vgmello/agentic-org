<script lang="ts">
	import { ProjectWork } from '$lib/modules/projects';

	let { data } = $props();
</script>

<ProjectWork snapshot={data.snapshot} />
