<script lang="ts">
	import { untrack } from 'svelte';
	import { SvelteSet } from 'svelte/reactivity';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { addAgent } from '$lib/org/actions';
	import { messagingNameError, runtimeLabel, suggestMessagingName } from '$lib/org/agents';
	import { byName, owner } from '$lib/org/model';
	import type { OrgSession } from '$lib/org/session';
	import { teamOptions } from '$lib/org/team-edit';
	import type { AgentRuntime, AppError, OrgSnapshot } from '$lib/proto';

	/** "Add an existing agent" (our-org-in-the-app §7): an agent that already runs elsewhere, as one row. */
	let {
		open = $bindable(false),
		snapshot,
		fullSnapshot,
		session,
		officeId,
		officeName,
		organization,
		ondone
	}: {
		open: boolean;
		/** What the chart shows: teams and managers to choose from. */
		snapshot: OrgSnapshot;
		/** Everyone, including agents who left: their names stay reserved. */
		fullSnapshot: OrgSnapshot;
		session: OrgSession;
		officeId: string;
		officeName: string;
		organization: string;
		ondone: (result: { memberId: string; name: string; problems: string[] }) => void;
	} = $props();

	const RUNTIMES = ['claude_code', 'codex', 'antigravity'] as const;
	let name = $state('');
	let handle = $state('');
	let handleEdited = $state(false);
	let runtime = $state<string>('claude_code');
	let otherRuntime = $state('');
	let role = $state('');
	let title = $state('');
	let home = $state('');
	let runMode = $state<string>('on_call');
	let manager = $state('');
	const teamIds = new SvelteSet<string>();
	let failure = $state<AppError | undefined>();
	let busy = $state(false);
	let touched = $state(false);

	$effect(() => {
		if (open) {
			untrack(() => {
				name = '';
				handle = '';
				handleEdited = false;
				runtime = 'claude_code';
				otherRuntime = '';
				role = '';
				title = '';
				home = '';
				runMode = 'on_call';
				manager = '';
				teamIds.clear();
				failure = undefined;
				busy = false;
				touched = false;
			});
		}
	});

	// The messaging name follows the name until the owner edits it.
	$effect(() => {
		const suggestion = suggestMessagingName(name);
		untrack(() => {
			if (!handleEdited) handle = suggestion;
		});
	});

	const handleError = $derived(handle === '' ? (touched ? 'Enter a messaging name.' : undefined) : messagingNameError(fullSnapshot, handle));
	const nameError = $derived(touched && name.trim() === '' ? 'Enter a name.' : undefined);
	const missing = $derived(name.trim() === '' || handle === '' || role.trim() === '' || (runtime === 'other' && otherRuntime.trim() === ''));
	const teams = $derived(teamOptions(snapshot));
	const managers = $derived(snapshot.members.filter((m) => !m.owner).toSorted(byName));
	const ownerName = $derived(owner(snapshot)?.name ?? 'you');
	const teamName = (id: string) => snapshot.teams.find((t) => t.id === id)?.name ?? id;

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		touched = true;
		if (busy || handleError || missing) return;
		busy = true;
		failure = undefined;
		try {
			const result = await addAgent(
				session,
				{
					name: name.trim(),
					messagingName: handle,
					runtime: (runtime === 'other' ? { other: otherRuntime.trim() } : runtime) as AgentRuntime,
					role: role.trim(),
					title: title.trim() || role.trim(),
					officeId,
					home: home.trim() || undefined,
					// An agent someone starts by hand has no run mode: the office never starts it.
					runMode: runMode === '' ? undefined : (runMode as 'always_on' | 'on_call'),
					teamIds: [...teamIds],
					managerId: manager || undefined
				},
				teamName
			);
			if (result.created.error) {
				failure = result.created.error;
				return;
			}
			open = false;
			ondone({ memberId: result.memberId, name: name.trim(), problems: result.steps.filter((s) => s.error).map((s) => `Couldn't ${s.what.charAt(0).toLowerCase()}${s.what.slice(1)}: ${s.error!.message}`) });
		} catch (error) {
			failure = toAppError(error);
		} finally {
			busy = false;
		}
	}
</script>

<Sheet bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<SheetContent side="right" class="w-full max-w-md overflow-y-auto">
		<SheetHeader>
			<SheetTitle>Add an agent</SheetTitle>
			<SheetDescription>For an agent that already runs on this Mac. It will be reachable by everyone in {organization}.</SheetDescription>
		</SheetHeader>

		<form class="flex flex-1 flex-col gap-y-5 px-4" onsubmit={submit} novalidate>
			<div class="space-y-2">
				<Label for="new-agent-name">Name</Label>
				<Input id="new-agent-name" bind:value={name} disabled={busy} autocomplete="off" aria-invalid={nameError ? true : undefined} />
				{#if nameError}<p class="text-sm/6 text-red-400" role="alert">{nameError}</p>{/if}
			</div>
			<div class="space-y-2">
				<Label for="new-agent-handle">Messaging name</Label>
				<Input id="new-agent-handle" bind:value={handle} oninput={() => (handleEdited = true)} disabled={busy} autocomplete="off" class="font-mono" aria-invalid={handleError ? true : undefined} aria-describedby={handleError ? 'new-agent-handle-error' : 'new-agent-handle-help'} />
				{#if handleError}
					<p id="new-agent-handle-error" class="text-sm/6 text-red-400 [overflow-wrap:anywhere]" role="alert">{handleError}</p>
				{:else}
					<p id="new-agent-handle-help" class="text-xs/5 text-gray-400">What others write to reach it (--to {handle || 'name'}). Unique, and kept for good.</p>
				{/if}
			</div>
			<div class="space-y-2">
				<Label for="new-agent-runtime">Runs on</Label>
				<select id="new-agent-runtime" bind:value={runtime} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					{#each RUNTIMES as r (r)}<option value={r}>{runtimeLabel(r)}</option>{/each}
					<option value="other">Another runtime…</option>
				</select>
				{#if runtime === 'other'}<Input aria-label="Runtime name" bind:value={otherRuntime} disabled={busy} autocomplete="off" placeholder="Its name" />{/if}
			</div>
			<div class="grid gap-4 sm:grid-cols-2">
				<div class="space-y-2">
					<Label for="new-agent-role">Role</Label>
					<Input id="new-agent-role" bind:value={role} disabled={busy} autocomplete="off" placeholder="software engineer" />
				</div>
				<div class="space-y-2">
					<Label for="new-agent-title">Title <span class="font-normal text-gray-400">(optional)</span></Label>
					<Input id="new-agent-title" bind:value={title} disabled={busy} autocomplete="off" />
				</div>
			</div>
			<fieldset class="space-y-2" disabled={busy}>
				<legend class="text-sm/6 font-medium text-white">Teams <span class="font-normal text-gray-400">(optional)</span></legend>
				<ul class="max-h-36 divide-y divide-white/5 overflow-y-auto rounded-lg bg-white/5 outline-1 -outline-offset-1 outline-white/10" aria-label="Teams">
					{#each teams as option (option.team.id)}
						<li>
							<label class="flex cursor-pointer items-center gap-x-3 py-1.5 pr-3 text-sm/6" style="padding-left: {0.75 + option.depth * 1.25}rem">
								<Checkbox checked={teamIds.has(option.team.id)} onCheckedChange={(on) => (on === true ? teamIds.add(option.team.id) : teamIds.delete(option.team.id))} aria-label={option.path} />
								<span class="min-w-0 text-white [overflow-wrap:anywhere]">{option.team.name}</span>
							</label>
						</li>
					{:else}
						<li class="px-3 py-2 text-sm/6 text-gray-400">There are no teams yet.</li>
					{/each}
				</ul>
			</fieldset>
			<div class="space-y-2">
				<Label for="new-agent-run">How it runs</Label>
				<select id="new-agent-run" bind:value={runMode} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					<option value="on_call">On call: wakes when it's needed</option>
					<option value="always_on">Always on: keeps a session running</option>
					<option value="">Started by a person: the office never starts it</option>
				</select>
			</div>
			<div class="space-y-2">
				<Label for="new-agent-manager">Reports to</Label>
				<select id="new-agent-manager" bind:value={manager} disabled={busy} class="w-full rounded-md bg-gray-800 px-2 py-2 text-sm/6 text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:outline-indigo-400">
					<option value="">{ownerName} (you)</option>
					{#each managers as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
				</select>
			</div>
			<div class="space-y-2">
				<Label for="new-agent-home">Home folder <span class="font-normal text-gray-400">(optional, an absolute path on {officeName})</span></Label>
				<Input id="new-agent-home" bind:value={home} disabled={busy} autocomplete="off" class="font-mono" placeholder="/Users/you/agents/name" />
			</div>

			{#if failure}
				<Alert variant="destructive">
					<CircleAlert aria-hidden="true" />
					<AlertTitle class="[overflow-wrap:anywhere]">Couldn't add the agent</AlertTitle>
					<AlertDescription class="[overflow-wrap:anywhere]">{failure.message}</AlertDescription>
				</Alert>
			{/if}

			<SheetFooter class="mt-auto px-0">
				<Button type="button" variant="outline" disabled={busy} onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || !!handleError || missing}>Add agent</Button>
			</SheetFooter>
		</form>
	</SheetContent>
</Sheet>
