<!--
  @component
  Hire, Hiring and Done (our-org-in-the-app §7): the office's provisioning in words (Creating Marlowe's record…,
  Setting up the home folder…, Registering with messaging…), followed until it settles. Stopped partway: "Couldn't
  finish hiring Marlowe: <step that failed>. Already done: <steps done>." with Try again, which finishes without a new
  confirmation. Done: "Marlowe is hired and in App." with View profile primary; no View session until the office runs
  agent sessions (STAFF-7). Hired doesn't mean started.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import HeroIcon from '$lib/components/HeroIcon.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/modules/offices/office-api';
	import { employeesApi } from '../../api';
	import {
		HIRE_REQUESTS_URL,
		backoff,
		doneText,
		givenUpText,
		requestName,
		stepStates,
		stepWords
	} from '../../hire';
	import { pollWhile } from '../../poll.svelte';
	import type { HireRequest } from '../../types';

	let {
		organizationId,
		request,
		onchange,
		onsettled
	}: {
		organizationId: string;
		request: HireRequest;
		/** Each newer reading, so the flow knows the hire's state (the step list marks Done). */
		onchange?: (r: HireRequest) => void;
		/** The hire finished or stopped: the step heading changes, so the caller moves focus to it. */
		onsettled?: () => void;
	} = $props();

	let latest = $state.raw<HireRequest | undefined>();
	let pollError = $state<string | undefined>();
	let retrying = $state(false);
	let retryError = $state<string | undefined>();
	let failures = $state(0);
	let gone = false;
	$effect(() => () => (gone = true));

	const current = $derived(latest?.id === request.id ? latest : request);
	const name = $derived(requestName(current));
	const steps = $derived(stepStates(current));
	const moving = $derived(current.state === 'hiring' && !current.failedStep);
	const doing = $derived(steps.find((s) => s.state === 'doing'));
	// "Couldn't finish hiring Marlowe: <step that failed>. Already done: <steps done>." (§7).
	const failureText = $derived(
		current.failedStep
			? `Couldn't finish hiring ${name}: ${stepWords(current.failedStep, name).failed}.` +
					(current.doneSteps.length ? ` Already done: ${doneText(current, name)}.` : '')
			: ''
	);
	const givenUp = $derived(current.state === 'failed' || current.state === 'withdrawn');
	const heading = $derived(
		current.state === 'hired'
			? `${name} is hired and in ${current.homeTeam.name}.`
			: givenUp
				? `Hiring ${name} was given up`
				: current.failedStep
					? `Couldn't finish hiring ${name}`
					: `Hiring ${name}`
	);

	pollWhile(
		() => moving,
		async () => {
			try {
				const r = await employeesApi.hireRequest(organizationId, current.id);
				if (gone) return;
				pollError = undefined;
				failures = 0;
				const settled = r.state !== 'hiring' || !!r.failedStep;
				latest = r;
				onchange?.(r);
				if (settled) onsettled?.();
			} catch (e) {
				if (gone) return;
				pollError = toAppError(e).message;
				failures++;
			}
		},
		() => backoff(1000, failures)
	);

	async function tryAgain() {
		if (retrying) return;
		retrying = true;
		retryError = undefined;
		try {
			await employeesApi.retry(organizationId, current.id);
			latest = { ...current, failedStep: undefined };
			// The failure and its Try again are gone: focus the heading, which now says it is hiring.
			setTimeout(() => document.getElementById('hire-step-heading')?.focus(), 0);
		} catch (e) {
			retryError = toAppError(e).message;
		} finally {
			retrying = false;
		}
	}
</script>

<div class="max-w-2xl space-y-6">
	<h3
		id="hire-step-heading"
		tabindex="-1"
		class="text-base/7 font-semibold text-fg outline-none [overflow-wrap:anywhere]"
	>
		{heading}
	</h3>
	{#if current.state === 'hired'}
		<div class="space-y-2">
			<p class="text-sm/6 text-fg-secondary">
				Hired doesn't mean started: {name}'s profile shows whether it can run, and Activity records
				the hire.
			</p>
		</div>
		<div class="flex flex-wrap gap-3">
			{#if current.employeeId}
				<Button
					href={resolve(`/members/${encodeURIComponent(current.employeeId)}` as '/')}
					class={mobileButton}>View profile</Button
				>
			{/if}
			<Button href={resolve(HIRE_REQUESTS_URL as '/')} variant="outline" class={mobileButton}
				>Hire requests</Button
			>
		</div>
	{:else}
		<ol role="list" class="space-y-3" aria-label="Hiring {name}">
			{#each steps as s (s.step)}
				{@const words = stepWords(s.step, name)}
				<li class="flex items-start gap-x-3 text-sm/6">
					<span class="flex size-6 shrink-0 items-center justify-center" aria-hidden="true">
						{#if s.state === 'done'}
							<HeroIcon variant="micro" name="check-circle" class="size-4 text-success" />
						{:else if s.state === 'doing'}
							<span
								class="size-4 rounded-full border-2 border-info-text/30 border-t-info-text motion-safe:animate-spin"
							></span>
						{:else if s.state === 'failed'}
							<span class="flex size-4 items-center justify-center rounded-full bg-danger/20">
								<span class="size-1.5 rounded-full bg-danger-text"></span>
							</span>
						{:else}
							<span class="size-3 rounded-full outline-1 -outline-offset-1 outline-tint/30"></span>
						{/if}
					</span>
					<span
						class={s.state === 'waiting'
							? 'text-fg-muted'
							: s.state === 'failed'
								? 'text-danger-text'
								: s.state === 'doing'
									? 'text-fg'
									: 'text-fg-secondary'}
					>
						{s.state === 'done'
							? words.done
							: s.state === 'failed'
								? `${words.doing.replace('…', '')} · failed`
								: words.doing.replace('…', '')}
						<span class="sr-only"
							>{s.state === 'done'
								? ', done'
								: s.state === 'doing'
									? ', in progress'
									: s.state === 'waiting'
										? ', waiting'
										: ''}</span
						>
					</span>
				</li>
			{/each}
		</ol>
		<p role="status" class="sr-only">{doing ? stepWords(doing.step, name).doing : ''}</p>

		{#if givenUp}
			<div class="space-y-3">
				<p class="text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
					{failureText ||
						(current.doneSteps.length ? `Already done: ${doneText(current, name)}.` : '')}
					Given up: {givenUpText(current)}. Nothing more happens with it, and {name} isn't a member.
				</p>
				<Button href={resolve(HIRE_REQUESTS_URL as '/')} variant="outline" class={mobileButton}
					>Hire requests</Button
				>
			</div>
		{:else if current.failedStep}
			<div role="alert" class="space-y-1">
				<p class="text-sm/6 font-semibold text-danger-text [overflow-wrap:anywhere]">
					{failureText}
				</p>
				<p class="text-sm/6 text-fg-secondary">
					{name} isn't a member until the hire finishes. Try again picks up from the step that failed;
					it doesn't need a new confirmation.
				</p>
			</div>
			{#if retryError}
				<p role="alert" class="text-sm/6 text-danger-text [overflow-wrap:anywhere]">{retryError}</p>
			{/if}
			{#if current.state === 'hiring'}
				<Button class="min-w-24 {mobileButton}" aria-busy={retrying} onclick={tryAgain}
					>{retrying ? 'Trying again…' : 'Try again'}</Button
				>
			{/if}
		{:else}
			<p class="text-sm/6 text-fg-muted">
				The office is setting {name} up. You can leave this page: the hire carries on, and Hire requests
				shows how it went.
			</p>
		{/if}
		{#if pollError}
			<p class="text-xs/5 text-danger-text [overflow-wrap:anywhere]">
				Couldn't check on the hire: {pollError} Checking again.
			</p>
		{/if}
	{/if}
</div>
