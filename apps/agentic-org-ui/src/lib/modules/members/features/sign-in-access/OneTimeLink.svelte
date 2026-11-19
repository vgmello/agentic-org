<!--
  @component
  A one-time sign-in link, shown once (offices-and-teams-ux §7.4, SIGN-25, SIGN-37): who to send it to, when it
  expires, the link with Copy link, and the line that says it is shown only now. The product sends no email.
  Exported from the members module, so Settings › Members and access shows links the same way.
  Usage: <OneTimeLink link={{ url, expiresAt, kind: 'invite' }} who="Sam" />
-->
<script lang="ts" module>
	export interface OneTimeLinkValue {
		url: string;
		/** RFC 3339. */
		expiresAt: string;
		/** A registration link, or a set-new-password link. */
		kind: 'invite' | 'reset';
	}
</script>

<script lang="ts">
	import CopyButton from '$lib/components/CopyButton.svelte';
	import { expiryText } from './expiry';

	let { link, who }: { link: OneTimeLinkValue; who: string } = $props();
	const expires = $derived(expiryText(link.expiresAt));
	const inputId = $props.id();
</script>

<div class="space-y-3">
	<p class="text-sm/6 text-fg-secondary">
		Send this link to {who}. It works once and expires in 24 hours ({expires}).
		{link.kind === 'invite'
			? `${who} opens it and chooses a username and password.`
			: `${who} opens it and chooses a new password. That signs the account out everywhere.`}
	</p>
	<div class="flex flex-wrap items-center gap-2">
		<input
			id={inputId}
			readonly
			aria-label={link.kind === 'invite' ? 'Registration link' : 'Set-new-password link'}
			value={link.url}
			class="min-w-0 flex-1 basis-56 rounded-md bg-surface-raised px-3 py-1.5 font-mono text-xs/6 text-fg outline-1 -outline-offset-1 outline-field-border focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11 max-[639px]:text-base"
			onfocus={(e) => e.currentTarget.select()}
		/>
		<CopyButton text={link.url} label="Copy link" />
	</div>
	<p class="text-xs/5 text-fg-muted">
		It's shown only now. If it's lost, {link.kind === 'invite' ? 'invite' : 'make a new link for'}
		{who} again.
	</p>
</div>
