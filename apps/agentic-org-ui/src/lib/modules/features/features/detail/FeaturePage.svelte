<!--
  @component
  Feature page (`/projects/[id]/features/[featureId]`, features-ux §2–§3, FTR-5..14):
  Shows canonical definition and path, owner, explicit status controls (active owner required for
  In progress/Live; setting Live starts nothing automatically), Edit definition (preserving typed
  draft on failure/conflict), Break into tasks, Retire (with reason + cycle-checked replacement),
  tasks grouped by current state with Link/Unlink (confirming when moving a task from another
  feature), Affected bugs separately from implementation tasks, Supporting knowledge, Included in
  releases backlinks, Cost & time (authorized or explicit unavailable), and Git revision history.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { toast } from 'svelte-sonner';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import ListPlusIcon from '@lucide/svelte/icons/list-plus';
	import Link2Icon from '@lucide/svelte/icons/link-2';
	import ArchiveIcon from '@lucide/svelte/icons/archive';
	import AlertTriangleIcon from '@lucide/svelte/icons/alert-triangle';
	import Loaded from '$lib/components/Loaded.svelte';
	import Avatar from '$lib/components/Avatar.svelte';
	import CopyButton from '$lib/components/CopyButton.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Dialog from '$lib/components/ui/dialog';
	import {
		errorClass,
		gutters,
		inputClass,
		mobileButton,
		selectClass,
		textareaClass
	} from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatDate } from '$lib/modules/preferences/time';
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/proto';
	import { projectFrame } from '$lib/modules/projects';
	import { featuresApi } from '../../api';
	import {
		FEATURE_STATUS_TONE,
		FEATURE_STATUS_WORD,
		checkFeatureSlug,
		conflictLatestFeature,
		featureHref,
		featureProgressStats,
		featurePublicSlug,
		featureRef,
		featuresHref,
		formatMicroUsd,
		groupFeatureTasks
	} from '../../present';
	import type {
		EligibleTaskChoice,
		FeatureDetail,
		FeatureStatus,
		ProjectFeaturesOverview
	} from '../../types';
	import RetireFeatureDialog from '../dialogs/RetireFeatureDialog.svelte';
	import BreakIntoTasksDialog from '../dialogs/BreakIntoTasksDialog.svelte';

	let { featureId }: { featureId: string } = $props();

	const frame = projectFrame();
	const uid = $props.id();

	type FeatureLookup =
		| { found: true; feature: FeatureDetail; overview: ProjectFeaturesOverview }
		| { found: false; message: string; overview: ProjectFeaturesOverview };

	async function loadFeature(): Promise<FeatureLookup> {
		const overview = await featuresApi.list(frame.organizationId, frame.projectId);
		try {
			const feature = await featuresApi.get(frame.organizationId, frame.projectId, featureId);
			return { found: true, feature, overview };
		} catch (e) {
			const err = toAppError(e);
			if (err.source === 'api' && err.code === 'not_found') {
				return { found: false, message: err.message, overview };
			}
			throw e;
		}
	}

	const featureRead = new Resource<FeatureLookup>(() => loadFeature());

	const lookup = $derived(featureRead.data);
	const feat = $derived(lookup?.found ? lookup.feature : undefined);
	const overview = $derived(lookup?.overview);
	const stale = $derived(!!feat?.stale || !!overview?.stale || !!frame.project.stale);
	const canEdit = $derived(!!feat?.canEdit && !stale && !frame.project.archived);

	// Edit definition state (writable $derived so ?edit=1 opens editor cleanly without $effect)
	let editing = $derived(!!feat && canEdit && page.url.searchParams.get('edit') === '1');
	let editTitle = $derived(feat?.title ?? '');
	let editSlug = $derived(feat?.localSlug ?? '');
	let editOwnerId = $derived(feat?.owner?.id ?? '');
	let editBody = $derived(feat?.bodyMarkdown ?? '');
	let editBusy = $state(false);
	let editError = $state<string | undefined>();
	let conflictFeature = $state<FeatureDetail | null>(null);

	// Status change state
	let statusBusy = $state(false);

	// Dialogs
	let retireOpen = $state(false);
	let breakOpen = $state(false);
	let linkOpen = $state(false);
	let eligibleTasks = $state.raw<EligibleTaskChoice[]>([]);
	let selectedTaskId = $state('');
	let linkBusy = $state(false);
	let linkError = $state<string | undefined>();
	let moveConfirmTask = $state<EligibleTaskChoice | null>(null);
	let repairBusy = $state(false);

	const title = $derived(
		feat
			? pageTitle(`${featureRef(feat)} ${feat.title}`, 'Features', frame.project.name)
			: pageTitle('Feature', 'Features', frame.project.name)
	);

	function formatWhen(iso: string): string {
		const date = new Date(iso);
		if (Number.isNaN(date.getTime())) return iso;
		return formatDate(date, accountZone.zone, new Date());
	}

	function parseBodyBlocks(
		markdown: string
	): (
		| { kind: 'heading'; text: string }
		| { kind: 'list'; items: string[] }
		| { kind: 'p'; text: string }
	)[] {
		const out: (
			| { kind: 'heading'; text: string }
			| { kind: 'list'; items: string[] }
			| { kind: 'p'; text: string }
		)[] = [];
		let para: string[] = [];
		const flush = () => {
			if (para.length) out.push({ kind: 'p', text: para.join(' ') });
			para = [];
		};
		for (const raw of markdown.split(/\r?\n/)) {
			const line = raw.trim();
			const h = /^#{1,6}\s+(.*)$/.exec(line);
			const li = /^[-*+]\s+(.*)$/.exec(line);
			if (!line) flush();
			else if (h) {
				flush();
				out.push({ kind: 'heading', text: h[1] });
			} else if (li) {
				flush();
				const last = out.at(-1);
				if (last?.kind === 'list') last.items.push(li[1]);
				else out.push({ kind: 'list', items: [li[1]] });
			} else {
				para.push(line);
			}
		}
		flush();
		return out;
	}

	function beginEdit(f: FeatureDetail) {
		editTitle = f.title;
		editSlug = f.localSlug;
		editOwnerId = f.owner?.id ?? '';
		editBody = f.bodyMarkdown;
		editError = undefined;
		conflictFeature = null;
		editing = true;
	}

	async function saveEdit(expectedRevision?: number) {
		if (!feat || editBusy) return;
		if (!editTitle.trim()) {
			editError = 'Enter a title for the feature.';
			return;
		}
		const slugProblem = checkFeatureSlug(editSlug);
		if (slugProblem) {
			editError = slugProblem;
			return;
		}
		if ((feat.status === 'in_progress' || feat.status === 'live') && !editOwnerId) {
			editError = 'An In progress or Live feature needs an active accountable owner.';
			return;
		}
		editBusy = true;
		editError = undefined;
		try {
			await featuresApi.save(frame.organizationId, frame.projectId, feat.id, {
				title: editTitle.trim(),
				localSlug: editSlug.trim(),
				ownerId: editOwnerId || null,
				bodyMarkdown: editBody,
				expectedRevision: expectedRevision ?? feat.revision
			});
			editing = false;
			conflictFeature = null;
			toast.success(`Saved ${featureRef(feat)}.`);
			await featureRead.reload();
		} catch (e) {
			const latest = conflictLatestFeature(e);
			if (latest) {
				conflictFeature = latest;
				editError = `${featureRef(latest)} changed while you were editing (rev ${latest.revision}). Your draft is kept below.`;
			} else {
				editError = toAppError(e).message;
			}
		} finally {
			editBusy = false;
		}
	}

	async function changeStatus(nextStatus: Exclude<FeatureStatus, 'retired'>) {
		if (!feat || statusBusy || nextStatus === feat.status) return;
		const ownerId = feat.owner?.id ?? overview?.owners[0]?.id ?? null;
		if ((nextStatus === 'in_progress' || nextStatus === 'live') && !ownerId) {
			toast.error('Assign an accountable owner before moving this feature to In progress or Live.');
			return;
		}
		statusBusy = true;
		try {
			await featuresApi.setStatus(frame.organizationId, frame.projectId, feat.id, {
				status: nextStatus,
				ownerId: feat.owner?.id ?? ownerId,
				expectedRevision: feat.revision
			});
			toast.success(`${featureRef(feat)} marked ${FEATURE_STATUS_WORD[nextStatus]}.`);
			await featureRead.reload();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			statusBusy = false;
		}
	}

	async function openLinkDialog() {
		if (!feat) return;
		linkError = undefined;
		moveConfirmTask = null;
		selectedTaskId = '';
		linkOpen = true;
		try {
			eligibleTasks = await featuresApi.eligibleTasks(
				frame.organizationId,
				frame.projectId,
				feat.id
			);
			selectedTaskId = eligibleTasks[0]?.id ?? '';
		} catch (e) {
			linkError = toAppError(e).message;
		}
	}

	async function submitLinkTask(confirmedMove = false) {
		if (!feat || !selectedTaskId || linkBusy) return;
		const chosen = eligibleTasks.find((t) => t.id === selectedTaskId);
		if (!chosen) return;
		if (chosen.currentFeature && !confirmedMove) {
			moveConfirmTask = chosen;
			return;
		}
		linkBusy = true;
		linkError = undefined;
		try {
			await featuresApi.linkTask(frame.organizationId, frame.projectId, feat.id, {
				taskId: chosen.id,
				expectedRevision: feat.revision
			});
			linkOpen = false;
			moveConfirmTask = null;
			toast.success(`Linked ${chosen.ref} to ${featureRef(feat)}.`);
			await featureRead.reload();
		} catch (e) {
			linkError = toAppError(e).message;
		} finally {
			linkBusy = false;
		}
	}

	async function unlinkTask(taskId: string, ref: string) {
		if (!feat) return;
		try {
			await featuresApi.unlinkTask(frame.organizationId, frame.projectId, feat.id, { taskId });
			toast.success(`Unlinked ${ref} from ${featureRef(feat)}.`);
			await featureRead.reload();
		} catch (e) {
			toast.error(toAppError(e).message);
		}
	}

	async function repairFile() {
		if (!feat || repairBusy) return;
		repairBusy = true;
		try {
			await featuresApi.repair(frame.organizationId, frame.projectId, feat.id);
			toast.success(`Repaired canonical file for ${featureRef(feat)}.`);
			await featureRead.reload();
		} catch (e) {
			toast.error(toAppError(e).message);
		} finally {
			repairBusy = false;
		}
	}
</script>

<svelte:head><title>{title}</title></svelte:head>

<div class="border-b border-line-subtle py-3 {gutters}">
	<a
		href={resolve(featuresHref(frame.projectId) as '/')}
		class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
	>
		Features
	</a>
</div>

<Loaded
	resource={featureRead}
	label="the feature"
	error="Couldn't load this feature"
	errorLevel={4}
	rows={5}
>
	{#snippet children(res)}
		{#if !res.found}
			<div class="py-8 {gutters}">
				<h3 class="text-base/7 font-semibold text-fg">Feature not found</h3>
				<p class="mt-1 max-w-prose text-sm/6 text-fg-muted">{res.message}</p>
			</div>
		{:else}
			{@const f = res.feature}
			{@const ov = res.overview}
			{@const stats = featureProgressStats(f.progress)}
			{@const taskGroups = groupFeatureTasks(f.tasks)}

			<!-- Header & actions -->
			<div class="border-b border-line-subtle py-6 {gutters}" data-testid="feature-header">
				<div class="flex flex-wrap items-start justify-between gap-4">
					<div class="min-w-0 flex-1 basis-72">
						<div class="flex flex-wrap items-center gap-2">
							<span class="font-mono text-xs/5 font-semibold text-fg-muted">{featureRef(f)}</span>
							<StatusPill
								label={FEATURE_STATUS_WORD[f.status]}
								class={tone[FEATURE_STATUS_TONE[f.status]]}
							/>
							{#if f.pendingPublication}
								<StatusPill label="Pending publication" class={tone.owner} />
							{/if}
						</div>
						<h3 class="mt-2 text-lg/7 font-semibold text-fg [overflow-wrap:anywhere]">
							{f.title}
						</h3>
						<p class="mt-1 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs/5 text-fg-muted">
							<span class="font-mono">{featurePublicSlug(f)}</span>
							<span class="inline-flex items-center gap-1.5">
								{#if f.owner}
									<Avatar name={f.owner.name} size="sm" />
									<span>Owner: <span class="text-fg-secondary">{f.owner.name}</span></span>
								{:else}
									<span>Owner: Unassigned</span>
								{/if}
							</span>
							<span>
								Updated <time datetime={f.updatedAt}>{formatWhen(f.updatedAt)}</time> · rev {f.revision}
							</span>
						</p>
						<p class="mt-1 font-mono text-xs/5 text-fg-muted [overflow-wrap:anywhere]">
							{f.path} · ID: {f.id}
						</p>
					</div>

					{#if canEdit && !editing}
						<div class="flex flex-wrap items-center gap-2">
							{#if f.status !== 'retired'}
								<Label for="{uid}-status-select" class="sr-only">Change feature status</Label>
								<select
									id="{uid}-status-select"
									aria-label="Feature status"
									value={f.status}
									disabled={statusBusy}
									onchange={(e) =>
										changeStatus(
											(e.currentTarget as HTMLSelectElement).value as Exclude<
												FeatureStatus,
												'retired'
											>
										)}
									class="w-auto {selectClass}"
								>
									<option value="planned">Planned</option>
									<option value="in_progress">In progress</option>
									<option value="live">Live</option>
								</select>

								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={() => (breakOpen = true)}
									data-testid="break-into-tasks-btn"
								>
									<ListPlusIcon data-icon="inline-start" />
									Break into tasks…
								</Button>

								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={() => (retireOpen = true)}
									data-testid="retire-feature-btn"
								>
									<ArchiveIcon data-icon="inline-start" />
									Retire…
								</Button>
							{/if}

							<Button
								type="button"
								size="sm"
								class={mobileButton}
								onclick={() => beginEdit(f)}
								data-testid="edit-feature-btn"
							>
								<PencilIcon data-icon="inline-start" />
								Edit
							</Button>
						</div>
					{/if}
				</div>

				{#if f.status === 'retired'}
					<div
						class="mt-4 rounded-lg bg-surface-raised p-4 text-sm/6 ring-1 ring-line"
						data-testid="retired-banner"
					>
						<p class="font-semibold text-fg">Retired</p>
						{#if f.retiredReason}
							<p class="mt-1 text-fg-body">{f.retiredReason}</p>
						{/if}
						{#if f.replacedBy}
							<p class="mt-1 text-xs/5 text-fg-muted">
								Replaced by:
								<a
									href={resolve(featureHref(frame.projectId, f.replacedBy.id) as '/')}
									class="font-semibold text-link hover:underline"
								>
									{f.replacedBy.ref}
									{f.replacedBy.title}
								</a>
							</p>
						{/if}
						<p class="mt-1 text-xs/5 text-fg-muted">
							Task links and release history remain preserved.
						</p>
					</div>
				{/if}

				{#if f.problem}
					<div
						class="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-lg bg-warning/10 p-4 text-sm/6 text-warning-text-strong ring-1 ring-warning/30"
						role="alert"
						data-testid="feature-problem"
					>
						<div class="flex items-start gap-2">
							<AlertTriangleIcon class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
							{#if f.problem.kind === 'malformed_metadata'}
								<p>
									Malformed metadata in field <span class="font-mono">{f.problem.field}</span>: {f
										.problem.message}. The body remains readable below.
								</p>
							{:else}
								<p>
									Canonical file missing at <span class="font-mono">{f.problem.expectedPath}</span>: {f
										.problem.message}. Feature identity, tasks, and history are kept.
								</p>
							{/if}
						</div>
						{#if canEdit}
							<Button
								type="button"
								variant="outline"
								size="sm"
								class={mobileButton}
								disabled={repairBusy}
								onclick={repairFile}
							>
								{repairBusy ? 'Repairing…' : 'Repair file'}
							</Button>
						{/if}
					</div>
				{/if}
			</div>

			<!-- Main grid: Definition + Tasks + Affected Bugs | Metadata + Knowledge + Releases + Cost + History -->
			<div
				class="grid grid-cols-1 gap-8 py-6 min-[1024px]:grid-cols-[minmax(0,2fr)_minmax(0,1fr)] {gutters}"
			>
				<div class="flex min-w-0 flex-col gap-8">
					<!-- Definition reader or editor -->
					<section aria-labelledby="{uid}-def-h" data-testid="feature-definition">
						<h4 id="{uid}-def-h" class="text-sm/6 font-semibold text-fg">Capability definition</h4>

						{#if editing}
							<div
								class="mt-3 flex flex-col gap-4 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
							>
								{#if conflictFeature}
									<div
										class="flex flex-col gap-2 rounded-lg bg-warning/10 p-3 text-xs/5 text-warning-text-strong ring-1 ring-warning/30"
										role="alert"
										data-testid="feature-conflict"
									>
										<p class="font-semibold">
											Changed to rev {conflictFeature.revision} while you were editing. Your draft is
											kept below.
										</p>
										<div class="flex flex-wrap gap-2">
											<Button
												type="button"
												size="sm"
												class={mobileButton}
												disabled={editBusy}
												onclick={() => saveEdit(conflictFeature!.revision)}
											>
												Save draft at rev {conflictFeature.revision}
											</Button>
											<Button
												type="button"
												variant="outline"
												size="sm"
												class={mobileButton}
												onclick={() => beginEdit(conflictFeature!)}
											>
												Load latest
											</Button>
										</div>
									</div>
								{:else if editError}
									<div
										class="flex flex-col gap-2 rounded-lg bg-danger-emphasis/10 p-3 text-xs/5 text-danger-text ring-1 ring-danger-emphasis/20"
										role="alert"
									>
										<p>{editError}</p>
										<div class="flex flex-wrap gap-2">
											<Button
												type="button"
												size="sm"
												class={mobileButton}
												disabled={editBusy}
												onclick={() => saveEdit()}
											>
												Retry
											</Button>
											<CopyButton text={editBody} label="Copy definition" size="sm" />
										</div>
									</div>
								{/if}

								<div class="grid grid-cols-1 gap-3 min-[640px]:grid-cols-2">
									<div class="min-[640px]:col-span-2">
										<Label for="{uid}-ed-title" class="mb-1 block text-xs/5 font-medium text-fg"
											>Title</Label
										>
										<Input id="{uid}-ed-title" bind:value={editTitle} class={inputClass} />
									</div>
									<div>
										<Label for="{uid}-ed-slug" class="mb-1 block text-xs/5 font-medium text-fg"
											>Local slug</Label
										>
										<Input
											id="{uid}-ed-slug"
											bind:value={editSlug}
											class="font-mono {inputClass}"
										/>
									</div>
									<div>
										<Label for="{uid}-ed-owner" class="mb-1 block text-xs/5 font-medium text-fg"
											>Accountable owner</Label
										>
										<select id="{uid}-ed-owner" bind:value={editOwnerId} class={selectClass}>
											<option value="">Unassigned</option>
											{#each ov.owners as emp (emp.id)}
												<option value={emp.id}>{emp.name} ({emp.kind})</option>
											{/each}
										</select>
									</div>
								</div>

								<div>
									<Label for="{uid}-ed-body" class="mb-1 block text-xs/5 font-medium text-fg"
										>Definition (Markdown)</Label
									>
									<textarea
										id="{uid}-ed-body"
										rows={12}
										bind:value={editBody}
										class="font-mono {textareaClass}"></textarea>
								</div>

								<div class="flex flex-wrap gap-2">
									<Button
										type="button"
										size="sm"
										class={mobileButton}
										disabled={editBusy}
										onclick={() => saveEdit()}
									>
										{editBusy ? 'Saving…' : 'Save definition'}
									</Button>
									<Button
										type="button"
										variant="outline"
										size="sm"
										class={mobileButton}
										disabled={editBusy}
										onclick={() => {
											editing = false;
											conflictFeature = null;
											editError = undefined;
										}}
									>
										Cancel
									</Button>
								</div>
							</div>
						{:else}
							<div
								class="mt-3 flex max-w-prose flex-col gap-3 text-sm/6 text-fg-body [overflow-wrap:anywhere]"
							>
								{#each parseBodyBlocks(f.bodyMarkdown) as blk, i (`${i}-${blk.kind}`)}
									{#if blk.kind === 'heading'}
										<h5 class="pt-2 text-sm/6 font-semibold text-fg first:pt-0">{blk.text}</h5>
									{:else if blk.kind === 'list'}
										<ul role="list" class="flex list-disc flex-col gap-1 pl-5 marker:text-fg-muted">
											{#each blk.items as li, j (`${i}-${j}`)}
												<li>{li}</li>
											{/each}
										</ul>
									{:else}
										<p>{blk.text}</p>
									{/if}
								{/each}
							</div>
						{/if}
					</section>

					<!-- Implementation tasks grouped by state (FTR-6, FTR-11, FTR-12) -->
					<section
						aria-labelledby="{uid}-tasks-h"
						class="border-t border-line-subtle pt-6"
						data-testid="feature-tasks"
					>
						<div class="flex flex-wrap items-center justify-between gap-2">
							<div>
								<h4 id="{uid}-tasks-h" class="text-sm/6 font-semibold text-fg">
									Implementation tasks ({f.tasks.length})
								</h4>
								<p class="text-xs/5 text-fg-muted">
									{stats.label}{stats.cancelledNote ? ` · ${stats.cancelledNote}` : ''}
								</p>
							</div>
							{#if f.canLinkTasks && !stale && !frame.project.archived}
								<Button
									type="button"
									variant="outline"
									size="sm"
									class={mobileButton}
									onclick={openLinkDialog}
									data-testid="link-task-btn"
								>
									<Link2Icon data-icon="inline-start" />
									Link a task
								</Button>
							{/if}
						</div>

						{#if taskGroups.length === 0}
							<p class="mt-3 text-sm/6 text-fg-muted">
								No implementation tasks linked yet. Use Break into tasks or Link a task.
							</p>
						{:else}
							<div class="mt-4 flex flex-col gap-4">
								{#each taskGroups as grp (grp.status)}
									<div>
										<h5 class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase">
											{grp.label} · {grp.items.length}
										</h5>
										<ul
											role="list"
											class="mt-1.5 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
										>
											{#each grp.items as t (t.id)}
												<li
													class="flex flex-wrap items-center justify-between gap-2 px-4 py-3 text-sm/6"
												>
													<div class="min-w-0 flex-1">
														<a
															href={resolve(`/work/${encodeURIComponent(t.ref)}` as '/')}
															class="font-medium text-link hover:underline [overflow-wrap:anywhere]"
														>
															<span class="mr-1.5 font-mono text-xs/5">{t.ref}</span>
															{t.title}
														</a>
														<p class="text-xs/5 text-fg-muted">
															{t.type === 'bug' ? 'Bug fix task' : 'Task'} · Accountable: {t.team
																.name}
															{#if t.assignee}
																· Assignee: {t.assignee.name}
															{/if}
															{#if t.archived}
																· Archived (tombstone)
															{/if}
														</p>
													</div>
													{#if f.canLinkTasks && !stale && !frame.project.archived}
														<Button
															type="button"
															variant="outline"
															size="sm"
															class={mobileButton}
															onclick={() => unlinkTask(t.id, t.ref)}
														>
															Unlink
														</Button>
													{/if}
												</li>
											{/each}
										</ul>
									</div>
								{/each}
							</div>
						{/if}
					</section>

					<!-- Affected bugs (details.affects_feature_id) kept separate from implementation tasks (FTR-11) -->
					<section
						aria-labelledby="{uid}-affected-bugs-h"
						class="border-t border-line-subtle pt-6"
						data-testid="feature-affected-bugs"
					>
						<h4 id="{uid}-affected-bugs-h" class="text-sm/6 font-semibold text-fg">
							Affected bugs ({f.affectedBugs.length})
						</h4>
						<p class="mt-0.5 text-xs/5 text-fg-muted">
							Bugs that affect this capability (<span class="font-mono">affects_feature_id</span>),
							listed separately from implementation tasks.
						</p>
						{#if f.affectedBugs.length === 0}
							<p class="mt-2 text-sm/6 text-fg-muted">No reported bugs affect this feature.</p>
						{:else}
							<ul
								role="list"
								class="mt-3 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
							>
								{#each f.affectedBugs as bug (bug.id)}
									<li class="flex flex-wrap items-center justify-between gap-2 px-4 py-3 text-sm/6">
										<a
											href={resolve(`/work/${encodeURIComponent(bug.ref)}` as '/')}
											class="font-medium text-link hover:underline [overflow-wrap:anywhere]"
										>
											<span class="mr-1.5 font-mono text-xs/5">{bug.ref}</span>
											{bug.title}
										</a>
										<span class="text-xs/5 text-fg-muted">{bug.status}</span>
									</li>
								{/each}
							</ul>
						{/if}
					</section>
				</div>

				<!-- Side column: Supporting knowledge, Releases, Cost & time, History -->
				<aside aria-label="Feature context" class="flex flex-col gap-5">
					<!-- Supporting designs, decisions & reports -->
					<section
						aria-labelledby="{uid}-docs-h"
						class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
						data-testid="feature-supporting-docs"
					>
						<h4 id="{uid}-docs-h" class="text-sm/6 font-semibold text-fg">
							Supporting knowledge ({f.supportingDocs.length})
						</h4>
						{#if f.supportingDocs.length === 0}
							<p class="text-xs/5 text-fg-muted">
								No supporting designs, decisions, or reports yet.
							</p>
						{:else}
							<ul role="list" class="flex flex-col gap-2 text-sm/6">
								{#each f.supportingDocs as doc (doc.id)}
									<li class="flex items-baseline justify-between gap-2">
										<a
											href={resolve(
												`/projects/${encodeURIComponent(frame.projectId)}/docs/${encodeURIComponent(doc.id)}` as '/'
											)}
											class="text-link hover:underline [overflow-wrap:anywhere]"
										>
											{#if doc.key}
												<span class="mr-1 font-mono text-xs/5">{doc.key}</span>
											{/if}
											{doc.title}
										</a>
										<span class="shrink-0 text-xs/5 text-fg-muted">{doc.kind} · {doc.status}</span>
									</li>
								{/each}
							</ul>
						{/if}
					</section>

					<!-- Included in releases (FTR-6, FTR-11) -->
					<section
						aria-labelledby="{uid}-releases-h"
						class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
						data-testid="feature-releases"
					>
						<h4 id="{uid}-releases-h" class="text-sm/6 font-semibold text-fg">
							Included in release ({f.releases.length})
						</h4>
						{#if f.releases.length === 0}
							<p class="text-xs/5 text-fg-muted">Not listed in any release manifest yet.</p>
						{:else}
							<ul role="list" class="flex flex-col gap-2 text-sm/6">
								{#each f.releases as rel (rel.releaseId)}
									<li class="flex flex-col gap-0.5">
										<div class="flex items-baseline justify-between gap-2">
											<a
												href={resolve(
													`/projects/${encodeURIComponent(frame.projectId)}/releases/${encodeURIComponent(rel.releaseId)}` as '/'
												)}
												class="font-medium text-link hover:underline"
											>
												<span class="mr-1 font-mono text-xs/5">{rel.ref}</span>
												{rel.title}
											</a>
											<span class="text-xs/5 text-fg-muted">{rel.status}</span>
										</div>
										{#if rel.productChangelogDocId}
											<a
												href={resolve(
													`/projects/${encodeURIComponent(frame.projectId)}/docs/${encodeURIComponent(rel.productChangelogDocId)}` as '/'
												)}
												class="text-xs/5 text-link hover:underline"
											>
												{rel.productChangelogTitle ?? 'Product changelog'}
											</a>
										{/if}
									</li>
								{/each}
							</ul>
							<p class="mt-1 text-xs/5 text-fg-muted">
								Manifest inclusion does not by itself prove deployment or availability.
							</p>
						{/if}
					</section>

					<!-- Cost and time (FTR-11) -->
					<section
						aria-labelledby="{uid}-cost-h"
						class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
						data-testid="feature-cost"
					>
						<h4 id="{uid}-cost-h" class="text-sm/6 font-semibold text-fg">Cost and time</h4>
						{#if !f.cost.allowed}
							<p class="text-xs/5 text-fg-muted">{f.cost.reason}</p>
						{:else}
							<dl class="divide-y divide-line-subtle text-xs/5">
								<div class="flex justify-between gap-2 py-1.5">
									<dt class="text-fg-muted">Known spend</dt>
									<dd class="font-mono text-fg">
										{f.cost.knownCostMicroUsd === null
											? 'Unavailable'
											: `${f.cost.complete ? '' : 'At least '}${formatMicroUsd(f.cost.knownCostMicroUsd)}`}
									</dd>
								</div>
								<div class="flex justify-between gap-2 py-1.5">
									<dt class="text-fg-muted">Known tokens</dt>
									<dd class="font-mono text-fg">
										{f.cost.knownTokens === null
											? 'Unavailable'
											: f.cost.knownTokens.toLocaleString('en-US')}
									</dd>
								</div>
								{#if f.cost.leadTimeLabel}
									<div class="flex justify-between gap-2 py-1.5">
										<dt class="text-fg-muted">Lead time</dt>
										<dd class="text-fg">{f.cost.leadTimeLabel}</dd>
									</div>
								{/if}
							</dl>
							{#if !f.cost.complete && f.cost.incompleteReason}
								<p class="text-xs/5 text-warning-text-strong">
									Incomplete usage: {f.cost.incompleteReason}
								</p>
							{/if}
						{/if}
					</section>

					<!-- Git revision history -->
					<section
						aria-labelledby="{uid}-hist-h"
						class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
						data-testid="feature-history"
					>
						<h4 id="{uid}-hist-h" class="text-sm/6 font-semibold text-fg">
							Git revision history ({f.history.length})
						</h4>
						<ul role="list" class="flex flex-col gap-2 text-xs/5">
							{#each [...f.history].toReversed() as entry (entry.revision)}
								<li class="border-b border-line-subtle pb-2 last:border-b-0 last:pb-0">
									<p class="text-fg-body">
										<span class="font-mono font-semibold text-fg">rev {entry.revision}</span> · {entry.summary}
									</p>
									<p class="text-fg-muted">
										{entry.by.name} · <time datetime={entry.at}>{formatWhen(entry.at)}</time>
									</p>
								</li>
							{/each}
						</ul>
					</section>
				</aside>
			</div>

			<!-- Dialogs -->
			<RetireFeatureDialog
				bind:open={retireOpen}
				organizationId={frame.organizationId}
				projectId={frame.projectId}
				feature={f}
				allFeatures={ov.features}
				onretired={() => void featureRead.reload()}
			/>

			<BreakIntoTasksDialog
				bind:open={breakOpen}
				organizationId={frame.organizationId}
				projectId={frame.projectId}
				ownerTeamName={ov.ownerTeam.name}
				feature={f}
				owners={ov.owners}
				teams={ov.teams}
				oncreated={() => void featureRead.reload()}
			/>

			<!-- Link a task dialog with cross-feature move confirmation (FTR-12) -->
			<Dialog.Root bind:open={linkOpen}>
				<Dialog.Content class="max-h-[calc(100dvh-2rem)] overflow-y-auto">
					<Dialog.Header>
						<Dialog.Title
							text={moveConfirmTask
								? `Move ${moveConfirmTask.ref} to ${featureRef(f)}?`
								: `Link a task to ${featureRef(f)}`}
						/>
						<Dialog.Description>
							{#if moveConfirmTask && moveConfirmTask.currentFeature}
								{moveConfirmTask.ref} is currently linked to {moveConfirmTask.currentFeature.ref}
								{moveConfirmTask.currentFeature.title}. Moving it to {featureRef(f)}
								{f.title} keeps its identity and history and changes neither feature's status.
							{:else}
								Select an eligible same-project task to link to {featureRef(f)} {f.title}.
							{/if}
						</Dialog.Description>
					</Dialog.Header>

					{#if moveConfirmTask}
						{#if linkError}
							<p class={errorClass} role="alert">{linkError}</p>
						{/if}
						<Dialog.Footer>
							<Button
								type="button"
								variant="outline"
								class={mobileButton}
								disabled={linkBusy}
								onclick={() => (moveConfirmTask = null)}
							>
								Back
							</Button>
							<Button
								type="button"
								class={mobileButton}
								disabled={linkBusy}
								onclick={() => submitLinkTask(true)}
							>
								{linkBusy ? 'Moving…' : `Move to ${featureRef(f)}`}
							</Button>
						</Dialog.Footer>
					{:else}
						<div class="flex flex-col gap-4">
							{#if eligibleTasks.length === 0}
								<p class="text-sm/6 text-fg-muted">
									No other eligible tasks in {frame.project.name} right now.
								</p>
							{:else}
								<div>
									<Label for="{uid}-link-select" class="mb-1 block text-sm/6 font-medium text-fg">
										Task
									</Label>
									<select id="{uid}-link-select" bind:value={selectedTaskId} class={selectClass}>
										{#each eligibleTasks as choice (choice.id)}
											<option value={choice.id}>
												{choice.ref}
												{choice.title}{choice.currentFeature
													? ` (currently ${choice.currentFeature.ref})`
													: ''}
											</option>
										{/each}
									</select>
								</div>
							{/if}
							{#if linkError}
								<p class={errorClass} role="alert">{linkError}</p>
							{/if}
							<Dialog.Footer>
								<Button
									type="button"
									variant="outline"
									class={mobileButton}
									disabled={linkBusy}
									onclick={() => (linkOpen = false)}
								>
									Cancel
								</Button>
								{#if eligibleTasks.length > 0}
									<Button
										type="button"
										class={mobileButton}
										disabled={linkBusy || !selectedTaskId}
										onclick={() => submitLinkTask(false)}
									>
										{linkBusy ? 'Linking…' : 'Link task'}
									</Button>
								{/if}
							</Dialog.Footer>
						</div>
					{/if}
				</Dialog.Content>
			</Dialog.Root>
		{/if}
	{/snippet}
</Loaded>
