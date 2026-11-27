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
	import { replaceState } from '$app/navigation';
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
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Label } from '$lib/components/ui/label';
	import { gutters, mobileButton, selectClass } from '$lib/components/classes';
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
		groupFeatureTasks
	} from '../../present';
	import type { FeatureDetail, FeatureStatus, ProjectFeaturesOverview } from '../../types';
	import RetireFeatureDialog from '../dialogs/RetireFeatureDialog.svelte';
	import BreakIntoTasksDialog from '../dialogs/BreakIntoTasksDialog.svelte';
	import FeatureDefinitionSection from './FeatureDefinitionSection.svelte';
	import FeatureLinkTaskDialog from './FeatureLinkTaskDialog.svelte';
	import FeatureSideColumn from './FeatureSideColumn.svelte';

	let { featureId }: { featureId: string } = $props();

	const frame = projectFrame();
	const uid = $props.id();

	type FeatureLookup =
		| { found: true; feature: FeatureDetail; overview: ProjectFeaturesOverview }
		| { found: false; message: string; overview: ProjectFeaturesOverview };

	let editing = $state(false);
	let editTitle = $state('');
	let editSlug = $state('');
	let editOwnerId = $state('');
	let editBody = $state('');
	let editBusy = $state(false);
	let editError = $state<string | undefined>();
	let conflictFeature = $state<FeatureDetail | null>(null);
	let consumedInitialEditFor: string | null = null;

	function clearEditQueryParam() {
		if (!page.url.searchParams.has('edit')) return;
		const kept = [...page.url.searchParams.entries()]
			.filter(([k]) => k !== 'edit')
			.map(([k, v]) =>
				v ? `${encodeURIComponent(k)}=${encodeURIComponent(v)}` : encodeURIComponent(k)
			);
		const next = kept.length > 0 ? `${page.url.pathname}?${kept.join('&')}` : page.url.pathname;
		replaceState(resolve(next as '/'), page.state);
	}

	async function loadFeature(): Promise<FeatureLookup> {
		const overview = await featuresApi.list(frame.organizationId, frame.projectId);
		try {
			const feature = await featuresApi.get(frame.organizationId, frame.projectId, featureId);
			const editable =
				feature.canEdit &&
				!feature.stale &&
				!overview.stale &&
				!frame.project.stale &&
				!frame.project.archived;
			if (
				consumedInitialEditFor !== feature.id &&
				editable &&
				page.url.searchParams.get('edit') === '1'
			) {
				consumedInitialEditFor = feature.id;
				beginEdit(feature);
			}
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

	let statusBusy = $state(false);
	let retireOpen = $state(false);
	let breakOpen = $state(false);
	let repairBusy = $state(false);
	let linkDialog = $state<ReturnType<typeof FeatureLinkTaskDialog>>();

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
			clearEditQueryParam();
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

	async function changeStatus(
		nextStatus: Exclude<FeatureStatus, 'retired'>,
		selectEl: HTMLSelectElement
	) {
		if (!feat || statusBusy || nextStatus === feat.status) {
			if (feat) selectEl.value = feat.status;
			return;
		}
		const ownerId = feat.owner?.id ?? null;
		if ((nextStatus === 'in_progress' || nextStatus === 'live') && !ownerId) {
			selectEl.value = feat.status;
			toast.error('Assign an accountable owner before moving this feature to In progress or Live.');
			return;
		}
		statusBusy = true;
		try {
			await featuresApi.setStatus(frame.organizationId, frame.projectId, feat.id, {
				status: nextStatus,
				ownerId,
				expectedRevision: feat.revision
			});
			toast.success(`${featureRef(feat)} marked ${FEATURE_STATUS_WORD[nextStatus]}.`);
			await featureRead.reload();
		} catch (e) {
			selectEl.value = feat.status;
			toast.error(toAppError(e).message);
		} finally {
			statusBusy = false;
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
									onchange={(e) => {
										const el = e.currentTarget as HTMLSelectElement;
										void changeStatus(el.value as Exclude<FeatureStatus, 'retired'>, el);
									}}
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

			<!-- Main grid: Definition + Tasks + Affected Bugs | Side column -->
			<div
				class="grid grid-cols-1 gap-8 py-6 min-[1024px]:grid-cols-[minmax(0,2fr)_minmax(0,1fr)] {gutters}"
			>
				<div class="flex min-w-0 flex-col gap-8">
					<FeatureDefinitionSection
						{uid}
						feature={f}
						owners={ov.owners}
						{editing}
						{editBusy}
						{editError}
						{conflictFeature}
						bind:editTitle
						bind:editSlug
						bind:editOwnerId
						bind:editBody
						onSave={(rev) => void saveEdit(rev)}
						onLoadLatest={beginEdit}
						onCancel={() => {
							editing = false;
							conflictFeature = null;
							editError = undefined;
							clearEditQueryParam();
						}}
					/>

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
									onclick={() => void linkDialog?.openDialog()}
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

				<FeatureSideColumn {uid} projectId={frame.projectId} feature={f} {formatWhen} />
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

			{#if breakOpen}
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
			{/if}

			<FeatureLinkTaskDialog
				bind:this={linkDialog}
				{uid}
				organizationId={frame.organizationId}
				projectId={frame.projectId}
				projectName={frame.project.name}
				feature={f}
				onlinked={() => featureRead.reload()}
			/>
		{/if}
	{/snippet}
</Loaded>
