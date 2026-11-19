<!--
  @component
  Profile › Activity (our-org-in-the-app §6, §10): what this member did and what happened to them, newest first; each
  row has its actor, kind, text and time.
-->
<script lang="ts">
	import Loaded from '$lib/components/Loaded.svelte';
	import { membersApi } from '../../api';
	import ActivityRows from '../../components/ActivityRows.svelte';
	import { profileContext } from '../profile/context.svelte';
	import { tabResource } from './reload.svelte';

	const ctx = profileContext();
	const activity = tabResource((o, m) => membersApi.activity(o, m));
</script>

<Loaded
	resource={activity}
	label="activity"
	error="Couldn't load {ctx.profile.name}'s activity"
	isEmpty={(rows) => rows.length === 0}
	emptyTitle="No activity yet"
	emptyDescription="Messages, work changes, team changes and runs involving {ctx.profile
		.name} show here."
>
	{#snippet children(rows)}
		<ActivityRows {rows} />
	{/snippet}
</Loaded>
