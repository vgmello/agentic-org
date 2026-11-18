<!--
  @component
  An invitation for another machine, shown once (offices-and-teams-ux §6.2, OFC-3): the secret, Copy invitation, its expiry, and the
  setup steps on the other machine. The secret is entered there through a protected prompt, never as a command-line argument.
-->
<script lang="ts">
	import CopyButton from '$lib/modules/settings/components/CopyButton.svelte';
	import { Button } from '$lib/components/ui/button';
	import { mobileButton } from '$lib/modules/settings/components/classes';
	import type { Issued } from '../../types';

	let { issued, ondone }: { issued: Issued; ondone: () => void } = $props();
	const expires = $derived(
		new Intl.DateTimeFormat(undefined, {
			weekday: 'short',
			hour: '2-digit',
			minute: '2-digit'
		}).format(new Date(issued.invitation.expiresAt))
	);
</script>

<div class="space-y-4" data-testid="invitation-panel">
	<ol class="list-decimal space-y-1 pl-5 text-sm/6 text-gray-300">
		<li>On the other machine, start Agentic Org's office setup.</li>
		<li>
			Paste this invitation when it asks. It's a protected prompt: the invitation never goes on the
			command line.
		</li>
		<li>The setup shows a check code. You type it here to approve the join.</li>
	</ol>
	<div class="flex flex-wrap items-center gap-2">
		<input
			readonly
			aria-label="Invitation"
			value={issued.secret}
			class="min-w-0 flex-1 basis-56 rounded-md bg-white/5 px-3 py-1.5 font-mono text-xs/6 text-white outline-1 -outline-offset-1 outline-gray-500 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-indigo-400 max-[639px]:min-h-11 max-[639px]:text-base"
			onfocus={(e) => e.currentTarget.select()}
		/>
		<CopyButton text={issued.secret} label="Copy invitation" copiedLabel="Copied invitation" />
	</div>
	<p class="text-xs/5 text-gray-400">
		It works once and expires in 24 hours ({expires}). It's shown only now. If it's lost, make a new
		invitation: that cancels this one.
	</p>
	<div class="flex justify-end">
		<Button type="button" class={mobileButton} onclick={ondone}>Done</Button>
	</div>
</div>
