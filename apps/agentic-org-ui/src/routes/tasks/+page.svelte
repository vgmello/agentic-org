<script lang="ts">
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Alert from '$lib/components/ui/alert';
	import type { SessionState } from '$lib/platform/auth/session-controller';
	import { createBrowserTasks, type BrowserTasks } from '$lib/features/task-assignment/browser';
	import type { TaskFeature } from '$lib/features/task-assignment/feature';
	import AssignmentSheet from '$lib/features/task-assignment/AssignmentSheet.svelte';
	import type { Card } from '$lib/features/task-assignment/wire';
	import type { Queue } from '$lib/features/task-assignment/gateway';
	let session = $state<SessionState>({ kind: 'signed-out' });
	let runtime = $state.raw<BrowserTasks>(),
		feature = $state.raw<TaskFeature>();
	let loading = $state(true),
		busy = $state(false),
		error = $state(''),
		login = $state(''),
		password = $state('');
	let org = $state(''),
		office = $state(''),
		queueKind = $state<Queue['kind']>('all'),
		target = $state('');
	let cards = $state<Card[]>([]),
		next = $state<string | null>(null),
		selected = $state<string>();
	const links = $derived(session.kind === 'signed-in' ? session.session.organizations : []);
	const offices = $derived(links.find((o) => o.org_id === org)?.office_ids ?? []);
	const canUse = $derived(session.kind === 'signed-in' && !session.reauth);
	let generation = 0,
		disposed = false;
	function changed(value: SessionState) {
		generation++;
		busy = false;
		session = value;
		feature = undefined;
		cards = [];
		selected = undefined;
		next = null;
		if (value.kind === 'signed-in' && !value.reauth) {
			org = value.session.organizations[0]?.org_id ?? '';
			office = value.session.organizations[0]?.office_ids[0] ?? '';
		}
	}
	async function signIn(event: SubmitEvent) {
		event.preventDefault();
		if (!runtime || busy) return;
		busy = true;
		error = '';
		const credentials = { login, password };
		password = '';
		try {
			await runtime.signIn(credentials);
		} catch {
			error =
				'Sign-in failed or the service is unavailable. Check your credentials and connection.';
		} finally {
			credentials.password = '';
			if (!disposed) busy = false;
		}
	}
	async function signOut() {
		if (!runtime || busy) return;
		busy = true;
		error = '';
		try {
			await runtime.signOut();
		} catch {
			error = 'Sign-out could not be confirmed. Your durable task operations remain recoverable.';
		} finally {
			if (!disposed) busy = false;
		}
	}
	function queue(): Queue {
		return queueKind === 'all' ? { kind: 'all' } : { kind: queueKind, targetId: target };
	}
	function invalidateQueue() {
		generation++;
		feature = undefined;
		cards = [];
		selected = undefined;
		next = null;
	}
	async function connect(event: SubmitEvent) {
		event.preventDefault();
		if (!runtime || !canUse || busy) return;
		const token = ++generation;
		busy = true;
		error = '';
		feature = undefined;
		cards = [];
		selected = undefined;
		next = null;
		try {
			const candidate = runtime.select(org, office);
			await candidate.reads.ready();
			const page = await candidate.reads.tasks(queue());
			if (disposed || token !== generation) return;
			feature = candidate;
			cards = page.items;
			next = page.next;
		} catch {
			if (!disposed && token === generation)
				error =
					'Selected tasks are unavailable for this office. Writer recovery, directory admission and owner authorization must be ready.';
		} finally {
			if (!disposed && token === generation) busy = false;
		}
	}
	async function reload(more = false) {
		if (!feature || busy || !canUse) return;
		const current = feature,
			token = generation;
		busy = true;
		error = '';
		try {
			const page = await current.reads.tasks(queue(), more ? (next ?? undefined) : undefined);
			if (!disposed && token === generation && feature === current) {
				cards = more ? [...cards, ...page.items] : page.items;
				next = page.next;
			}
		} catch {
			if (!disposed && token === generation)
				error = 'This queue could not be loaded. Retry after checking your session and office.';
		} finally {
			if (!disposed && token === generation) busy = false;
		}
	}
	onMount(() => {
		try {
			runtime = createBrowserTasks(changed);
			void runtime
				.boot()
				.catch(() => {
					if (!disposed)
						error =
							'The authenticated service or saved session could not be reached. Sign in to try again.';
				})
				.finally(() => {
					if (!disposed) loading = false;
				});
		} catch {
			error = 'This browser cannot establish a protected session.';
			loading = false;
		}
		return () => {
			disposed = true;
			generation++;
			runtime?.dispose();
		};
	});
</script>

<svelte:head
	><title>Tasks · Agentic Org</title><meta
		name="description"
		content="Assign work and inspect its durable history in your organization."
	/></svelte:head
>
<main class="mx-auto flex min-h-screen max-w-5xl flex-col gap-6 p-6 md:p-10">
	<header class="flex flex-wrap items-center justify-between gap-4">
		<div>
			<p class="text-muted-foreground text-sm">Agentic Org</p>
			<h1 class="text-2xl font-semibold">Tasks</h1>
		</div>
		{#if session.kind === 'signed-in'}<div class="flex items-center gap-3">
				<span>{session.session.account.login}</span><Button
					variant="outline"
					disabled={busy}
					onclick={signOut}>Sign out</Button
				>
			</div>{/if}
	</header>
	{#if loading}<p role="status">Restoring your session…</p>{/if}
	{#if error}<Alert.Root variant="destructive"
			><Alert.Title>Could not continue</Alert.Title><Alert.Description>{error}</Alert.Description
			></Alert.Root
		>{/if}
	{#if session.kind === 'update-required'}<Alert.Root
			><Alert.Title>Update required</Alert.Title><Alert.Description
				>This client and gateway do not support the same contract. Reload after updating the
				application.</Alert.Description
			></Alert.Root
		>
	{:else if !canUse}
		<section class="mx-auto flex w-full max-w-sm flex-col gap-4" aria-labelledby="sign-in-title">
			<h2 id="sign-in-title" class="text-lg font-medium">
				{session.kind === 'signed-in' ? 'Sign in again' : 'Sign in'}
			</h2>
			<p class="text-muted-foreground text-sm">
				Use the account registered with this Agentic Org instance. An employee profile is optional
				for an organization owner.
			</p>
			<form onsubmit={signIn} class="flex flex-col gap-4">
				<Label for="task-login">Login</Label><Input
					id="task-login"
					bind:value={login}
					autocomplete="username"
					required
					disabled={busy || loading}
				/><Label for="task-password">Password</Label><Input
					id="task-password"
					type="password"
					bind:value={password}
					autocomplete="current-password"
					required
					disabled={busy || loading}
				/><Button class="transition-none" type="submit" disabled={busy || loading}
					>{busy ? 'Signing in…' : 'Sign in'}</Button
				>
			</form>
		</section>
	{:else}
		<form onsubmit={connect} oninput={invalidateQueue} class="flex flex-col gap-4">
			<fieldset disabled={busy} class="grid gap-4 sm:grid-cols-2">
				<legend class="mb-2 font-medium">Select an office and queue</legend>
				<div class="flex flex-col gap-2">
					<Label for="task-org">Organization</Label><select
						id="task-org"
						bind:value={org}
						onchange={(event) => {
							office =
								links.find((o) => o.org_id === event.currentTarget.value)?.office_ids[0] ?? '';
						}}
						class="border-input bg-background rounded-md border p-2"
						>{#each links as link (link.org_id)}<option value={link.org_id}>{link.org_id}</option
							>{/each}</select
					>
				</div>
				<div class="flex flex-col gap-2">
					<Label for="task-office">Office</Label><select
						id="task-office"
						bind:value={office}
						class="border-input bg-background rounded-md border p-2"
						>{#each offices as id (id)}<option value={id}>{id}</option>{/each}</select
					>
				</div>
				<div class="flex flex-col gap-2">
					<Label for="task-queue">Queue</Label><select
						id="task-queue"
						bind:value={queueKind}
						class="border-input bg-background rounded-md border p-2"
						><option value="all">All tasks</option><option value="owned">Accountable team</option
						><option value="assigned_team">Assigned team</option><option value="assigned_employee"
							>Assigned employee</option
						></select
					>
				</div>
				{#if queueKind !== 'all'}<div class="flex flex-col gap-2">
						<Label for="task-target"
							>{queueKind === 'assigned_employee' ? 'Employee ID' : 'Team ID'}</Label
						><Input id="task-target" bind:value={target} required />
					</div>{/if}
			</fieldset>
			<Button type="submit" disabled={busy || !org || !office || (queueKind !== 'all' && !target)}
				>{busy ? 'Loading…' : 'Open queue'}</Button
			>
		</form>
		{#if links.length === 0}<p role="status">
				This account is not linked to an organization. Complete protected HQ setup or adoption with
				an instance administrator.
			</p>{/if}
		{#if feature}<section aria-labelledby="queue-title" class="flex flex-col gap-4">
				<div class="flex items-center justify-between">
					<h2 id="queue-title" class="text-lg font-medium">Task queue</h2>
					<Button variant="outline" disabled={busy} onclick={() => reload()}>Refresh</Button>
				</div>
				{#if !cards.length}<p role="status">No tasks in this queue.</p>{/if}
				<ul class="divide-border divide-y rounded-md border">
					{#each cards as task (task.task_id)}<li class="flex flex-col gap-2 p-4">
							<Button
								variant="link"
								class="justify-start px-0"
								onclick={() => (selected = task.task_id)}
								>{task.number ? `T-${task.number} · ` : ''}{task.title_preview}</Button
							>
							<p class="text-muted-foreground text-sm">
								Accountable: {task.accountable_team_id} · Assigned team: {task.assigned_team_id ??
									'none'} · Assignee: {task.assignee_id ?? 'none'}
							</p>
							<p class="text-muted-foreground text-xs">{task.status} · Revision {task.revision}</p>
						</li>{/each}
				</ul>
				{#if next}<Button variant="outline" disabled={busy} onclick={() => reload(true)}
						>More tasks</Button
					>{/if}
			</section>{/if}
	{/if}
	<div role="status" aria-live="polite" class="sr-only">
		{busy ? 'Waiting for the office.' : ''}
	</div>
</main>
{#if selected && feature}{#key JSON.stringify([feature.scope, selected])}<AssignmentSheet
			{feature}
			taskId={selected}
			closed={() => (selected = undefined)}
			changed={() => {
				void reload();
			}}
		/>{/key}{/if}
