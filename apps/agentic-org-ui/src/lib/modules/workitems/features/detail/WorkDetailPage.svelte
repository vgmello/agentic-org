<!--
  @component
  Full page for any Task, Bug or Request at `/work/[ref]` (work-items-ux §7, §9; feature-requests-ux §3;
  costs-and-insights-ux §2, §4). Uses Resource + Loaded (explicitly handling loading, unavailable, unsupported,
  error, not_found, stale, and ready).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import Loaded from '$lib/components/Loaded.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { pageTitle } from '$lib/nav';
	import { useShellData } from '$lib/platform/shell/shell-data.svelte';
	import { toAppError } from '$lib/proto';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import {
		errorClass,
		gutters,
		inputClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { workItemsApi } from '../../api';
	import {
		STATUS_WORD,
		TYPE_WORD,
		WORK_URL,
		activityKindLabel,
		inboxItemHref,
		isOpen,
		normalizeRef,
		titleError,
		when
	} from '../../present';
	import type { Choices, CloseAs, TaskDetail, TaskStatus, WhyLink } from '../../types';
	import WorkIcon from '../../components/WorkIcon.svelte';
	import StatusBadge from '../../components/StatusBadge.svelte';
	import BudgetChip from '../../components/BudgetChip.svelte';
	import StaleBanner from '../../components/StaleBanner.svelte';
	import CaptureDialog from '../capture/CaptureDialog.svelte';
	import ChecklistSection from './ChecklistSection.svelte';
	import DiscussionSection from './DiscussionSection.svelte';
	import BugSection from './BugSection.svelte';
	import RequestSection from './RequestSection.svelte';
	import WorkSideColumn from './WorkSideColumn.svelte';
	import WorkDetailDialogs from './WorkDetailDialogs.svelte';

	let {
		organizationId,
		refParam,
		embedded = false,
		onchanged
	}: {
		organizationId: string;
		refParam: string;
		embedded?: boolean;
		onchanged?: () => void;
	} = $props();

	let shellData: ReturnType<typeof useShellData> | undefined;
	try {
		shellData = useShellData();
	} catch {
		shellData = undefined;
	}

	async function reloadDetail() {
		await taskRead.reload();
		void shellData?.refresh();
		onchanged?.();
	}

	const uid = $props.id();

	type LookupResult = { found: true; task: TaskDetail } | { found: false; message: string };

	async function fetchTask(): Promise<LookupResult> {
		try {
			const task = await workItemsApi.task(organizationId, normalizeRef(refParam));
			return { found: true, task };
		} catch (e) {
			const err = toAppError(e);
			if (err.source === 'api' && err.code === 'not_found') {
				return { found: false, message: err.message };
			}
			throw e;
		}
	}

	const taskRead = new Resource<LookupResult>(() => fetchTask());
	const choicesRead = new Resource<Choices>(() => workItemsApi.choices(organizationId));
	const choices = $derived(choicesRead.data ?? null);
	let whyOptions = $state.raw<WhyLink[]>([]);
	let whyPickerOpen = $state(false);
	let selectedWhyId = $state('');
	let linkingWhy = $state(false);

	// Edit title/description state
	let editing = $state(false);
	let editTitle = $state('');
	let editDesc = $state('');
	let editBusy = $state(false);
	let editError = $state<string | undefined>();

	// Action modals
	let statusBusy = $state(false);
	let closeOpen = $state(false);
	let closeKind = $state<CloseAs['as']>('done');
	let reopenOpen = $state(false);
	let blockOpen = $state(false);
	let createFixOpen = $state(false);

	const task = $derived(taskRead.data?.found ? taskRead.data.task : undefined);

	const documentTitle = $derived.by(() => {
		if (taskRead.state !== 'ready') return pageTitle('Work');
		if (!task) return pageTitle('Task not found', 'Work');
		return pageTitle(`${task.ref} ${task.title}`, 'Work');
	});

	function startEdit(t: TaskDetail) {
		editTitle = t.title;
		editDesc = t.description;
		editError = undefined;
		editing = true;
	}

	async function saveEdit(event: SubmitEvent, t: TaskDetail) {
		event.preventDefault();
		if (editBusy || t.stale) return;
		const err = titleError(editTitle);
		if (err) {
			editError = err;
			return;
		}
		editBusy = true;
		editError = undefined;
		try {
			await workItemsApi.edit(organizationId, t.id, {
				title: editTitle.trim(),
				description: editDesc.trim(),
				expectedRevision: t.revision
			});
			editing = false;
			toast.success(`Updated ${t.ref}.`);
			await reloadDetail();
		} catch (e) {
			editError = toAppError(e).message;
		} finally {
			editBusy = false;
		}
	}

	async function changeStatus(t: TaskDetail, selectEl: HTMLSelectElement) {
		const next = selectEl.value as TaskStatus;
		if (statusBusy || t.stale || next === t.status) return;
		if (next === 'blocked') {
			selectEl.value = t.status;
			blockOpen = true;
			return;
		}
		if (next === 'done' || next === 'cancelled') {
			selectEl.value = t.status;
			closeKind = next;
			closeOpen = true;
			return;
		}
		statusBusy = true;
		try {
			await workItemsApi.setStatus(organizationId, t.id, {
				status: next,
				expectedRevision: t.revision
			});
			toast.success(`Moved ${t.ref} to ${STATUS_WORD[next]}.`);
			await reloadDetail();
		} catch (e) {
			selectEl.value = t.status;
			toast.error(toAppError(e).message);
		} finally {
			statusBusy = false;
		}
	}

	async function openWhyPicker(t: TaskDetail) {
		whyPickerOpen = true;
		try {
			whyOptions = await workItemsApi.whyChoices(organizationId, t.id);
		} catch (e) {
			toast.error(toAppError(e).message);
			whyPickerOpen = false;
		}
	}

	async function submitLinkWhy(t: TaskDetail) {
		if (!selectedWhyId || linkingWhy) return;
		linkingWhy = true;
		try {
			await workItemsApi.linkWhy(organizationId, t.id, selectedWhyId);
			selectedWhyId = '';
			whyPickerOpen = false;
			toast.success('Linked document.');
			await reloadDetail();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			linkingWhy = false;
		}
	}
</script>

<svelte:head>
	{#if !embedded}
		<title>{documentTitle}</title>
	{/if}
</svelte:head>

{#if !embedded}
	<div class="{gutters} pt-6">
		<a
			href={resolve(WORK_URL)}
			class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
		>
			Work
		</a>
	</div>
{/if}

<Loaded
	resource={taskRead}
	label="the work item"
	error="Couldn't load this work item"
	place="Work item"
	area="Work"
>
	{#snippet children(lookup)}
		{#if !lookup.found}
			<header class="border-b border-line-subtle {gutters} pt-2 pb-6">
				<h2
					id="work-detail-heading"
					class="text-base/7 font-semibold text-fg [overflow-wrap:anywhere]"
				>
					Task not found
				</h2>
				<p class="mt-1 max-w-2xl text-sm/6 text-fg-muted">{lookup.message}</p>
			</header>
		{:else}
			{@const t = lookup.task}
			{@const isStale = !!t.stale}

			{#if t.stale}
				<StaleBanner stale={t.stale} retrying={taskRead.reloading} onretry={() => reloadDetail()} />
			{/if}

			<!-- Header -->
			<header
				class="border-b border-line-subtle {gutters} pt-2 pb-6"
				data-testid="work-detail-header"
			>
				<div class="flex flex-wrap items-start justify-between gap-4">
					<div class="flex min-w-0 flex-1 flex-col gap-2">
						<div class="flex flex-wrap items-center gap-2 text-xs/5 text-fg-muted">
							<span class="inline-flex items-center gap-1 font-medium text-fg-secondary">
								<WorkIcon name={t.type} class="size-4 text-fg-muted" />
								<span>{TYPE_WORD[t.type]}</span>
								<span class="font-mono">{t.ref}</span>
							</span>
							<StatusBadge status={t.status} needsMe={t.needsMe} />
							{#if t.budget}
								<BudgetChip budget={t.budget} hold={t.budgetHold} />
							{/if}
							{#if t.reopenCount > 0}
								<span
									class="inline-flex items-center gap-1 text-fg-muted tabular-nums"
									data-testid="reopen-count"
								>
									<WorkIcon name="rotate" class="size-3.5" />
									<span>Reopened {t.reopenCount}</span>
								</span>
							{/if}
						</div>

						<h2
							id={embedded ? 'work-detail-title' : 'work-detail-heading'}
							tabindex="-1"
							class="text-lg/7 font-semibold text-fg [overflow-wrap:anywhere] focus:outline-none"
						>
							{t.title}
						</h2>

						{#if t.needsMe}
							<p class="text-sm/6 text-warning-text-strong" data-testid="detail-needs-me">
								{t.needsMe.text}
								{#if t.needsMe.inboxItemId}
									<a
										href={resolve(inboxItemHref(t.needsMe.inboxItemId) as '/')}
										class="ml-2 font-medium text-link hover:underline"
									>
										Open Inbox item
									</a>
								{/if}
							</p>
						{/if}

						{#if t.status === 'blocked' && t.blockedReason}
							<p class="text-sm/6 text-danger-text" data-testid="detail-blocked-reason">
								Blocked: {t.blockedReason}
							</p>
						{/if}
					</div>

					{#if !isStale && t.can.edit}
						<div class="flex flex-wrap items-center gap-2">
							{#if isOpen(t.status)}
								<label class="sr-only" for="{uid}-status-select">Status</label>
								<select
									id="{uid}-status-select"
									value={t.status}
									disabled={statusBusy}
									onchange={(e) => changeStatus(t, e.currentTarget as HTMLSelectElement)}
									class="w-auto {selectClass}"
									data-testid="detail-status-select"
								>
									<option value="todo">To do</option>
									<option value="in_progress">In progress</option>
									<option value="blocked">Blocked…</option>
									<option value="done">Done…</option>
									<option value="cancelled">Cancelled…</option>
								</select>

								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={() => {
										closeKind = 'done';
										closeOpen = true;
									}}>Close as…</Button
								>
							{:else}
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={() => (reopenOpen = true)}>Reopen…</Button
								>
							{/if}

							{#if !editing}
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={() => startEdit(t)}>Edit</Button
								>
							{/if}
						</div>
					{/if}
				</div>
			</header>

			<!-- Two-column body: main column + side column -->
			<div class="grid gap-6 {gutters} py-6 lg:grid-cols-[minmax(0,1fr)_22rem]">
				<div class="flex min-w-0 flex-col gap-6">
					<!-- Edit title & description inline form -->
					{#if editing}
						<form
							class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
							onsubmit={(e) => saveEdit(e, t)}
							novalidate
						>
							<div>
								<Label for="{uid}-edit-title" class="block text-sm/6 font-medium text-fg">
									Title
								</Label>
								<Input
									id="{uid}-edit-title"
									bind:value={editTitle}
									maxlength={120}
									disabled={editBusy}
									class="mt-1.5 {inputClass}"
								/>
							</div>
							<div>
								<Label for="{uid}-edit-desc" class="block text-sm/6 font-medium text-fg">
									Description
								</Label>
								<textarea
									id="{uid}-edit-desc"
									rows={4}
									bind:value={editDesc}
									disabled={editBusy}
									class="mt-1.5 {textareaClass}"></textarea>
							</div>
							{#if editError}
								<p class={errorClass} role="alert">{editError}</p>
							{/if}
							<div class="flex justify-end gap-2">
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									disabled={editBusy}
									onclick={() => (editing = false)}>Cancel</Button
								>
								<Button type="submit" size="sm" class={mobileButton} disabled={editBusy}>
									{editBusy ? 'Saving…' : 'Save changes'}
								</Button>
							</div>
						</form>
					{:else if t.type === 'task'}
						<section
							aria-labelledby="{uid}-desc-heading"
							class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
						>
							<h3 id="{uid}-desc-heading" class="text-sm/6 font-semibold text-fg">Description</h3>
							{#if t.description}
								<p class="whitespace-pre-wrap text-sm/6 text-fg-body [overflow-wrap:anywhere]">
									{t.description}
								</p>
							{:else}
								<p class="text-sm/6 text-fg-muted">No description yet.</p>
							{/if}
						</section>
					{/if}

					<!-- Bug-specific sections -->
					{#if t.type === 'bug' && t.bug}
						<BugSection
							{organizationId}
							task={t}
							bug={t.bug}
							{choices}
							stale={isStale}
							oncreatefix={() => (createFixOpen = true)}
							onchanged={() => reloadDetail()}
						/>
					{/if}

					<!-- Request-specific sections -->
					{#if t.type === 'request' && t.request}
						<RequestSection
							{organizationId}
							task={t}
							request={t.request}
							stale={isStale}
							onchanged={() => reloadDetail()}
						/>
					{/if}

					<!-- Checklist (tasks & bugs) -->
					{#if t.type !== 'request'}
						<ChecklistSection
							{organizationId}
							taskId={t.id}
							steps={t.steps}
							canEdit={t.can.edit}
							stale={isStale}
							onchanged={() => reloadDetail()}
						/>
					{/if}

					<!-- Output (§9.3) -->
					<section
						aria-labelledby="{uid}-output-heading"
						class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
						data-testid="output-section"
					>
						<h3 id="{uid}-output-heading" class="text-sm/6 font-semibold text-fg">Output</h3>
						{#if t.completion}
							<div
								class="rounded-md bg-surface p-3 text-sm/6 outline-1 -outline-offset-1 outline-line-subtle"
							>
								<p class="text-xs/5 text-fg-muted">
									Completion note by {t.completion.by.name} · {when(t.completion.at)}
								</p>
								<p class="mt-1 whitespace-pre-wrap text-fg-body">{t.completion.text}</p>
							</div>
						{:else}
							<p class="text-sm/6 text-fg-muted">No outcome recorded.</p>
						{/if}

						{#if t.output.length > 0}
							<ul role="list" class="divide-y divide-line-subtle text-sm/6">
								{#each t.output as out (out.id)}
									<li class="flex flex-wrap items-baseline justify-between gap-2 py-2">
										<div class="flex min-w-0 items-center gap-2">
											<span
												class="rounded bg-surface-overlay px-1.5 py-0.5 text-xs/5 text-fg-muted"
											>
												{out.kind}
											</span>
											{#if out.href}
												<a
													href={resolve(out.href as '/')}
													class="font-medium text-link hover:underline [overflow-wrap:anywhere]"
												>
													{out.label}
												</a>
											{:else}
												<span
													class="font-mono text-xs/5 text-fg-secondary [overflow-wrap:anywhere]"
												>
													{out.label}
												</span>
											{/if}
										</div>
										{#if out.unresolved}
											<span class="text-xs/5 text-fg-muted">{out.unresolved}</span>
										{/if}
									</li>
								{/each}
							</ul>
						{/if}
					</section>

					<!-- Why block (decisions-and-tech-debt §5) -->
					<section
						aria-labelledby="{uid}-why-heading"
						class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
						data-testid="why-section"
					>
						<div class="flex flex-wrap items-center justify-between gap-2">
							<h3 id="{uid}-why-heading" class="text-sm/6 font-semibold text-fg">Why</h3>
							{#if t.can.edit && !isStale && !whyPickerOpen}
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={() => openWhyPicker(t)}>Link document…</Button
								>
							{/if}
						</div>

						{#if t.why.length === 0}
							<p class="text-sm/6 text-fg-muted">No linked decisions, designs or specs yet.</p>
						{:else}
							<ul role="list" class="flex flex-col gap-2 text-sm/6">
								{#each t.why as doc (doc.id)}
									<li class="flex flex-wrap items-center justify-between gap-2">
										<span class="text-fg">
											{#if doc.key}
												<span class="mr-1.5 font-mono text-xs/5 text-fg-muted">{doc.key}</span>
											{/if}
											{doc.title}
										</span>
										<span class="text-xs/5 text-fg-muted">{doc.kind} · {doc.status}</span>
									</li>
								{/each}
							</ul>
						{/if}

						{#if whyPickerOpen}
							<div class="flex flex-wrap items-center gap-2 pt-1">
								<select
									aria-label="Choose document to link"
									bind:value={selectedWhyId}
									disabled={linkingWhy}
									class="min-w-48 flex-1 {selectClass}"
								>
									<option value="">Choose a document…</option>
									{#each whyOptions as opt (opt.id)}
										<option value={opt.id}>
											{opt.key ? `${opt.key} · ` : ''}{opt.title} ({opt.kind})
										</option>
									{/each}
								</select>
								<Button
									type="button"
									size="sm"
									class={mobileButton}
									disabled={!selectedWhyId || linkingWhy}
									onclick={() => submitLinkWhy(t)}
								>
									{linkingWhy ? 'Linking…' : 'Link'}
								</Button>
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={() => (whyPickerOpen = false)}>Cancel</Button
								>
							</div>
						{/if}
					</section>

					<!-- Shared discussion & Private notes -->
					<DiscussionSection
						{organizationId}
						taskId={t.id}
						comments={t.comments}
						canComment={t.can.edit}
						stale={isStale}
						onsharedchanged={() => reloadDetail()}
					/>

					<!-- Immutable Activity feed (newest last; unknown event kinds render as "Unknown change") -->
					<section
						aria-labelledby="{uid}-activity-heading"
						class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line"
						data-testid="activity-section"
					>
						<h3 id="{uid}-activity-heading" class="text-sm/6 font-semibold text-fg">Activity</h3>
						{#if t.activity.length === 0}
							<p class="text-sm/6 text-fg-muted">No activity recorded.</p>
						{:else}
							<ol class="flex flex-col gap-2.5 text-sm/6" data-testid="activity-events">
								{#each t.activity as ev (ev.id)}
									{@const known = activityKindLabel(ev.kind) !== 'unknown'}
									<li class="flex flex-col gap-0.5 border-l-2 border-line pl-3">
										<div class="flex flex-wrap items-baseline justify-between gap-2">
											<p class="text-fg-secondary [overflow-wrap:anywhere]">
												{#if known}
													<span class="font-medium text-fg">
														{ev.actor ? ev.actor.name : 'System'}
													</span>
													{ev.text}
												{:else}
													<span class="font-medium text-fg">Unknown change</span>
													{#if ev.actor}
														by {ev.actor.name}
													{/if}
												{/if}
											</p>
											<time class="shrink-0 text-xs/5 text-fg-muted" datetime={ev.at}>
												{when(ev.at)}
											</time>
										</div>
										{#if known && ev.note}
											<p class="text-xs/5 text-fg-muted [overflow-wrap:anywhere]">{ev.note}</p>
										{/if}
									</li>
								{/each}
							</ol>
						{/if}
					</section>
				</div>

				<!-- Side column -->
				<WorkSideColumn
					{organizationId}
					task={t}
					{choices}
					stale={isStale}
					onchanged={() => reloadDetail()}
				/>
			</div>

			<WorkDetailDialogs
				{organizationId}
				task={t}
				bind:closeOpen
				initialCloseKind={closeKind}
				bind:reopenOpen
				bind:blockOpen
				onchanged={() => reloadDetail()}
			/>

			{#if t.type === 'bug'}
				<CaptureDialog
					bind:open={createFixOpen}
					{organizationId}
					initialType="task"
					projectId={t.project?.id ?? null}
					fixesTaskId={t.id}
					contextLabel="Creating a fix task linked to Bug {t.ref} ({t.title})."
					oncreated={() => reloadDetail()}
				/>
			{/if}
		{/if}
	{/snippet}
</Loaded>
