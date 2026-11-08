<script lang="ts">
	import { untrack } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { toAppError } from '$lib/office/api';
	import { updateAgent, type AgentChange } from '$lib/org/actions';
	import { messagingNameError, runtimeLabel } from '$lib/org/agents';
	import type { OrgSession } from '$lib/org/session';
	import type { AgentRuntime, Member, OrgSnapshot } from '$lib/proto';

	let {
		open = $bindable(false),
		agent,
		snapshot,
		session,
		ondone
	}: {
		open: boolean;
		agent: Member | undefined;
		/** The whole organization (also the agents who left): their messaging names stay taken. */
		snapshot: OrgSnapshot;
		session: OrgSession;
		ondone: (result: { name: string; stale?: boolean }) => void;
	} = $props();

	const RUNTIMES = ['claude_code', 'codex', 'antigravity'] as const;
	let name = $state('');
	let role = $state('');
	let title = $state('');
	let focus = $state('');
	let runtime = $state<string>('claude_code');
	let otherRuntime = $state('');
	let model = $state('');
	let home = $state('');
	let handle = $state('');
	let error = $state<string | undefined>();
	let busy = $state(false);
	let shown = $state<Member | undefined>();

	$effect(() => {
		// Only opening re-initialises the form: a reload behind the dialog (a stale edit) must keep what was typed.
		if (open) {
			untrack(() => {
				if (!(agent?.agent)) return;
				const a = agent.agent;
				untrack(() => {
					shown = agent;
					name = agent.name;
					role = a.role;
					title = a.title;
					focus = a.focus ?? '';
					runtime = typeof a.runtime === 'object' ? 'other' : (RUNTIMES as readonly string[]).includes(a.runtime) ? a.runtime : 'unknown'; // an unreadable runtime stays unset until the owner picks one
					otherRuntime = typeof a.runtime === 'object' ? a.runtime.other : '';
					model = a.model ?? '';
					home = a.home ?? '';
					handle = a.messaging_name;
					error = undefined;
				});
			});
		}
	});

	const handleError = $derived(shown && handle !== shown.agent?.messaging_name ? messagingNameError(snapshot, handle, shown.id) : undefined);
	const runtimeValue = $derived<AgentRuntime>(runtime === 'other' ? { other: otherRuntime.trim() } : (runtime as AgentRuntime));
	const missing = $derived(name.trim() === '' || role.trim() === '' || title.trim() === '' || (runtime === 'other' && otherRuntime.trim() === ''));

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		if (!shown?.agent || busy || handleError || missing) return;
		const a = shown.agent;
		const change: AgentChange = {};
		if (name.trim() !== shown.name) change.name = name;
		if (role.trim() !== a.role) change.role = role;
		if (title.trim() !== a.title) change.title = title;
		if (focus.trim() !== (a.focus ?? '')) change.focus = focus;
		if (runtime !== 'unknown' && JSON.stringify(runtimeValue) !== JSON.stringify(a.runtime)) change.runtime = runtimeValue;
		if (model.trim() !== (a.model ?? '')) change.model = model;
		if (home.trim() !== (a.home ?? '')) change.home = home.trim();
		if (handle !== a.messaging_name) change.messagingName = handle;
		if (Object.keys(change).length === 0) {
			open = false;
			return;
		}
		busy = true;
		try {
			await updateAgent(session, shown.id, change, shown.version);
			open = false;
			ondone({ name: change.name?.trim() ?? shown.name });
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
	<DialogContent class="max-h-[90vh] overflow-y-auto sm:max-w-lg">
		<form class="grid gap-4" onsubmit={submit} novalidate>
			<DialogHeader>
				<DialogTitle class="[overflow-wrap:anywhere]">Edit {shown?.name}</DialogTitle>
				<DialogDescription>Changes take effect at the agent's next session.</DialogDescription>
			</DialogHeader>
			<div class="space-y-2">
				<Label for="agent-name">Name</Label>
				<Input id="agent-name" bind:value={name} disabled={busy} autocomplete="off" />
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label for="agent-role">Role</Label>
					<Input id="agent-role" bind:value={role} disabled={busy} autocomplete="off" />
				</div>
				<div class="space-y-2">
					<Label for="agent-title">Title</Label>
					<Input id="agent-title" bind:value={title} disabled={busy} autocomplete="off" />
				</div>
			</div>
			<div class="space-y-2">
				<Label for="agent-focus">Focus <span class="font-normal text-gray-400">(clear it to remove)</span></Label>
				<Input id="agent-focus" bind:value={focus} disabled={busy} autocomplete="off" />
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label for="agent-runtime">Runs on</Label>
					<select id="agent-runtime" bind:value={runtime} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
						{#if runtime === 'unknown'}<option value="unknown">Runtime not set: choose one</option>{/if}
						{#each RUNTIMES as r (r)}<option value={r}>{runtimeLabel(r)}</option>{/each}
						<option value="other">Another runtime…</option>
					</select>
					{#if runtime === 'other'}<Input aria-label="Runtime name" bind:value={otherRuntime} disabled={busy} autocomplete="off" placeholder="Its name" />{/if}
				</div>
				<div class="space-y-2">
					<Label for="agent-model">Model <span class="font-normal text-gray-400">(optional)</span></Label>
					<Input id="agent-model" bind:value={model} disabled={busy} autocomplete="off" />
				</div>
			</div>
			<div class="space-y-2">
				<Label for="agent-home">Home folder <span class="font-normal text-gray-400">(optional, an absolute path on this Mac)</span></Label>
				<Input id="agent-home" bind:value={home} disabled={busy} autocomplete="off" class="font-mono" />
			</div>
			<div class="space-y-2">
				<Label for="agent-handle">Messaging name</Label>
				<Input id="agent-handle" bind:value={handle} disabled={busy} autocomplete="off" class="font-mono" aria-invalid={handleError ? true : undefined} aria-describedby={handleError ? 'agent-handle-error' : undefined} />
				{#if handleError}<p id="agent-handle-error" class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{handleError}</p>{/if}
			</div>
			{#if error}<p class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{error}</p>{/if}
			<DialogFooter>
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || !!handleError || missing}>Save</Button>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
