<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import type { CommandForm } from '$lib/chat/commands';
	import { Badge } from '$lib/components/ui/badge';

	/** What a slash command or Turn into… opens (CHAT-9). UI only: it fills the form and says what it would do. */
	let { open = $bindable(false), form, ondone }: { open: boolean; form: CommandForm | undefined; ondone: (message: string) => void } = $props();

	let values = $state<Record<string, string>>({});
	let touched = $state(false);
	$effect(() => {
		if (open && form) {
			untrack(() => {
				values = Object.fromEntries(form.fields.map((f) => [f.id, f.value]));
				touched = false;
			});
		}
	});
	const missing = $derived(form?.fields.filter((f) => f.required && (values[f.id] ?? '').trim() === '') ?? []);

	function submit(e: SubmitEvent) {
		e.preventDefault();
		touched = true;
		if (!form || missing.length) return;
		const message = form.done(values);
		open = false;
		ondone(message);
	}
</script>

<Dialog bind:open>
	<DialogContent class="sm:max-w-lg">
		{#if form}
			<DialogHeader>
				<DialogTitle>{form.title}</DialogTitle>
				<DialogDescription>{form.description} <Badge variant="secondary">Sample data</Badge></DialogDescription>
			</DialogHeader>
			<form class="space-y-4" onsubmit={submit} novalidate>
				{#each form.fields as f (f.id)}
					<div class="space-y-2">
						<Label for="cmd-{f.id}">{f.label}{#if !f.required}{' '}<span class="font-normal text-gray-400">(optional)</span>{/if}</Label>
						{#if f.kind === 'text'}
							<Input id="cmd-{f.id}" bind:value={values[f.id]} autocomplete="off" aria-invalid={touched && missing.includes(f) ? true : undefined} />
						{:else if f.kind === 'textarea'}
							<textarea id="cmd-{f.id}" bind:value={values[f.id]} rows="4" class="w-full rounded-md bg-gray-800 px-3 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400"></textarea>
						{:else}
							<select id="cmd-{f.id}" bind:value={values[f.id]} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
								{#each f.options ?? [] as o (o.value)}<option value={o.value}>{o.label}</option>{/each}
							</select>
						{/if}
						{#if touched && missing.includes(f)}<p class="text-sm/6 text-red-400" role="alert">{f.label} is needed.</p>{/if}
					</div>
				{/each}
				<DialogFooter>
					<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
					<Button type="submit">{form.submit}</Button>
				</DialogFooter>
			</form>
		{/if}
	</DialogContent>
</Dialog>
