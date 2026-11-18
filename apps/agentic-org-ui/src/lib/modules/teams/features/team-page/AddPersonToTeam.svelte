<!--
  @component
  A team page's one primary action (§7.2, Hire or add): Add a person, opening the organization's existing flow
  with this team chosen. Hire an agent joins it once the Hire flow (our-org-in-the-app §7) is built.
  Success is said only after HQ accepts it.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import AddPersonSheet from '$lib/components/AddPersonSheet.svelte';
	import MiniIcon from '$lib/components/MiniIcon.svelte';
	import { Button } from '$lib/components/ui/button';
	import { withoutFormer } from '$lib/org/model';
	import type { OrgSession } from '$lib/org/session';
	import type { Connection, OrgSnapshot } from '$lib/proto';

	let {
		connection,
		snapshot,
		session,
		teamId,
		onadded
	}: {
		connection: Connection;
		snapshot: OrgSnapshot;
		session: OrgSession;
		teamId: string;
		/** After HQ accepted the new person: re-read the team. */
		onadded: () => Promise<void>;
	} = $props();

	let open = $state(false);

	async function added(r: { name: string; problems: string[] }) {
		await onadded();
		if (r.problems.length === 0) toast.success(`Added ${r.name}. Agents can now reach ${r.name}.`);
		else
			toast.warning(`Added ${r.name}, but not everything went through.`, {
				description: r.problems.join(' ')
			});
	}
</script>

<Button class="max-[639px]:h-11" onclick={() => (open = true)}>
	<MiniIcon name="plus" class="-ml-0.5 size-5" /> Add a person
</Button>

<AddPersonSheet
	bind:open
	snapshot={withoutFormer(snapshot)}
	{session}
	organization={connection.organization.name}
	presetTeamId={teamId}
	ondone={added}
/>
