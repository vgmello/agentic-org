<!--
  @component
  A person's sign-in access on their profile (offices-and-teams-ux §7.4, SIGN-25, SIGN-36, SIGN-37): whether they can
  sign in, and for the owner or the administrator the actions (SignInAccessActions). Agents have none of this: an
  account is never tied to an agent, so the caller renders it only for people.
-->
<script lang="ts">
	import DescriptionList from '$lib/components/DescriptionList.svelte';
	import Section from '$lib/components/Section.svelte';
	import type { MemberProfile } from '../../types';
	import { expiryUntil } from './expiry';
	import SignInAccessActions from './SignInAccessActions.svelte';

	let {
		organizationId,
		organizationName,
		profile,
		onchanged
	}: {
		organizationId: string;
		organizationName: string;
		profile: MemberProfile;
		/** After a confirmed change: the profile reloads. */
		onchanged: () => Promise<void>;
	} = $props();

	const canManage = $derived(profile.viewer.isOwnerOrAdmin && profile.status !== 'inactive');

	const status = $derived(
		profile.canSignIn
			? 'Can sign in'
			: profile.invitePending
				? `No account yet. Invited: the link works until ${expiryUntil(profile.invitePending.expiresAt)}.`
				: 'No account yet'
	);
</script>

<Section
	level={3}
	title="Sign-in access"
	description="An account lets a person sign in to {organizationName} in the browser. The product sends no email."
>
	<DescriptionList items={[{ term: 'Access', value: status }]} />
	{#if canManage}
		<div class="px-4 pt-4 min-[640px]:px-6 min-[1024px]:px-8">
			<SignInAccessActions {organizationId} {organizationName} person={profile} {onchanged} />
		</div>
	{/if}
</Section>
