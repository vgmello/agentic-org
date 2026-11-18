<script lang="ts">
	import { goto } from '$app/navigation';
	import { onDestroy, onMount, tick } from 'svelte';
	import CircleAlert from '@lucide/svelte/icons/circle-alert';
	import Info from '@lucide/svelte/icons/info';
	import LoaderCircle from '@lucide/svelte/icons/loader-circle';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { appSession } from '$lib/platform/auth/app-session.svelte';
	import { signInHref } from '$lib/platform/auth/return-target';
	import { createOperationClock } from '$lib/platform/http/clock';
	import { createOrganizationsApi } from '$lib/platform/organizations/api';
	import { recoveryReleaseCommand } from '$lib/platform/organizations/command';
	import {
		createOrganizationFlow,
		failureDetails,
		isWorking,
		NOT_ADMINISTRATOR,
		type Phase
	} from '$lib/platform/organizations/create-flow';
	import type { CreateField, InstanceName } from '$lib/platform/organizations/readers';
	import { selection } from '$lib/platform/organizations/selection';
	import { createSlugField } from '$lib/platform/organizations/slug';
	import { ORGANIZATION_NAME_MAX_CHARS } from '$lib/proto';

	// Offices-and-teams UX §5a (OFC-15 to OFC-24). The flow owns the attempt; this page shows it.
	let name = $state('');
	let slug = $state('');
	let repository = $state('');
	let stateBranch = $state('');
	let phase = $state<Phase>({ kind: 'editing', errors: {} });
	let instance = $state<InstanceName>();
	let copied = $state<'details' | 'command'>();
	let alert = $state<HTMLElement | null>(null);
	let heading = $state<HTMLElement | null>(null);

	const session = $derived(
		appSession.state.kind === 'signed-in' ? appSession.state.session : undefined
	);
	const slugField = createSlugField();
	const clock = createOperationClock();
	const sleep = (ms: number, signal?: AbortSignal) =>
		new Promise<void>((resolve, reject) => {
			const timer = setTimeout(resolve, ms);
			signal?.addEventListener(
				'abort',
				() => {
					clearTimeout(timer);
					reject(new Error('Stopped'));
				},
				{ once: true }
			);
		});
	const api = createOrganizationsApi({
		http: {
			read: (path, signal) => appSession.controller().read(path, signal),
			submit: (path, body, signal) => appSession.controller().submit(path, body, signal)
		},
		instanceId: () => {
			if (!session) throw new Error('Sign in required');
			return session.instance_id;
		},
		newId: () => clock.newId()
	});
	const flow = createOrganizationFlow({ api, sleep, report: (next) => void show(next) });

	const working = $derived(isWorking(phase));
	const errors = $derived(phase.kind === 'editing' ? phase.errors : {});
	const failure = $derived(phase.kind === 'failed' ? phase : undefined);
	// Until a lost attempt's outcome is known, nothing new may be sent (OFC-21).
	const locked = $derived(working || failure?.action === 'check_again');
	const created = $derived(phase.kind === 'created' ? phase.organization : undefined);
	const command = $derived(created ? recoveryReleaseCommand(created.name) : '');
	const progress = $derived(
		phase.kind === 'creating'
			? 'Creating…'
			: phase.kind === 'checking'
				? phase.still
					? `Still creating ${phase.name}…`
					: `Checking whether ${phase.name} was created…`
				: ''
	);

	async function show(next: Phase) {
		phase = next;
		copied = undefined;
		await tick();
		if (next.kind === 'failed') alert?.focus();
		else if (next.kind === 'created') {
			// The choice is kept even if this page is closed before Open is pressed.
			if (session)
				selection.select(
					{ instanceId: session.instance_id, accountId: session.account.id },
					next.organization.id
				);
			heading?.focus();
		} else if (next.kind === 'editing') {
			const first = (['name', 'slug', 'organization_repository', 'state_branch'] as const).find(
				(field) => next.errors[field]
			);
			if (first) document.getElementById(first)?.focus();
		}
	}

	onMount(async () => {
		await appSession.boot();
		const state = appSession.state;
		if (state.kind !== 'signed-in') {
			await goto(signInHref('/create', location.origin), { replaceState: true });
			return;
		}
		clock.synchronize(state.session.server_time_ms);
		instance = await api.instance();
		// The instance's own clock reading is the freshest one for operation IDs.
		if (instance) clock.synchronize(instance.server_time_ms);
	});
	onDestroy(() => flow.stop());

	// Signing out here or in another tab leaves this page, as on the organizations page.
	$effect(() => {
		const state = appSession.state;
		if (state.kind !== 'signed-out') return;
		if (state.reason === 'signed-out')
			void goto('/sign-in?note=signed-out', { replaceState: true });
		else if (state.reason === 'changed-account') void goto('/sign-in', { replaceState: true });
		else if (state.reason === 'ended')
			void goto(signInHref('/create', location.origin, 'ended'), { replaceState: true });
	});

	function submit(event: SubmitEvent) {
		event.preventDefault();
		if (locked) return;
		void flow.submit({ name, slug, repository, stateBranch });
	}
	function nameTyped() {
		slug = slugField.nameChanged(name);
		flow.edited('name');
		flow.edited('slug');
	}
	function slugTyped() {
		slug = slugField.slugEdited(slug, name);
		flow.edited('slug');
	}
	async function copy(what: 'details' | 'command') {
		const text =
			what === 'command'
				? command
				: failure
					? failureDetails(failure, instance?.server_version)
					: '';
		try {
			await navigator.clipboard.writeText(text);
			copied = what;
		} catch {
			copied = undefined;
		}
	}
	const describedBy = (field: CreateField, ...always: string[]) =>
		[...always, errors[field] ? `${field}-error` : ''].filter(Boolean).join(' ') || undefined;
	const secondary =
		'rounded-md bg-white/10 px-3 py-2 text-sm font-semibold text-white hover:bg-white/20 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500';
</script>

<svelte:head><title>Create organization · Agentic Org</title></svelte:head>

{#snippet fieldError(field: CreateField)}
	{#if errors[field]}
		<p id="{field}-error" class="text-sm/6 text-red-400" role="alert">{errors[field]}</p>
	{/if}
{/snippet}

{#if !session}
	<p class="text-sm/6 text-gray-400" aria-live="polite">Loading…</p>
{:else if !session.admin}
	<h1 class="text-2xl/9 font-semibold tracking-tight text-white">Create organization</h1>
	<p class="mt-4 text-base/7 text-gray-300" role="alert">{NOT_ADMINISTRATOR}</p>
	<div class="mt-6"><a href="/welcome" class={secondary}>Back to organizations</a></div>
{:else if created}
	<h1
		bind:this={heading}
		tabindex="-1"
		class="text-2xl/9 font-semibold tracking-tight text-white focus:outline-none"
	>
		Created {created.name}. You own it.
	</h1>
	<p class="mt-2 text-base/7 text-gray-400">
		{created.name} is starting. This usually takes a few seconds.
	</p>
	<section
		aria-labelledby="recovery-title"
		class="mt-8 rounded-lg bg-white/5 p-4 outline-1 -outline-offset-1 outline-white/10"
	>
		<h2 id="recovery-title" class="text-sm/6 font-semibold text-white">
			Recovery code: waiting to be collected
		</h2>
		<p class="mt-1 text-sm/6 text-gray-300">
			The recovery code is never shown in the browser. Collect it once on the HQ machine{instance
				? ` (HQ instance ${instance.name})`
				: ''}, in a terminal:
		</p>
		<pre class="mt-3 overflow-x-auto rounded-md bg-gray-900 p-3 text-sm/6 text-gray-100"><code
				data-testid="recovery-command">{command}</code
			></pre>
		<div class="mt-3 flex items-center gap-x-3">
			<button type="button" class={secondary} onclick={() => copy('command')}>Copy command</button>
			<span class="text-sm/6 text-gray-300" aria-live="polite"
				>{copied === 'command' ? 'Copied.' : ''}</span
			>
		</div>
	</section>
	<div class="mt-8">
		<Button onclick={() => goto('/')}>Open {created.name}</Button>
	</div>
{:else}
	<h1 class="text-3xl/9 font-semibold tracking-tight text-white">Create organization</h1>

	<form class="mt-8 space-y-6" onsubmit={submit} novalidate>
		<div class="space-y-2">
			<Label for="name">Name</Label>
			<Input
				id="name"
				name="name"
				autocomplete="off"
				placeholder="Side Project"
				maxlength={ORGANIZATION_NAME_MAX_CHARS * 2}
				readonly={locked}
				bind:value={name}
				oninput={nameTyped}
				aria-invalid={errors.name ? true : undefined}
				aria-describedby={describedBy('name', 'create-note')}
			/>
			{@render fieldError('name')}
		</div>
		<div class="space-y-2">
			<Label for="slug">Slug</Label>
			<Input
				id="slug"
				name="slug"
				autocomplete="off"
				autocapitalize="none"
				spellcheck={false}
				placeholder="side-project"
				readonly={locked}
				bind:value={slug}
				oninput={slugTyped}
				aria-invalid={errors.slug ? true : undefined}
				aria-describedby={describedBy('slug', 'slug-rule')}
			/>
			<p id="slug-rule" class="text-sm/6 text-gray-400">
				Lowercase letters, numbers and hyphens. It names the organization in addresses and is not
				changed by a rename.
			</p>
			{@render fieldError('slug')}
		</div>
		<div class="space-y-2">
			<Label for="organization_repository">Org Git URL</Label>
			<Input
				id="organization_repository"
				name="organization_repository"
				type="url"
				autocomplete="off"
				autocapitalize="none"
				spellcheck={false}
				placeholder="https://example.com/org/state.git"
				readonly={locked}
				bind:value={repository}
				oninput={() => flow.edited('organization_repository')}
				aria-invalid={errors.organization_repository ? true : undefined}
				aria-describedby={describedBy('organization_repository', 'repository-rule')}
			/>
			<p id="repository-rule" class="text-sm/6 text-gray-400">
				The remote repository that holds the organization's files (https:// or ssh://). Nothing is
				fetched or pushed yet.
			</p>
			{@render fieldError('organization_repository')}
		</div>
		<div class="space-y-2">
			<Label for="state_branch">State branch</Label>
			<Input
				id="state_branch"
				name="state_branch"
				autocomplete="off"
				autocapitalize="none"
				spellcheck={false}
				placeholder="main"
				readonly={locked}
				bind:value={stateBranch}
				oninput={() => flow.edited('state_branch')}
				aria-invalid={errors.state_branch ? true : undefined}
				aria-describedby={describedBy('state_branch')}
			/>
			{@render fieldError('state_branch')}
		</div>

		<div
			id="create-note"
			class="flex gap-x-3 border-l-2 border-indigo-400 bg-white/5 px-4 py-3 text-sm/6 text-gray-200"
		>
			<Info class="mt-0.5 size-4 shrink-0 text-indigo-300" aria-hidden="true" />
			<p>
				It will be stored on {instance ? `HQ instance ${instance.name}` : 'this HQ instance'}, and
				you'll own it. Its recovery code is then collected on that machine.
			</p>
		</div>

		<p class="flex min-h-6 items-center gap-x-2 text-sm/6 text-gray-200" aria-live="polite">
			{#if working}
				<LoaderCircle class="size-4 animate-spin text-indigo-300" aria-hidden="true" />
			{/if}
			{progress}
		</p>

		{#if failure}
			<div
				bind:this={alert}
				role="alert"
				tabindex="-1"
				class="rounded-md bg-red-500/10 p-4 outline-1 -outline-offset-1 outline-red-400/40 focus-visible:outline-2 focus-visible:outline-red-300"
			>
				<p class="flex gap-x-2 text-sm/6 text-red-100">
					<CircleAlert class="mt-0.5 size-4 shrink-0 text-red-300" aria-hidden="true" />
					<span>{failure.message}</span>
				</p>
				<div class="mt-3 flex flex-wrap items-center gap-3">
					{#if failure.action === 'try_again'}
						<button type="submit" class={secondary}>Try again</button>
					{:else if failure.action === 'check_again'}
						<button type="button" class={secondary} onclick={() => flow.checkAgain()}
							>Check again</button
						>
					{:else if failure.action === 'open_list'}
						<a href="/welcome" class={secondary}>Open organization list</a>
					{/if}
					<button type="button" class={secondary} onclick={() => copy('details')}
						>Copy details</button
					>
					<span class="text-sm/6 text-red-100" aria-live="polite"
						>{copied === 'details' ? 'Copied.' : ''}</span
					>
				</div>
			</div>
		{/if}

		<div class="flex flex-wrap items-center gap-x-3 gap-y-2">
			<Button type="submit" disabled={locked}>
				{working ? 'Creating…' : 'Create organization'}
			</Button>
			<a href="/welcome" class={secondary}>{working ? 'Close' : 'Cancel'}</a>
			{#if working}
				<span class="text-sm/6 text-gray-300">Closing this panel doesn't stop it.</span>
			{/if}
		</div>
	</form>
{/if}
