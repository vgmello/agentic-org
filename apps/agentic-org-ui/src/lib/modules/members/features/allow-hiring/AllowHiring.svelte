<!--
  @component
  Allow hiring (our-org-in-the-app §7.1, APPR-1): whether this employee may hire into the teams it leads and the teams
  under them. Only the owner, or a person above the employee, changes it, and nobody their own; otherwise the reason
  is shown instead of a control. The change is confirmed first and shown only once HQ accepts it.
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { membersApi } from '../../api';
	import type { MemberProfile } from '../../types';

	let {
		organizationId,
		profile,
		onchanged
	}: {
		organizationId: string;
		profile: MemberProfile;
		onchanged: () => Promise<void>;
	} = $props();

	let open = $state(false);
	let opener = $state<HTMLButtonElement | null>(null);
	const on = $derived(profile.allowHiring.on);
	const leads = $derived(profile.leads.map((t) => t.name).join(', '));

	async function change() {
		await membersApi.setAllowHiring(organizationId, profile.id, !on);
		toast.success(on ? `${profile.name} can no longer hire.` : `${profile.name} can now hire.`);
		await onchanged();
	}
</script>

<div class="flex flex-wrap items-baseline gap-x-4 gap-y-2">
	<span class="min-w-0 flex-1 basis-48">
		{on ? 'On' : 'Off'}{on
			? leads
				? `: may hire into ${leads} and the teams under them`
				: ': leads no team yet, so there is nowhere to hire into'
			: ''}
		{#if !profile.allowHiring.canChange && profile.allowHiring.reason}
			<span class="block text-xs/5 text-gray-400">{profile.allowHiring.reason}</span>
		{/if}
	</span>
	{#if profile.allowHiring.canChange && profile.status !== 'inactive'}
		<Button
			bind:ref={opener}
			variant="outline"
			size="sm"
			class={mobileButton}
			onclick={() => (open = true)}>{on ? 'Turn off…' : 'Turn on…'}</Button
		>
	{/if}
</div>

<ConfirmDialog
	bind:open
	title={on ? `Stop ${profile.name} from hiring?` : `Let ${profile.name} hire?`}
	description={on
		? `${profile.name} can't start a hire any more. Hires already made stay.`
		: `${profile.name} can hire into the teams they lead and the teams under them. No role or title grants this.`}
	confirmLabel={on ? 'Turn off' : 'Turn on'}
	busyLabel={on ? 'Turning off…' : 'Turning on…'}
	onconfirm={change}
	onCloseAutoFocus={(e) => {
		e.preventDefault();
		opener?.focus();
	}}
/>
