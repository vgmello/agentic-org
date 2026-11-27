<!--
  @component
  Release detail page (`/projects/[id]/releases/[releaseId]`, features-ux §2, organization-data 07 & 09):
  Displays the release manifest (R- reference, version label, status, canonical path, purpose),
  product changelog & supporting docs, milestones (M-), explicit included features and tasks,
  intended feature flag settings (`flag-settings.yaml`), and actual deployment attempts with
  commit snapshots, confirmed applied flag snapshots, and technical changelogs.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import Avatar from '$lib/components/Avatar.svelte';
	import Callout from '$lib/components/Callout.svelte';
	import StatusPill from '$lib/components/StatusPill.svelte';
	import { Resource } from '$lib/components/resource.svelte';
	import { gutters } from '$lib/components/classes';
	import { tone } from '$lib/components/pills';
	import { accountZone } from '$lib/modules/preferences/account-zone.svelte';
	import { formatDate } from '$lib/modules/preferences/time';
	import { pageTitle } from '$lib/nav';
	import { toAppError } from '$lib/proto';
	import { projectFrame } from '$lib/platform/shell/project-frame';
	import { releasesApi } from '../../api';
	import {
		DEPLOYMENT_OP_WORD,
		DEPLOYMENT_STATUS_TONE,
		DEPLOYMENT_STATUS_WORD,
		MILESTONE_STATUS_WORD,
		RELEASE_STATUS_TONE,
		RELEASE_STATUS_WORD,
		milestoneProgressText,
		milestoneRef,
		releaseRef,
		releasesHref,
		shortSha
	} from '../../present';
	import type { ReleaseDetail } from '../../types';

	let { releaseId }: { releaseId: string } = $props();

	const frame = projectFrame();
	const uid = $props.id();

	type ReleaseLookup = { found: true; release: ReleaseDetail } | { found: false; message: string };

	async function loadRelease(): Promise<ReleaseLookup> {
		try {
			const release = await releasesApi.get(frame.organizationId, frame.projectId, releaseId);
			return { found: true, release };
		} catch (e) {
			const err = toAppError(e);
			if (err.source === 'api' && err.code === 'not_found') {
				return { found: false, message: err.message };
			}
			throw e;
		}
	}

	const releaseRead = new Resource<ReleaseLookup>(() => loadRelease());
	const rel = $derived(releaseRead.data?.found ? releaseRead.data.release : undefined);

	const title = $derived(
		rel
			? pageTitle(`${releaseRef(rel)} ${rel.title}`, 'Releases', frame.project.name)
			: pageTitle('Release', 'Releases', frame.project.name)
	);

	function formatWhen(iso: string): string {
		const date = new Date(iso);
		if (Number.isNaN(date.getTime())) return iso;
		return formatDate(date, accountZone.zone, new Date());
	}

	function bodyLines(markdown: string): { kind: 'h' | 'p'; text: string }[] {
		return markdown
			.split(/\r?\n/)
			.map((l) => l.trim())
			.filter(Boolean)
			.map((l) => {
				const h = /^#{1,6}\s+(.*)$/.exec(l);
				return h ? { kind: 'h' as const, text: h[1] } : { kind: 'p' as const, text: l };
			});
	}
</script>

<svelte:head><title>{title}</title></svelte:head>

<div class="border-b border-line-subtle py-3 {gutters}">
	<a
		href={resolve(releasesHref(frame.projectId) as '/')}
		class="inline-flex min-h-6 items-center rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus max-[639px]:min-h-11"
	>
		Releases
	</a>
</div>

<Loaded
	resource={releaseRead}
	label="the release"
	error="Couldn't load this release"
	errorLevel={4}
	rows={5}
>
	{#snippet children(res)}
		{#if !res.found}
			<div class="py-8 {gutters}">
				<h3 class="text-base/7 font-semibold text-fg">Release not found</h3>
				<p class="mt-1 max-w-prose text-sm/6 text-fg-muted">{res.message}</p>
			</div>
		{:else}
			{@const r = res.release}

			<!-- Header card -->
			<div class="border-b border-line-subtle py-6 {gutters}" data-testid="release-header">
				<div class="flex flex-wrap items-center gap-2">
					<span class="font-mono text-xs/5 font-semibold text-fg-muted">{releaseRef(r)}</span>
					{#if r.versionLabel}
						<span class="font-mono text-xs/5 text-fg-secondary">{r.versionLabel}</span>
					{/if}
					<StatusPill
						label={RELEASE_STATUS_WORD[r.status]}
						class={tone[RELEASE_STATUS_TONE[r.status]]}
					/>
				</div>
				<h3 class="mt-2 text-lg/7 font-semibold text-fg [overflow-wrap:anywhere]">{r.title}</h3>
				<p class="mt-2 flex flex-wrap items-center gap-x-5 gap-y-1 text-xs/5 text-fg-muted">
					<span class="inline-flex items-center gap-1.5">
						{#if r.owner}
							<Avatar name={r.owner.name} size="sm" />
							<span>Owner: <span class="text-fg-secondary">{r.owner.name}</span></span>
						{:else}
							<span>Owner: Unassigned</span>
						{/if}
					</span>
					<span>
						Updated <time datetime={r.updatedAt}>{formatWhen(r.updatedAt)}</time> by {r.updatedBy
							.name} · rev {r.revision}
					</span>
				</p>
				<p class="mt-1 font-mono text-xs/5 text-fg-muted [overflow-wrap:anywhere]">{r.path}</p>

				<div class="mt-4">
					<Callout>
						<p class="text-xs/5">
							Release manifest membership and intended flag settings record product intent; actual
							environment availability comes only from the confirmed deployment attempts below.
						</p>
					</Callout>
				</div>
			</div>

			<div
				class="grid grid-cols-1 gap-8 py-6 min-[1024px]:grid-cols-[minmax(0,2fr)_minmax(0,1fr)] {gutters}"
			>
				<div class="flex min-w-0 flex-col gap-8">
					<!-- Release notes body -->
					{#if r.bodyMarkdown.trim()}
						<section aria-labelledby="{uid}-notes-h">
							<h4 id="{uid}-notes-h" class="text-sm/6 font-semibold text-fg">Release overview</h4>
							<div
								class="mt-2 flex max-w-prose flex-col gap-2 text-sm/6 text-fg-body [overflow-wrap:anywhere]"
							>
								{#each bodyLines(r.bodyMarkdown) as line, idx (`${idx}-${line.kind}`)}
									{#if line.kind === 'h'}
										<h5 class="pt-2 text-sm/6 font-semibold text-fg first:pt-0">{line.text}</h5>
									{:else}
										<p>{line.text}</p>
									{/if}
								{/each}
							</div>
						</section>
					{/if}

					<!-- Explicit release content: features and tasks -->
					<section
						aria-labelledby="{uid}-content-h"
						class="border-t border-line-subtle pt-6"
						data-testid="release-manifest-content"
					>
						<h4 id="{uid}-content-h" class="text-sm/6 font-semibold text-fg">
							Explicit release contents
						</h4>

						<div class="mt-4 grid grid-cols-1 gap-6 min-[640px]:grid-cols-2">
							<div>
								<h5 class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase">
									Included features · {r.features.length}
								</h5>
								{#if r.features.length === 0}
									<p class="mt-2 text-xs/5 text-fg-muted">No features explicitly selected.</p>
								{:else}
									<ul
										role="list"
										class="mt-2 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
									>
										{#each r.features as f (f.id)}
											<li class="flex items-baseline justify-between gap-2 px-3 py-2.5 text-sm/6">
												<a
													href={resolve(
														`/projects/${encodeURIComponent(frame.projectId)}/features/${encodeURIComponent(f.id)}` as '/'
													)}
													class="font-medium text-link hover:underline [overflow-wrap:anywhere]"
												>
													<span class="mr-1 font-mono text-xs/5">{f.ref}</span>
													{f.title}
												</a>
												<span class="shrink-0 text-xs/5 text-fg-muted">{f.status}</span>
											</li>
										{/each}
									</ul>
								{/if}
							</div>

							<div>
								<h5 class="text-xs/5 font-semibold tracking-wide text-fg-muted uppercase">
									Included tasks · {r.tasks.length}
								</h5>
								{#if r.tasks.length === 0}
									<p class="mt-2 text-xs/5 text-fg-muted">No tasks explicitly selected.</p>
								{:else}
									<ul
										role="list"
										class="mt-2 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
									>
										{#each r.tasks as t (t.id)}
											<li class="flex items-baseline justify-between gap-2 px-3 py-2.5 text-sm/6">
												<a
													href={resolve(`/work/${encodeURIComponent(t.ref)}` as '/')}
													class="font-medium text-link hover:underline [overflow-wrap:anywhere]"
												>
													<span class="mr-1 font-mono text-xs/5">{t.ref}</span>
													{t.title}
												</a>
												<span class="shrink-0 text-xs/5 text-fg-muted">
													{t.milestoneRef ? `${t.milestoneRef} · ` : ''}{t.status}{t.archived
														? ' (archived)'
														: ''}
												</span>
											</li>
										{/each}
									</ul>
								{/if}
							</div>
						</div>
					</section>

					<!-- Intended flag settings (flag-settings.yaml) -->
					<section
						aria-labelledby="{uid}-flags-h"
						class="border-t border-line-subtle pt-6"
						data-testid="release-intended-flags"
					>
						<h4 id="{uid}-flags-h" class="text-sm/6 font-semibold text-fg">
							Intended feature flag settings ({r.flagSettings.length})
						</h4>
						<p class="mt-0.5 text-xs/5 text-fg-muted">
							Release intent from <span class="font-mono">flag-settings.yaml</span>. Shipping code
							does not enable a flag; confirmed applied values live on each deployment attempt.
						</p>
						{#if r.flagSettings.length === 0}
							<p class="mt-2 text-sm/6 text-fg-muted">
								No flag settings specified for this release.
							</p>
						{:else}
							<ul
								role="list"
								class="mt-3 divide-y divide-line-subtle rounded-lg bg-surface-raised ring-1 ring-line"
							>
								{#each r.flagSettings as flg (`${flg.flagId}-${flg.environment}-${flg.target}`)}
									<li
										class="flex flex-wrap items-baseline justify-between gap-2 px-4 py-3 text-sm/6"
									>
										<div>
											<span class="font-mono font-semibold text-fg">{flg.key}</span>
											<span class="ml-2 text-xs/5 text-fg-muted">
												{flg.environment}/{flg.target}
												{#if flg.featureRef}· {flg.featureRef}{/if}
											</span>
											{#if flg.rolloutSummary}
												<p class="text-xs/5 text-fg-secondary">{flg.rolloutSummary}</p>
											{/if}
										</div>
										<span class="font-mono text-xs/5 text-fg-bright">
											Intended: {flg.intendedValue}
										</span>
									</li>
								{/each}
							</ul>
						{/if}
					</section>

					<!-- Deployment attempts (organization-data 07 & 09) -->
					<section
						aria-labelledby="{uid}-deploys-h"
						class="border-t border-line-subtle pt-6"
						data-testid="release-deployments"
					>
						<h4 id="{uid}-deploys-h" class="text-sm/6 font-semibold text-fg">
							Deployment attempts ({r.deployments.length})
						</h4>
						{#if r.deployments.length === 0}
							<p class="mt-2 text-sm/6 text-fg-muted">No deployment attempts recorded yet.</p>
						{:else}
							<ul role="list" class="mt-3 flex flex-col gap-4">
								{#each r.deployments as dep (dep.id)}
									<li
										class="flex flex-col gap-3 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
										data-testid="deployment-attempt"
									>
										<div class="flex flex-wrap items-center justify-between gap-2">
											<div class="flex flex-wrap items-center gap-2">
												<span class="text-sm/6 font-semibold text-fg">
													{DEPLOYMENT_OP_WORD[dep.operation]} · {dep.environment}/{dep.target}
												</span>
												<StatusPill
													label={DEPLOYMENT_STATUS_WORD[dep.status]}
													class={tone[DEPLOYMENT_STATUS_TONE[dep.status]]}
												/>
											</div>
											<span class="text-xs/5 text-fg-muted">
												{dep.actor.name}
												{#if dep.finishedAt}
													· Finished <time datetime={dep.finishedAt}
														>{formatWhen(dep.finishedAt)}</time
													>
												{:else if dep.startedAt}
													· Started <time datetime={dep.startedAt}>{formatWhen(dep.startedAt)}</time
													>
												{/if}
											</span>
										</div>

										{#if dep.summary}
											<p class="text-sm/6 text-fg-body [overflow-wrap:anywhere]">{dep.summary}</p>
										{/if}

										{#if dep.repositoryCommits.length > 0}
											<div>
												<p class="text-xs/5 font-medium text-fg-muted">Commit snapshot</p>
												<ul role="list" class="mt-1 flex flex-wrap gap-2">
													{#each dep.repositoryCommits as rc (rc.repositoryKey)}
														<li
															class="rounded bg-surface-sunken px-2 py-0.5 font-mono text-xs/5 text-fg-secondary ring-1 ring-line"
														>
															{rc.repositoryKey}: {shortSha(rc.commitSha)}
														</li>
													{/each}
												</ul>
											</div>
										{/if}

										{#if dep.appliedFlags.length > 0}
											<div>
												<p class="text-xs/5 font-medium text-fg-muted">
													Confirmed applied flags (snapshot)
												</p>
												<ul
													role="list"
													class="mt-1 flex flex-col gap-1 text-xs/5 text-fg-secondary"
												>
													{#each dep.appliedFlags as af (af.flagId)}
														<li>
															<span class="font-mono text-fg">{af.key}</span> =
															<span class="font-mono">{af.appliedValue}</span>
															{#if af.rolloutSummary}
																({af.rolloutSummary})
															{/if}
														</li>
													{/each}
												</ul>
											</div>
										{/if}

										{#if dep.technicalChangelogDocId}
											<div>
												<a
													href={resolve(
														`/projects/${encodeURIComponent(frame.projectId)}/docs/${encodeURIComponent(dep.technicalChangelogDocId)}` as '/'
													)}
													class="text-xs/5 font-semibold text-link hover:underline"
												>
													{dep.technicalChangelogTitle ?? 'Technical changelog'}
												</a>
											</div>
										{/if}
									</li>
								{/each}
							</ul>
						{/if}
					</section>
				</div>

				<!-- Side column: Product changelog, Supporting docs, Milestones -->
				<aside aria-label="Release milestones and documents" class="flex flex-col gap-5">
					<section
						aria-labelledby="{uid}-changelog-h"
						class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
						data-testid="release-product-changelog"
					>
						<h4 id="{uid}-changelog-h" class="text-sm/6 font-semibold text-fg">
							Product changelog & docs
						</h4>
						{#if r.productChangelogDocId}
							<a
								href={resolve(
									`/projects/${encodeURIComponent(frame.projectId)}/docs/${encodeURIComponent(r.productChangelogDocId)}` as '/'
								)}
								class="text-sm/6 font-semibold text-link hover:underline"
							>
								{r.productChangelogTitle ?? `${releaseRef(r)} product changelog`}
							</a>
						{:else}
							<p class="text-xs/5 text-fg-muted">No product changelog written yet.</p>
						{/if}

						{#if r.supportingDocs.length > 0}
							<ul
								role="list"
								class="mt-2 flex flex-col gap-1.5 border-t border-line-subtle pt-2 text-sm/6"
							>
								{#each r.supportingDocs as doc (doc.id)}
									<li class="flex items-baseline justify-between gap-2">
										<a
											href={resolve(
												`/projects/${encodeURIComponent(frame.projectId)}/docs/${encodeURIComponent(doc.id)}` as '/'
											)}
											class="text-link hover:underline [overflow-wrap:anywhere]"
										>
											{doc.title}
										</a>
										<span class="shrink-0 text-xs/5 text-fg-muted">{doc.status}</span>
									</li>
								{/each}
							</ul>
						{/if}
					</section>

					<section
						aria-labelledby="{uid}-milestones-h"
						class="flex flex-col gap-2 rounded-xl bg-surface-raised p-4 ring-1 ring-line"
						data-testid="release-milestones"
					>
						<h4 id="{uid}-milestones-h" class="text-sm/6 font-semibold text-fg">
							Milestones ({r.milestones.length})
						</h4>
						{#if r.milestones.length === 0}
							<p class="text-xs/5 text-fg-muted">No milestones in this release.</p>
						{:else}
							<ul role="list" class="flex flex-col gap-3">
								{#each r.milestones as ms (ms.id)}
									<li class="border-b border-line-subtle pb-2.5 last:border-b-0 last:pb-0">
										<div class="flex items-baseline justify-between gap-2">
											<p class="text-sm/6 font-medium text-fg">
												<span class="mr-1 font-mono text-xs/5 text-fg-muted"
													>{milestoneRef(ms)}</span
												>
												{ms.title}
											</p>
											<span class="shrink-0 text-xs/5 text-fg-muted">
												{MILESTONE_STATUS_WORD[ms.status]}
											</span>
										</div>
										<p class="mt-0.5 text-xs/5 text-fg-muted">{milestoneProgressText(ms)}</p>
									</li>
								{/each}
							</ul>
						{/if}
					</section>
				</aside>
			</div>
		{/if}
	{/snippet}
</Loaded>
