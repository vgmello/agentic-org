<script lang="ts">
	import { beforeNavigate, goto, invalidate } from '$app/navigation';
	import ArrowLeft from '@lucide/svelte/icons/arrow-left';
	import Check from '@lucide/svelte/icons/check';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import Laptop from '@lucide/svelte/icons/laptop';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { Alert, AlertDescription, AlertTitle } from '$lib/components/ui/alert';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { createFlow, describeFailure, isBusy, type Phase } from '$lib/create-flow';
	import { officeApi } from '$lib/office/api';
	import { ORGANIZATION_NAME_MAX_CHARS } from '$lib/proto';

	let name = $state('');
	let phase = $state<Phase>({ kind: 'editing' });
	const flow = createFlow(officeApi, (next) => (phase = next));
	// Set just before the flow's own navigation, so only that one is let through while busy.
	let leaving = false;

	// Leaving mid-flow would strand the in-flight call: it would finish and pull the owner back.
	beforeNavigate((nav) => {
		if (busy && !leaving) nav.cancel();
	});

	const busy = $derived(isBusy(phase));
	const nameError = $derived(phase.kind === 'editing' ? phase.nameError : undefined);
	const failure = $derived(phase.kind === 'failed' ? describeFailure(phase.error) : undefined);

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		await flow.submit(name);
		await afterRun();
	}

	async function afterRun() {
		if (phase.kind === 'done') {
			// Let the shell's layout reload the remembered connection, then go in.
			leaving = true;
			try {
				await invalidate('app:connections');
				await goto('/agents', { replaceState: true });
			} catch {
				// Navigation failed after the organization was created: reload into the shell.
				window.location.assign('/agents');
			}
		}
	}

	type StepState = 'todo' | 'active' | 'done' | 'failed';
	const steps = $derived.by((): { label: string; state: StepState }[] => {
		const order = ['starting', 'creating', 'done'];
		const at = phase.kind === 'failed' ? order.indexOf(phase.during) : order.indexOf(phase.kind);
		const state = (i: number): StepState =>
			phase.kind === 'failed' ? (i < at ? 'done' : i === at ? 'failed' : 'todo') : i < at || phase.kind === 'done' ? 'done' : i === at ? 'active' : 'todo';
		return [
			{ label: 'Start the office on This Mac', state: state(0) },
			{ label: `Create ${name.trim() || 'the organization'}`, state: state(1) }
		];
	});
</script>

<svelte:head><title>Create organization · Agentic Org</title></svelte:head>

<a
	href="/welcome"
	aria-disabled={busy ? 'true' : undefined}
	tabindex={busy ? -1 : undefined}
	class="-ml-1 mb-6 inline-flex w-fit items-center gap-x-1.5 rounded-md text-sm/6 font-semibold text-gray-400 hover:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500 {busy ? 'pointer-events-none opacity-50' : ''}"
>
	<ArrowLeft class="size-4" aria-hidden="true" /> Back
</a>

<h1 class="text-3xl/9 font-semibold tracking-tight text-white">Create organization</h1>
<p class="mt-2 text-base/7 text-gray-400">Name it. You can change the rest later.</p>

<form class="mt-8 space-y-6" onsubmit={submit} novalidate>
	<div class="space-y-2">
		<Label for="name">Organization name</Label>
		<Input
			id="name"
			name="name"
			autocomplete="off"
			autofocus
			placeholder="Acme Studio"
			maxlength={ORGANIZATION_NAME_MAX_CHARS * 2}
			disabled={busy}
			bind:value={name}
			oninput={() => phase.kind === 'editing' && phase.nameError && (phase = { kind: 'editing' })}
			aria-invalid={nameError ? true : undefined}
			aria-describedby={nameError ? 'name-error' : undefined}
		/>
		{#if nameError}
			<p id="name-error" class="text-sm/6 text-red-400" role="alert">{nameError}</p>
		{/if}
	</div>

	<div class="space-y-2">
		<span class="text-sm/6 font-medium text-white">Where its office runs</span>
		<div class="flex items-center gap-x-3 rounded-lg bg-white/5 px-4 py-3 outline-1 -outline-offset-1 outline-white/10">
			<Laptop class="size-5 text-gray-400" aria-hidden="true" />
			<div class="text-sm/6">
				<p class="font-semibold text-white">This Mac</p>
				<p class="text-gray-400">Its first office, HQ, runs on this Mac. It starts with your Mac and keeps running when Agentic Org is closed.</p>
			</div>
		</div>
	</div>

	<div aria-live="polite">
		{#if busy || phase.kind === 'failed'}
		<ol class="space-y-2">
			{#each steps as step (step.label)}
				<li class="flex items-center gap-x-3 text-sm/6 {step.state === 'todo' ? 'text-gray-500' : 'text-white'}">
					{#if step.state === 'done'}
						<Check class="size-4 text-green-400" aria-hidden="true" />
					{:else if step.state === 'failed'}
						<CircleAlert class="size-4 text-red-400" aria-hidden="true" />
					{:else if step.state === 'active'}
						<LoaderCircle class="size-4 animate-spin text-indigo-400" aria-hidden="true" />
					{:else}
						<span class="size-4 rounded-full border border-white/20" aria-hidden="true"></span>
					{/if}
					{step.label}
				</li>
			{/each}
		</ol>
		{/if}
	</div>

	{#if failure}
		<Alert variant="destructive">
			<CircleAlert aria-hidden="true" />
			<AlertTitle>{failure.title}</AlertTitle>
			<AlertDescription>
				<p>{failure.hint}</p>
				{#if failure.action === 'open_login_items'}
					<Button type="button" variant="outline" size="sm" class="mt-3" onclick={() => officeApi.openLoginSettings()}>Open Login Items</Button>
				{:else if failure.action === 'turn_on'}
					<Button type="button" variant="outline" size="sm" class="mt-3" disabled={busy} onclick={async () => { await officeApi.setLoginItem(true); await flow.submit(name); await afterRun(); }}>Turn on</Button>
				{/if}
			</AlertDescription>
		</Alert>
	{/if}

	<div class="flex items-center gap-x-3">
		<Button type="submit" disabled={busy}>
			{#if busy}<LoaderCircle class="animate-spin" aria-hidden="true" />{/if}
			{phase.kind === 'failed' ? 'Try again' : 'Create organization'}
		</Button>
	</div>
</form>
