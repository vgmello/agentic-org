<!--
  @component
  Profile › Processes (processes-ux §2, "What this agent follows"; PROC-10): the organization processes and standards
  that apply to its current work, each with its source; with no current work, the organization's, and the spec's
  sentence. Read and shown by the processes module.
-->
<script lang="ts">
	import { readPlace } from '$lib/modules/processes';
	import { PlaceProcesses } from '$lib/modules/processes/ui';
	import { profileContext } from '../profile/context.svelte';
	import { tabResource } from './reload.svelte';

	const ctx = profileContext();
	const processes = tabResource((o, m) => readPlace(o, { kind: 'agent', id: m }));
</script>

<PlaceProcesses
	resource={processes}
	place={{ kind: 'agent', id: ctx.profile.id }}
	name={ctx.profile.name}
	headingLevel={3}
/>
