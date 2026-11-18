<!--
  @component
  Copies one piece of text and says so: "Copy command" becomes "Copied". The text is never shown by the button itself.
  Usage: <CopyButton text={command} label="Copy command" />
-->
<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { Button } from '$lib/components/ui/button';
	import { mobileButton } from './classes';

	let {
		text,
		label = 'Copy',
		copiedLabel = 'Copied',
		variant = 'outline',
		size = 'default',
		class: className = ''
	}: { text: string; label?: string; copiedLabel?: string; variant?: 'outline' | 'default' | 'secondary'; size?: 'default' | 'sm' | 'xs'; class?: string } = $props();

	let copied = $state(false);
	let timer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => () => clearTimeout(timer));

	async function copy() {
		try {
			await navigator.clipboard.writeText(text);
			copied = true;
			clearTimeout(timer);
			timer = setTimeout(() => (copied = false), 4000);
		} catch {
			toast.error("Couldn't copy. Select the text and copy it yourself.");
		}
	}
</script>

<Button type="button" {variant} {size} class="{mobileButton} {className}" onclick={copy}>{copied ? copiedLabel : label}</Button>
<span class="sr-only" role="status">{copied ? `${copiedLabel}.` : ''}</span>
