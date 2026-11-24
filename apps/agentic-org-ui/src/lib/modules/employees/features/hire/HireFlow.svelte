<!--
  @component
  Hire an agent `/teams/hire` (our-org-in-the-app §7, §7.1; ADR-0029): Describe → Suggestion → Review → Hiring.
  Someone who may hire nowhere sees "You don't have access to hire. Ask your team lead." instead of the form. The
  HR lead drafts the proposal, or, when it can't run, stalls or stops, the hirer fills in the details. Nothing is
  created until the hirer confirms the Review. `?team=<id>` preselects the home team from the launching team;
  `?request=<id>` picks up a request again (a draft to confirm, or a hire to follow). Each step change moves focus to
  the step's heading.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { replaceState } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import Callout from '$lib/components/Callout.svelte';
	import EmptyState from '$lib/components/EmptyState.svelte';
	import ErrorState from '$lib/components/ErrorState.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { gutters, mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { toAppError } from '$lib/proto';
	import { employeesApi } from '../../api';
	import { HIRE_REQUESTS_URL, REFUSED, cleanProposal, hireUrlWith } from '../../hire';
	import type { HireAccess, HireRequest } from '../../types';
	import { HireFlowState } from './flow.svelte';
	import DescribeStep from './DescribeStep.svelte';
	import HiringProgress from './HiringProgress.svelte';
	import HrDrafting from './HrDrafting.svelte';
	import ProposalForm from './ProposalForm.svelte';
	import ReviewStep from './ReviewStep.svelte';
	import StepList from './StepList.svelte';

	let {
		organizationId,
		organizationName,
		teamId,
		requestId,
		accessResource,
		resumeResource
	}: {
		organizationId: string;
		organizationName: string;
		/** The launching team: preselected as the home team when the viewer may hire into it. */
		teamId?: string;
		/** A request to pick up again. Read once, when the flow opens. */
		requestId?: string;
		/** Optional pre-created access resource (primarily for tests). */
		accessResource?: Resource<HireAccess>;
		/** Optional pre-created resume resource (primarily for tests). */
		resumeResource?: Resource<HireRequest | undefined>;
	} = $props();

	const flow = new HireFlowState();
	let asking = $state(false);
	let askError = $state<string | undefined>();
	let askingAgain = $state(false);
	/** Started over: a request that couldn't be opened no longer stands in the way. */
	let fresh = $state(false);

	const ownedAccess = untrack(() => accessResource)
		? undefined
		: new Resource(() => employeesApi.hireAccess(organizationId));
	const access = $derived(accessResource ?? ownedAccess!);
	const choices = new Resource(() => employeesApi.hireChoices(organizationId));
	const ownedResume = untrack(() => resumeResource)
		? undefined
		: new Resource(async () => {
				const id = initialRequest();
				if (!id) return undefined;
				const r = await employeesApi.hireRequest(organizationId, id);
				chosenTeam = r.homeTeam.id;
				flow.open(r);
				return r;
			});
	const resume = $derived(resumeResource ?? ownedResume!);

	const teams = $derived(access.data?.teams ?? []);
	const launching = $derived(teams.find((t) => t.id === teamId));
	// The hirer's choice, or a resumed request's team; until then the launching team, else the top-level team
	// (§7 Describe). Kept apart from the default so a late answer about access never overwrites a choice.
	let chosenTeam = $state<string | undefined>();
	const homeTeamId = $derived(
		chosenTeam ?? launching?.id ?? teams.find((t) => !t.parentId)?.id ?? teams[0]?.id ?? ''
	);
	const step = $derived(flow.step);
	const done = $derived(step === 'hiring' && flow.request?.state === 'hired');
	const index = $derived(
		step === 'describe' ? 0 : step === 'review' ? 2 : step === 'hiring' ? (done ? 4 : 3) : 1
	);
	const heading = $derived(
		step === 'describe'
			? 'Describe the hire'
			: step === 'drafting'
				? flow.request?.drafting?.state === 'stopped'
					? 'The HR lead stopped'
					: 'The HR lead is drafting'
				: step === 'proposal'
					? flow.manual
						? 'Fill in the details'
						: "Check the HR lead's proposal"
					: step === 'review'
						? 'Review what will be created'
						: flow.request?.state === 'drafting'
							? 'Only its hirer confirms this draft'
							: 'This hire request is closed'
	);

	function initialRequest() {
		return requestId;
	}

	/** After the shell's own focus on navigation (a tick later), so the step heading wins. */
	function focusHeading() {
		setTimeout(() => document.getElementById('hire-step-heading')?.focus(), 0);
	}

	/** Keeps the request in the address, so a reload or Back picks it up again. */
	function remember(r: HireRequest) {
		replaceState(resolve(hireUrlWith(page.url.search, r.id) as '/'), page.state);
	}

	async function askHr() {
		if (asking) return;
		// Nothing changed since the HR lead was asked: go back to that draft rather than asking again.
		const same = flow.unchanged(homeTeamId);
		if (same) {
			flow.step = same;
			return focusHeading();
		}
		asking = true;
		askError = undefined;
		try {
			const r = await employeesApi.draft(organizationId, {
				reason: flow.reason.trim(),
				homeTeamId,
				...(flow.request?.state === 'drafting' ? { requestId: flow.request.id } : {})
			});
			remember(r);
			flow.asked(r);
			focusHeading();
		} catch (e) {
			askError = toAppError(e).message;
		} finally {
			asking = false;
		}
	}

	async function askAgain() {
		const request = flow.request;
		if (!request || askingAgain) return;
		askingAgain = true;
		try {
			const r = await employeesApi.draft(organizationId, {
				reason: request.reason,
				homeTeamId: request.homeTeam.id,
				requestId: request.id
			});
			flow.asked(r);
		} catch (e) {
			askError = toAppError(e).message;
			flow.back();
		} finally {
			askingAgain = false;
			focusHeading();
		}
	}

	/** Fill in the details yourself (STAFF-5). */
	function fillIn() {
		flow.fillIn(flow.request?.state === 'drafting' ? flow.request.homeTeam.id : homeTeamId);
		focusHeading();
	}

	function hrAnswered(r: HireRequest) {
		if (flow.hrAnswered(r)) focusHeading();
	}

	/** A newer reading of the HR lead's turn; when it stops, the heading says so and takes focus. */
	function hrUpdated(r: HireRequest) {
		const stopped = flow.request?.drafting?.state !== 'stopped' && r.drafting?.state === 'stopped';
		flow.updated(r);
		if (stopped) focusHeading();
	}

	function back() {
		flow.back();
		focusHeading();
	}

	async function confirm() {
		const r = await employeesApi.confirm(organizationId, {
			requestId: flow.request?.state === 'drafting' ? flow.request.id : undefined,
			reason: flow.reason.trim(),
			proposal: cleanProposal(flow.fields)
		});
		remember(r);
		flow.confirmed(r);
		focusHeading();
	}

	function startOver() {
		fresh = true;
		chosenTeam = undefined;
		flow.reset();
		replaceState(resolve(hireUrlWith(page.url.search, undefined) as '/'), page.state);
		focusHeading();
	}

	const backLink = $derived(
		launching
			? { href: `/teams/${encodeURIComponent(launching.id)}`, label: launching.name }
			: { href: '/teams', label: 'Teams' }
	);
</script>

<PageHeader
	title="Hire an agent"
	back={backLink}
	description={step === 'describe' && teams.length > 0
		? 'The HR lead drafts a proposal from what you describe, or you fill it in. You see exactly what will be created, and nothing is created until you confirm it.'
		: undefined}
/>

<Loaded
	resource={access}
	label="who you may hire for"
	error="Couldn't open Hire"
	place="Hire an agent"
	area="Teams"
	errorLevel={3}
>
	{#snippet children(a)}
		{#if a.teams.length === 0}
			<EmptyState
				level={3}
				title={REFUSED}
				description="Only the organization's owner, and team leads who have Allow hiring, hire into a team."
			>
				{#snippet action()}
					<Button href={resolve(HIRE_REQUESTS_URL as '/')} variant="outline" class={mobileButton}
						>Hire requests</Button
					>
				{/snippet}
			</EmptyState>
		{:else if resume.state === 'loading' && !fresh}
			<SkeletonRows label="the hire request" rows={3} />
		{:else if resume.state === 'unsupported' && !fresh}
			<div class="px-4 py-12 min-[640px]:px-6 min-[1024px]:px-8" data-testid="resource-unsupported">
				<h3 class="text-sm/6 font-semibold text-fg">Not available in this version</h3>
				<p class="mt-1 max-w-xl text-sm/6 text-fg-muted [overflow-wrap:anywhere]">
					{resume.message || "This version of the office doesn't support resuming hire requests."}
				</p>
			</div>
			<div class={gutters}>
				<Button variant="outline" class={mobileButton} onclick={startOver}>Start a new hire</Button>
			</div>
		{:else if (resume.state === 'error' || resume.state === 'unavailable') && !fresh}
			<ErrorState
				level={3}
				title="Couldn't open this hire request"
				message={resume.message}
				onretry={() => resume.reload()}
				retrying={resume.reloading}
			/>
			<div class={gutters}>
				<Button variant="outline" class={mobileButton} onclick={startOver}>Start a new hire</Button>
			</div>
		{:else}
			{#if step !== 'closed'}<StepList current={index} />{/if}
			<section aria-labelledby="hire-step-heading" class="py-6">
				{#if step !== 'hiring'}
					<h3
						id="hire-step-heading"
						tabindex="-1"
						class="mb-6 text-base/7 font-semibold text-fg outline-none {gutters}"
					>
						{heading}
					</h3>
				{/if}
				{#if step === 'describe'}
					<div class={gutters}>
						<DescribeStep
							teams={a.teams}
							hr={a.hr}
							bind:reason={flow.reason}
							bind:homeTeamId={() => homeTeamId, (v) => (chosenTeam = v)}
							busy={asking}
							error={askError}
							onnext={askHr}
							onmanual={fillIn}
						/>
					</div>
				{:else if step === 'drafting' && flow.request}
					<div class={gutters}>
						{#key flow.request.drafting?.startedAt}
							<HrDrafting
								{organizationId}
								request={flow.request}
								retrying={askingAgain}
								onready={hrAnswered}
								onupdate={hrUpdated}
								onmanual={fillIn}
								ontryagain={askAgain}
								onback={back}
							/>
						{/key}
					</div>
				{:else if step === 'proposal'}
					<Loaded
						resource={choices}
						label="the choices for this hire"
						error="Couldn't load the choices for this hire"
						errorLevel={4}
					>
						{#snippet children(c)}
							<div class={gutters}>
								<ProposalForm
									bind:fields={flow.fields}
									choices={c}
									teams={a.teams}
									manual={flow.manual}
									reason={flow.reason}
									request={flow.request}
									onback={back}
									onnext={() => {
										flow.step = 'review';
										focusHeading();
									}}
								/>
							</div>
						{/snippet}
					</Loaded>
				{:else if step === 'review' && choices.data}
					<ReviewStep
						{organizationId}
						{organizationName}
						proposal={cleanProposal(flow.fields)}
						choices={choices.data}
						teams={a.teams}
						onback={back}
						onconfirm={confirm}
					/>
				{:else if step === 'hiring' && flow.request}
					<div class={gutters}>
						<HiringProgress
							{organizationId}
							request={flow.request}
							onchange={(r) => (flow.request = r)}
							onsettled={focusHeading}
						/>
					</div>
				{:else if flow.request}
					{@const request = flow.request}
					<div class="max-w-2xl space-y-4 {gutters}">
						<Callout>
							<p class="text-sm/6 [overflow-wrap:anywhere]">
								{#if request.state === 'drafting'}
									{request.requestedBy.name} started this hire request and is still drafting it. Only
									its hirer confirms or withdraws it; nothing has been created.
								{:else}
									{request.closedReason ??
										(request.state === 'withdrawn'
											? 'It was withdrawn.'
											: 'It was given up before it finished.')}
									Nothing more happens with it.
								{/if}
							</p>
						</Callout>
						<Button class={mobileButton} onclick={startOver}>Start a new hire</Button>
					</div>
				{/if}
			</section>
		{/if}
	{/snippet}
</Loaded>
