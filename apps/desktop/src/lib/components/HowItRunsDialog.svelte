<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { setRunMode } from '$lib/org/actions';
	import { RUN_MODES, runLimitsText } from '$lib/org/agents';
	import type { OrgSession } from '$lib/org/session';
	import type { Member, RunMode } from '$lib/proto';

	let {
		open = $bindable(false),
		agent,
		session,
		ondone
	}: {
		open: boolean;
		agent: Member | undefined;
		session: OrgSession;
		ondone: (result: { name: string; stale?: boolean }) => void;
	} = $props();

	// '' is "started by a person" (the office never starts it).
	let mode = $state<string>('on_call');
	let spec = $state('');
	let zone = $state('');
	let error = $state<string | undefined>();
	let busy = $state(false);
	let shown = $state<Member | undefined>();

	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!agent?.agent) return;
				const a = agent.agent;
				shown = agent;
				mode = a.run_mode === undefined ? '' : (RUN_MODES.some((m) => m.id === a.run_mode) ? a.run_mode : 'unknown'); // a mode this app can't read stays unset until one is picked
				spec = a.schedule?.spec ?? '';
				zone = a.schedule?.time_zone ?? Intl.DateTimeFormat().resolvedOptions().timeZone ?? '';
				error = undefined;
			});
		}
	});

	const scheduleMissing = $derived((mode === 'scheduled' && (spec.trim() === '' || zone.trim() === '')) || mode === 'unknown');

	// After a stale edit (HQ changed the agent meanwhile) the page reloads: take the latest as the baseline and the
	// version to send, but leave anything the owner typed alone, so "Here's the latest" is true and a retry can work.
	$effect(() => {
		const latest = agent;
		untrack(() => {
			if (open && shown && latest && latest.id === shown.id && latest.version !== shown.version) shown = latest;
		});
	});

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (!shown || busy || scheduleMissing) return;
		busy = true;
		try {
			await setRunMode(session, shown.id, mode === '' ? undefined : (mode as RunMode), mode === 'scheduled' ? { spec: spec.trim(), time_zone: zone.trim() } : undefined, shown.version);
			open = false;
			ondone({ name: shown.name });
		} catch (e) {
			const failure = toAppError(e);
			if (failure.code === 'conflict') {
				error = `${shown.name} changed at HQ while you were editing. Here's the latest.`;
				ondone({ name: shown.name, stale: true });
			} else error = failure.message;
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent>
		<form class="grid gap-4" onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle class="[overflow-wrap:anywhere]">How {shown?.name} runs</DialogTitle>
				<DialogDescription>On call and scheduled agents only cost while they run.</DialogDescription>
			</DialogHeader>
			<fieldset class="space-y-1" disabled={busy}>
				<legend class="sr-only">Run mode</legend>
				{#if mode === 'unknown'}
					<label class="flex items-start gap-x-3 rounded-md px-2 py-1.5 text-sm/6">
						<input type="radio" name="run-mode" value="unknown" class="mt-1" checked disabled />
						<span><span class="block text-white">Not recognised</span><span class="block text-xs/5 text-gray-400">This app can't read how it runs. Choose one.</span></span>
					</label>
				{/if}
				{#each RUN_MODES as m (m.id)}
					<label class="flex items-start gap-x-3 rounded-md px-2 py-1.5 text-sm/6 hover:bg-white/5">
						<input type="radio" name="run-mode" value={m.id} class="mt-1" bind:group={mode} />
						<span><span class="block text-white">{m.label}</span><span class="block text-xs/5 text-gray-400">{m.help}</span></span>
					</label>
				{/each}
				<label class="flex items-start gap-x-3 rounded-md px-2 py-1.5 text-sm/6 hover:bg-white/5">
					<input type="radio" name="run-mode" value="" class="mt-1" bind:group={mode} />
					<span><span class="block text-white">Started by a person</span><span class="block text-xs/5 text-gray-400">The office never starts it: you start it by hand.</span></span>
				</label>
			</fieldset>
			{#if mode === 'scheduled'}
				<div class="grid gap-3 rounded-lg bg-white/5 p-3 outline-1 -outline-offset-1 outline-white/10">
					<div class="space-y-2">
						<Label for="run-spec">When</Label>
						<Input id="run-spec" bind:value={spec} disabled={busy} autocomplete="off" placeholder="Weekdays at 9, every 30 minutes…" />
					</div>
					<div class="space-y-2">
						<Label for="run-zone">Time zone</Label>
						<Input id="run-zone" bind:value={zone} disabled={busy} autocomplete="off" placeholder="Europe/Lisbon" class="font-mono" />
					</div>
				</div>
			{/if}
			{@const limits = runLimitsText(mode)}
			{#if limits}<p class="text-xs/5 text-gray-400" data-testid="run-limits">{limits}</p>{/if}
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || scheduleMissing}>Save</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
