<!--
  @component
  About (app-shell-ux §0): the product, this app's version and the office it is connected to. Opened from
  the account menu; closing returns focus to the menu's trigger.
-->
<script lang="ts">
	import pkg from '../../../../package.json';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import type { Connection } from '$lib/proto';

	let { open = $bindable(false), connection }: { open: boolean; connection: Connection } = $props();
</script>

<Dialog bind:open>
	<DialogContent class="max-[639px]:max-w-full">
		<DialogHeader>
			<DialogTitle>About Agentic Org</DialogTitle>
			<DialogDescription>This app and the office it is connected to.</DialogDescription>
		</DialogHeader>
		<dl class="divide-y divide-white/5 text-sm/6">
			<div class="flex flex-wrap justify-between gap-x-4 py-3">
				<dt class="text-gray-400">Version</dt>
				<dd class="font-mono text-white">{pkg.version}</dd>
			</div>
			<div class="flex flex-wrap justify-between gap-x-4 py-3">
				<dt class="text-gray-400">Organization</dt>
				<dd class="min-w-0 text-white wrap-break-word">{connection.organization.name}</dd>
			</div>
			<div class="flex flex-wrap justify-between gap-x-4 py-3">
				<dt class="text-gray-400">Office</dt>
				<dd class="min-w-0 text-white wrap-break-word">
					{connection.office.name} office on {connection.office.host}
				</dd>
			</div>
		</dl>
		<DialogFooter>
			<Button variant="outline" class="max-[639px]:h-11" onclick={() => (open = false)}
				>Close</Button
			>
		</DialogFooter>
	</DialogContent>
</Dialog>
