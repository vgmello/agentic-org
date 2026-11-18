<!--
  @component
  A one-time link, shown once (offices-and-teams-ux §7.4, SIGN-25, SIGN-37): the link, Copy link, when it expires, and
  the line that says it is shown only now. The product sends no email.
-->
<script lang="ts">
	import CopyButton from '$lib/components/CopyButton.svelte';
	import type { IssuedLink } from '../../types';

	let { link, who }: { link: IssuedLink; who: string } = $props();
	const expires = $derived(
		new Intl.DateTimeFormat(undefined, {
			weekday: 'short',
			hour: '2-digit',
			minute: '2-digit'
		}).format(new Date(link.expiresAt))
	);
</script>

<div class="space-y-3">
	<p class="text-sm/6 text-gray-300">
		Send this link to {who}. It works once and expires in 24 hours ({expires}).
		{link.kind === 'invite'
			? `${who} opens it and chooses a username and password.`
			: `${who} opens it and chooses a new password. That signs the account out everywhere.`}
	</p>
	<div class="flex flex-wrap items-center gap-2">
		<input
			readonly
			aria-label={link.kind === 'invite' ? 'Registration link' : 'Set-new-password link'}
			value={link.url}
			class="min-w-0 flex-1 basis-56 rounded-md bg-white/5 px-3 py-1.5 font-mono text-xs/6 text-white outline-1 -outline-offset-1 outline-gray-500 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11 max-[639px]:text-base"
			onfocus={(e) => e.currentTarget.select()}
		/>
		<CopyButton text={link.url} label="Copy link" />
	</div>
	<p class="text-xs/5 text-gray-400">
		It's shown only now. If it's lost, {link.kind === 'invite' ? 'invite' : 'make a new link for'}
		{who} again.
	</p>
</div>
