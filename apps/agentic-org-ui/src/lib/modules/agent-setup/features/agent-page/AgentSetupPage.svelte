<!--
  @component
  One agent's setup, `/settings/agents/:memberId` (our-org-in-the-app §5, AC-IA07; employee-supervision-ux §1;
  agent-run-modes-ux §1, §2): back to Agent setup; the agent's name and who it is in one line; View profile and More
  (Pause or Resume, Offboard). Then, in the order a person fixes things: Health with every reason and its fix; How it
  runs (`execution.md`, its own revision; Edit is the page's one primary action); Where it runs (office, placement
  generation and freshness, observed runtime, the read-only managed home); the employee record (`profile.md`, its own
  revision; Change role). States: loading, unavailable, error with Retry, not found, read-only viewer, refusals
  inline, busy labels, a conflict that shows the newer version and keeps the person's choices.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { afterNavigate, goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
	import CopyButton from '$lib/components/CopyButton.svelte';
	import DescriptionList, { type DescriptionItem } from '$lib/components/DescriptionList.svelte';
	import Loaded from '$lib/components/Loaded.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Section from '$lib/components/Section.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { tone } from '$lib/components/pills';
	import { mobileButton } from '$lib/components/classes';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		DropdownMenu,
		DropdownMenuContent,
		DropdownMenuItem,
		DropdownMenuSeparator,
		DropdownMenuTrigger
	} from '$lib/components/ui/dropdown-menu';
	import { pageTitle } from '$lib/nav';
	import { cn } from '$lib/utils';
	import { toAppError } from '$lib/office/api';
	import { offboardAgent, setAgentStatus } from '$lib/org/actions';
	import { offboardSummary } from '$lib/org/agents';
	import { orgApi } from '$lib/org/api';
	import { sessionFor } from '$lib/org/session';
	import type { OrgSnapshot } from '$lib/proto';
	import { agentSetupApi } from '../../api';
	import HealthBadge from '../../components/HealthBadge.svelte';
	import { health, healthText } from '../../health';
	import { focusHeading, focusHeadingAfterShell } from '$lib/components/focus';
	import {
		CONNECTION_STATE,
		CONNECTION_TONE,
		ageText,
		codeParts,
		isStale,
		offboardLines,
		remainingHolds,
		runtimeText,
		toolsText
	} from '../../present';
	import type { AgentSetupDetail } from '../../types';
	import { AGENT_SETUP_URL, profileHref } from '../../urls';
	import ChangeRoleDialog from '../change-role/ChangeRoleDialog.svelte';
	import EditExecutionDialog from '../edit-execution/EditExecutionDialog.svelte';
	import { lookUpAgent } from './lookup';

	let {
		organizationId,
		memberId,
		snapshot
	}: {
		organizationId: string;
		memberId: string;
		/** The organization's structure: the member's version for Pause and Offboard, and Offboard's preview. */
		snapshot: OrgSnapshot;
	} = $props();

	const resource = new Resource(() =>
		lookUpAgent(() => agentSetupApi.agent(organizationId, memberId))
	);
	const detail = $derived(resource.data?.found ? resource.data.detail : undefined);
	const h = $derived(detail ? health(detail) : undefined);
	const canChange = $derived(!!detail?.viewer.canChange);
	const record = $derived(snapshot.members.find((m) => m.id === memberId));
	const session = $derived(sessionFor(orgApi, organizationId));
	const title = $derived(
		resource.state !== 'ready'
			? pageTitle('Agent', 'Agent setup', 'Organization settings')
			: detail
				? pageTitle(detail.name, 'Agent setup', 'Organization settings')
				: pageTitle('Agent not found', 'Agent setup', 'Organization settings')
	);
	const now = $derived(resource.data ? Date.now() : 0);

	// Arriving by a link or Back, the agent's heading takes focus once it has loaded, after the shell's own focus
	// on the top bar's heading. A full page load leaves focus where the browser puts it.
	let focusOnLoad = false;
	afterNavigate((move) => {
		focusOnLoad = move.type !== 'enter';
	});
	$effect(() => {
		if (resource.state !== 'ready' || !focusOnLoad) return;
		focusOnLoad = false;
		focusHeadingAfterShell('agent-heading');
	});

	type Dialog = 'execution' | 'role' | 'pause' | 'offboard';
	let dialog = $state<Dialog | undefined>();
	let opening = $state(0);
	let opener: HTMLElement | null = null;
	let moreButton = $state<HTMLButtonElement | null>(null);
	/** What a lifecycle confirmation acts on, fixed when it opens: the version it showed and Offboard's preview. */
	let shown = $state.raw<{ version?: number; lines: string[] }>({ lines: [] });
	let reloadOnClose = false;
	let leaving = false;

	function open(which: Dialog, from?: HTMLElement | null) {
		opener = from ?? moreButton;
		if (which === 'pause' || which === 'offboard') freeze();
		opening++;
		dialog = which;
	}

	function freeze() {
		shown = {
			version: record?.version,
			lines:
				record && detail ? offboardLines(offboardSummary(snapshot, record, false), detail.name) : []
		};
	}

	const openFlag = (which: Dialog) => ({
		get value() {
			return dialog === which;
		},
		set value(next: boolean) {
			dialog = next ? which : dialog === which ? undefined : dialog;
		}
	});
	const executionOpen = openFlag('execution');
	const roleOpen = openFlag('role');
	const pauseOpen = openFlag('pause');
	const offboardOpen = openFlag('offboard');

	/** Focus goes back to what opened the dialog, or what replaced it (Resume for Pause…), or the heading. */
	function refocus() {
		const target =
			(opener?.isConnected && opener) ||
			(moreButton?.isConnected && moreButton) ||
			document.getElementById('agent-heading');
		if (target?.id === 'agent-heading') focusHeading('agent-heading');
		else target?.focus();
	}

	async function restore(event: Event) {
		event.preventDefault();
		if (leaving) return;
		if (reloadOnClose) {
			reloadOnClose = false;
			await changed();
			await tick();
		}
		refocus();
	}

	async function changed() {
		await invalidate('app:org');
		await resource.reload();
	}

	/** A save or a conflict answered with the setup as it now is: show it without another read. */
	function show(latest: AgentSetupDetail) {
		resource.data = { found: true, detail: latest };
	}

	function executionSaved(saved: AgentSetupDetail) {
		show(saved);
		const e = saved.execution;
		toast.success(
			e
				? `Saved: ${saved.name} runs ${e.modelId} on ${e.profile.name} from its next turn.`
				: `Saved ${saved.name}'s setup.`
		);
	}

	function roleSaved(saved: AgentSetupDetail) {
		show(saved);
		toast.success(
			saved.role ? `${saved.name}'s role is ${saved.role}.` : `${saved.name} has no role now.`
		);
	}

	/** HQ refused a stale version: show the latest, so a retry acts on what the person now sees (§9). */
	async function refreshOnConflict(e: unknown, name: string): Promise<never> {
		if (toAppError(e).code === 'conflict') {
			await changed();
			freeze();
			throw new Error(
				`${name} changed at HQ while you were looking. Here's the latest; try again.`,
				{
					cause: e
				}
			);
		}
		throw e;
	}

	async function pause() {
		const d = detail;
		if (!d) return;
		await setAgentStatus(session, d.memberId, 'paused', shown.version).catch((e) =>
			refreshOnConflict(e, d.name)
		);
		toast.success(`Pause requested for ${d.name}. Stop pending until the office confirms it.`);
		reloadOnClose = true;
	}

	async function resume(event: Event) {
		const d = detail;
		if (!d) return;
		opener = (event.currentTarget as HTMLElement | null) ?? moreButton;
		try {
			await setAgentStatus(session, d.memberId, 'active', record?.version);
			const other = remainingHolds(d.holds);
			if (other.length)
				toast.warning(`Resumed ${d.name}, but it still can't start.`, {
					description: other.join(' · ')
				});
			else toast.success(`Resumed ${d.name}.`);
		} catch (e) {
			toast.error(`Couldn't resume ${d.name}`, { description: toAppError(e).message });
		}
		await changed();
		await tick();
		refocus();
	}

	async function offboard() {
		const d = detail;
		if (!d) return;
		await offboardAgent(session, d.memberId, shown.version).catch((e) =>
			refreshOnConflict(e, d.name)
		);
		toast.success(`Offboarded ${d.name}. The record and handle stay.`);
		leaving = true;
		await invalidate('app:org');
		await goto(resolve(AGENT_SETUP_URL));
		focusHeadingAfterShell('agent-setup-heading');
	}

	const linkClass =
		'rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
	/** A value that is a link: a 44 px target below 640 px (app-shell-ux §6a), without growing the row on desktop. */
	const valueLink = `${linkClass} inline-flex items-center max-[639px]:min-h-11`;
	const backClass =
		'inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11';
</script>

<svelte:head><title>{title}</title></svelte:head>

{#if resource.state !== 'ready'}
	<!-- The one named back link stays while the agent itself can't be shown. -->
	<div class="px-4 pt-6 min-[640px]:px-6 min-[1024px]:px-8">
		<a href={resolve(AGENT_SETUP_URL)} class={backClass}>Agent setup</a>
	</div>
{/if}

<Loaded
	{resource}
	label="the agent's setup"
	error="Couldn't load this agent's setup"
	place="Agent setup"
	area="Organization settings"
	rows={6}
>
	{#snippet children(lookup)}
		{#if !lookup.found}
			<PageHeader
				id="agent-heading"
				title="Agent not found"
				back={{ href: AGENT_SETUP_URL, label: 'Agent setup' }}
				description="{lookup.message} It may have been offboarded, or the link may be incomplete."
			/>
		{:else}
			{@const d = lookup.detail}
			{@const hh = h!}
			<PageHeader
				id="agent-heading"
				title={d.name}
				back={{ href: AGENT_SETUP_URL, label: 'Agent setup' }}
			>
				{#snippet description()}
					<!-- Below 640 px each fact takes its own line, so no separator is left at a line's start. -->
					<p
						class="flex flex-col gap-y-1 [overflow-wrap:anywhere] min-[640px]:flex-row min-[640px]:flex-wrap min-[640px]:items-baseline min-[640px]:gap-x-2"
					>
						<span>Agent{d.builtIn ? ', built in' : ''}</span>
						{#if d.role ?? d.roleText}<span aria-hidden="true" class="hidden min-[640px]:inline"
								>·</span
							><span>{d.role ?? d.roleText}</span>{/if}
						<span aria-hidden="true" class="hidden min-[640px]:inline">·</span><span
							class="font-mono">@{d.handle}</span
						>
					</p>
				{/snippet}
				{#snippet actions()}
					<Button
						href={resolve(profileHref(d.memberId) as '/')}
						variant="outline"
						class={mobileButton}>View profile</Button
					>
					{#if canChange && record}
						<DropdownMenu>
							<DropdownMenuTrigger>
								{#snippet child({ props })}
									<Button {...props} bind:ref={moreButton} variant="outline" class={mobileButton}
										>More<span class="sr-only"> actions for {d.name}</span></Button
									>
								{/snippet}
							</DropdownMenuTrigger>
							<DropdownMenuContent align="end">
								{#if d.status === 'paused'}
									<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={(e) => resume(e)}
										>Resume</DropdownMenuItem
									>
								{:else}
									<DropdownMenuItem class="max-[639px]:min-h-11" onSelect={() => open('pause')}
										>Pause…</DropdownMenuItem
									>
								{/if}
								{#if !d.builtIn}
									<DropdownMenuSeparator />
									<DropdownMenuItem
										variant="destructive"
										class="max-[639px]:min-h-11"
										onSelect={() => open('offboard')}>Offboard…</DropdownMenuItem
									>
								{/if}
							</DropdownMenuContent>
						</DropdownMenu>
					{/if}
				{/snippet}
			</PageHeader>

			<Section
				level={3}
				title="Health"
				description="From its configuration and the office's last check. Health isn't presence, and it doesn't allow a run."
			>
				<div class="px-4 text-sm/6 min-[640px]:px-6 min-[1024px]:px-8" data-testid="health">
					<p class="flex flex-wrap items-center gap-x-3 gap-y-2">
						<HealthBadge health={hh} aria-hidden="true" />
						<span class="sr-only">{healthText(hh)}</span>
						{#if d.readiness.known}
							<span class="text-xs/5 text-fg-muted"
								>Office checked {ageText(d.readiness.checkedAt, now)}</span
							>
						{/if}
					</p>
					{#if hh.kind === 'needs_setup'}
						<ul role="list" class="mt-3 flex max-w-3xl flex-col gap-3">
							{#each hh.reasons as reason (reason.code)}
								<li class="[overflow-wrap:anywhere]">
									<p class="font-medium text-fg">{reason.text}</p>
									{#if reason.fix}<p class="text-fg-muted">
											{#each codeParts(reason.fix) as part, i (i)}{#if part.code}<code
														class="rounded-sm bg-surface-sunken px-1.5 py-0.5 font-mono text-fg-body ring-1 ring-line-subtle ring-inset"
														>{part.text}</code
													>{:else}{part.text}{/if}{/each}
										</p>{/if}
								</li>
							{/each}
						</ul>
					{:else if hh.kind === 'paused'}
						<p class="mt-3 max-w-3xl text-fg-secondary">
							Paused: {d.name} starts nothing until it's resumed. It keeps its home team and its assignments.
						</p>
					{:else if hh.kind === 'unknown'}
						<p class="mt-3 max-w-3xl text-fg-secondary [overflow-wrap:anywhere]">{hh.why}</p>
						<p class="max-w-3xl text-fg-muted">
							Nothing here is known to stop it; it isn't shown as ready until the office says so.
						</p>
					{:else}
						<p class="mt-3 max-w-3xl text-fg-secondary">
							{d.name} can start when it's called. The office checks again before each turn.
						</p>
					{/if}
					{#if d.stopPending}
						<p class="mt-3 max-w-3xl text-fg-secondary" data-testid="stop-pending">
							Stop pending: HQ accepted the pause; the office hasn't confirmed the process stopped
							yet.
						</p>
					{/if}
					{#if d.holds.length}
						<p class="mt-3 text-fg-muted">Holds: {d.holds.join(' · ')}</p>
					{/if}
				</div>
			</Section>

			<Section
				level={3}
				title="How it runs"
				description={d.execution
					? `Execution configuration, rev ${d.execution.revision}${d.execution.updated?.byName ? ` · updated by ${d.execution.updated.byName}${d.execution.updated.at ? ` ${ageText(d.execution.updated.at, now)}` : ''}` : ''}. Changes take effect from its next turn.`
					: 'An agent without an execution configuration is not runnable. Nothing is chosen for it.'}
				class="border-t border-line-subtle"
			>
				{#snippet actions()}
					{#if canChange}
						<Button
							class="min-w-20 {mobileButton}"
							onclick={(e: MouseEvent) => open('execution', e.currentTarget as HTMLElement)}
							>{d.execution ? 'Edit' : 'Set up execution'}</Button
						>
					{/if}
				{/snippet}
				{#if d.execution}
					{@const e = d.execution}
					<DescriptionList
						class="mt-4"
						items={[
							{ term: 'Model profile', value: e.profile.name },
							{ term: 'Model', value: e.modelId, mono: true },
							{ term: 'Runtime', value: runtimeText(e) },
							{ term: 'Connections', content: connections },
							{
								term: 'Execution',
								value: e.enabled ? 'On' : 'Turned off in its configuration'
							},
							{
								term: 'Tools',
								value: `${toolsText(e.tools)}${e.tools.length ? `: ${e.tools.join(', ')}` : ''}`
							},
							{
								term: 'System prompt',
								value: e.hasPrompt ? 'Set in its configuration' : 'None'
							}
						] satisfies DescriptionItem[]}
					/>
				{:else}
					<DescriptionList
						class="mt-4"
						items={[{ term: 'Execution', value: 'Execution not configured' }]}
					/>
				{/if}
			</Section>

			{#snippet connections()}
				{#if d.execution && d.execution.connections.length}
					<ol role="list" class="flex flex-col gap-2">
						{#each d.execution.connections as c, i (c.id)}
							<li class="[overflow-wrap:anywhere]">
								<span class="text-fg-secondary">{i + 1}. {c.name}</span>
								<span class="block text-xs/5"
									>{c.runtime} ·
									<StatusPill
										label={CONNECTION_STATE[c.state]}
										class={cn(
											tone[CONNECTION_TONE[c.state]],
											'[&>span]:overflow-visible [&>span]:whitespace-normal'
										)}
									/>{#if c.checkedAt}, checked {ageText(c.checkedAt, now)}{/if}</span
								>
							</li>
						{/each}
					</ol>
				{:else}None{/if}
			{/snippet}

			<Section
				level={3}
				title="Where it runs"
				description="What the owning office observes. The managed home is resolved by the office and can't be changed here."
				class="border-t border-line-subtle"
			>
				<DescriptionList
					class="mt-4"
					items={[
						{ term: 'Office', content: office },
						{ term: 'Observed runtime', value: d.observedRuntime ?? 'Not reported by the office' },
						d.managedHome
							? { term: 'Managed home', value: d.managedHome, mono: true, action: copyHome }
							: { term: 'Managed home', value: 'Not reported by the office' }
					] satisfies DescriptionItem[]}
				/>
			</Section>

			{#snippet office()}
				{#if d.placement}
					<span class="text-fg-secondary"
						>{d.placement.office.name} · placement generation {d.placement.generation}</span
					>
					<span class="block text-xs/5"
						>{d.placement.observedAt
							? `Reported ${ageText(d.placement.observedAt, now)}`
							: 'Never reported'}{#if isStale(d.placement.observedAt, now)}. This may be out of
							date.{/if}</span
					>
				{:else}
					<span class="font-medium text-fg-secondary">Unplaced</span>
					<span class="block"
						>No office runs {d.name}. Choose one in
						<a href={resolve('/settings/offices')} class={linkClass}>Offices</a>, under Place or
						offboard.</span
					>
				{/if}
			{/snippet}
			{#snippet copyHome()}
				<CopyButton text={d.managedHome ?? ''} label="Copy path" size="sm" />
			{/snippet}

			<Section
				level={3}
				title="Employee record"
				description="Who {d.name} is, rev {d.employeeRevision}: separate from how it runs. Name, title and focus are edited on its profile."
				class="border-t border-line-subtle"
			>
				{#snippet actions()}
					{#if canChange}
						<Button
							variant="outline"
							class={mobileButton}
							onclick={(e: MouseEvent) => open('role', e.currentTarget as HTMLElement)}
							>Change role…</Button
						>
					{/if}
				{/snippet}
				<DescriptionList
					class="mt-4"
					items={[
						{ term: 'Role', content: role },
						{ term: 'Home team', content: homeTeam },
						{ term: 'Reports to', content: reportsTo }
					]}
				/>
			</Section>

			{#snippet role()}
				{#if d.role}<span class="text-fg-secondary">{d.role}</span>{:else}No role on record{/if}
				{#if !d.role && d.roleText}<span class="block text-xs/5">Role as written: {d.roleText}</span
					>{/if}
			{/snippet}
			{#snippet homeTeam()}
				{#if d.homeTeam}
					<a href={resolve(`/teams/${encodeURIComponent(d.homeTeam.id)}` as '/')} class={valueLink}
						>{d.homeTeam.name}</a
					>
				{:else}
					<span class="text-danger-text">Home team missing: this record is invalid.</span>
				{/if}
			{/snippet}
			{#snippet reportsTo()}
				{#if d.reportsTo}
					<a href={resolve(profileHref(d.reportsTo.id) as '/')} class={valueLink}
						>{d.reportsTo.name}</a
					>
				{:else}No explicit manager{/if}
			{/snippet}

			{#key opening}
				<EditExecutionDialog
					bind:open={executionOpen.value}
					{organizationId}
					agent={d}
					onlatest={show}
					onsaved={executionSaved}
					onCloseAutoFocus={restore}
				/>
				<ChangeRoleDialog
					bind:open={roleOpen.value}
					{organizationId}
					agent={d}
					onlatest={show}
					onsaved={roleSaved}
					onCloseAutoFocus={restore}
				/>
			{/key}
			<ConfirmDialog
				bind:open={pauseOpen.value}
				title="Pause {d.name}?"
				description="HQ will record the request. {d.name} stays in {d.homeTeam?.name ??
					'its home team'}; new admissions stop when the authoritative control is applied."
				confirmLabel="Pause"
				busyLabel="Pausing…"
				onconfirm={pause}
				onCloseAutoFocus={restore}
			/>
			<ConfirmDialog
				bind:open={offboardOpen.value}
				title="Offboard {d.name}?"
				description="{d.name} becomes a former member. This is what changes:"
				confirmLabel="Offboard"
				busyLabel="Offboarding…"
				destructive
				onconfirm={offboard}
				onCloseAutoFocus={restore}
			>
				{#snippet body()}
					<ul role="list" class="flex list-disc flex-col gap-1 pl-5">
						{#each shown.lines as line, i (i)}<li>{line}</li>{/each}
					</ul>
				{/snippet}
			</ConfirmDialog>
		{/if}
	{/snippet}
</Loaded>
