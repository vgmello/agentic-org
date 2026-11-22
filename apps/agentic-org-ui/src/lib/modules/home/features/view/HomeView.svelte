<!--
  @component
  Home (our-org-in-the-app §6.1): Needs you (the first five open Inbox items and Open Inbox (N)), Projects,
  Happening now, Recent activity, in that order, and the month line. Every section has its own skeleton, its
  honest empty state and a one-line error with Retry. Home has no page primary: its actions are outline.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import Avatar from '$lib/components/Avatar.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { buttonVariants } from '$lib/components/ui/button';
	import {
		ActivityRowItem,
		activityApi,
		matchesQuery,
		type ActivityRow
	} from '$lib/modules/activity';
	import { InboxListRow, inboxApi, openCount, openItems, type InboxItem } from '$lib/modules/inbox';
	import { accountZone, durationWords, formatDate } from '$lib/modules/preferences';
	import { newProjectUrl } from '$lib/modules/projects';
	import { homeApi } from '../../api';
	import { Loader } from '../../loader.svelte';
	import HomeSection from '../section/HomeSection.svelte';

	let { organizationId, organizationName }: { organizationId: string; organizationName: string } =
		$props();

	const NEEDS_YOU = 5;
	const RECENT = 6;
	let now = $state(new Date());

	// svelte-ignore state_referenced_locally
	const id = organizationId;
	const inbox = new Loader<InboxItem[]>(() => inboxApi.list(id));
	const projects = new Loader(() => homeApi.projects(id));
	const working = new Loader(() => homeApi.workingNow(id));
	const month = new Loader(() => homeApi.month(id));
	const recent = new Loader(async () => (await activityApi.feed(id, { kind: 'all' }, RECENT)).rows);
	let live = $state.raw<ActivityRow[]>([]);

	const needs = $derived(openItems(inbox.value ?? []));
	const feed = $derived([...live, ...(recent.value ?? [])].slice(0, RECENT));

	onMount(() => {
		void accountZone.ensure(id);
		for (const section of [inbox, projects, working, month, recent]) void section.run();
		const tick = setInterval(() => (now = new Date()), 30_000);
		const stops: (() => void)[] = [];
		let gone = false;
		const keep = (p: Promise<() => void>) =>
			p.then((stop) => (gone ? stop() : stops.push(stop))).catch(() => undefined);
		// An item cleared anywhere leaves Needs you within moments; a new activity row slides in at the top.
		keep(inboxApi.subscribe(id, () => void inbox.run(false)));
		keep(
			activityApi.subscribe(id, (row) => {
				if (matchesQuery(row, { kind: 'all' }) && !live.some((r) => r.id === row.id))
					live = [row, ...live];
			})
		);
		return () => {
			gone = true;
			stops.forEach((stop) => stop());
			clearInterval(tick);
		};
	});

	const dueLabel = (date: string) => {
		const [y, m, d] = date.split('-').map(Number);
		return formatDate(new Date(Date.UTC(y, m - 1, d, 12)), 'UTC', now);
	};
	const outline = buttonVariants({ variant: 'outline' });
</script>

<PageHeader
	title="Home"
	description="What needs you and what is happening across {organizationName}."
/>

<div class="divide-y divide-line-subtle">
	<HomeSection
		title="Needs you"
		what="your Inbox"
		phase={inbox.phase}
		message={inbox.message}
		retrying={inbox.retrying}
		onretry={inbox.retry}
		unavailable="The Inbox isn't available yet."
	>
		{#snippet actions()}
			<a href={resolve('/inbox')} class="{outline} max-[639px]:min-h-11"
				>Open Inbox ({openCount(inbox.value ?? [])})</a
			>
		{/snippet}
		{#if needs.length === 0}
			<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8" data-testid="needs-empty">
				<p class="text-sm/6 font-semibold text-fg">Nothing needs you</p>
				<p class="mt-1 text-sm/6 text-fg-muted">
					When an agent needs your approval, an answer or a decision, it shows up here.
				</p>
			</div>
		{:else}
			<ul
				role="list"
				class="divide-y divide-line-subtle border-y border-line-subtle"
				data-testid="needs-list"
			>
				{#each needs.slice(0, NEEDS_YOU) as item (item.id)}
					<InboxListRow
						{item}
						{now}
						href="/inbox?item={encodeURIComponent(item.id)}"
						class="min-[1024px]:px-8"
					/>
				{/each}
			</ul>
			{#if needs.length > NEEDS_YOU}
				<p class="px-4 pt-3 text-xs/5 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8">
					{needs.length - NEEDS_YOU} more in the Inbox.
				</p>
			{/if}
		{/if}
	</HomeSection>

	<HomeSection
		title="Projects"
		what="projects"
		phase={projects.phase}
		message={projects.message}
		retrying={projects.retrying}
		onretry={projects.retry}
		skeleton={3}
	>
		{#snippet actions()}
			<a href={resolve('/projects')} class="{outline} max-[639px]:min-h-11">View all projects</a>
		{/snippet}
		{#if (projects.value ?? []).length === 0}
			<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8" data-testid="projects-empty">
				<p class="text-sm/6 font-semibold text-fg">No projects yet</p>
				<p class="mt-1 text-sm/6 text-fg-muted">Projects hold the work your teams do.</p>
				<a
					href={resolve(newProjectUrl() as '/')}
					class="{outline} mt-4 max-[639px]:min-h-11"
					data-testid="create-project">Create project</a
				>
			</div>
		{:else}
			<ul
				role="list"
				class="grid gap-4 px-4 min-[640px]:grid-cols-2 min-[640px]:px-6 min-[1024px]:px-8 min-[1280px]:grid-cols-3"
			>
				{#each projects.value ?? [] as project (project.id)}
					{@const pct = project.total > 0 ? Math.round((project.done / project.total) * 100) : 0}
					<li
						class="relative rounded-xl bg-surface-raised p-4 outline-1 -outline-offset-1 outline-line hover:bg-tint/[0.07]"
						data-testid="project-card"
						data-project={project.id}
					>
						<h4 class="text-sm/6 font-semibold text-fg [overflow-wrap:anywhere]">
							<a
								href={resolve(`/projects/${project.id}` as '/')}
								class="rounded-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
								><span class="absolute inset-0" aria-hidden="true"></span>{project.name}</a
							>
						</h4>
						{#if project.total === 0}
							<p class="mt-3 text-sm/6 text-fg-muted">No work yet</p>
						{:else}
							<div
								class="mt-3"
								role="progressbar"
								aria-label="{project.name}: {project.done} of {project.total} done"
								aria-valuemin={0}
								aria-valuemax={project.total}
								aria-valuenow={project.done}
							>
								<div class="h-1.5 overflow-hidden rounded-full bg-bar-track">
									<div class="h-full rounded-full bg-primary" style:width="{pct}%"></div>
								</div>
							</div>
							<p class="mt-2 text-sm/6 text-fg-body">{project.done} of {project.total} done</p>
							<p class="text-xs/5 text-fg-muted">{project.open} open · {project.blocked} blocked</p>
						{/if}
						<p class="mt-2 text-xs/5 text-fg-muted">
							{[
								`${project.members} ${project.members === 1 ? 'member' : 'members'}`,
								project.lead ? `Lead ${project.lead.name}` : '',
								project.due ? `Due ${dueLabel(project.due)}` : ''
							]
								.filter(Boolean)
								.join(' · ')}
						</p>
					</li>
				{/each}
			</ul>
		{/if}
	</HomeSection>

	<HomeSection
		title="Happening now"
		what="who is working"
		phase={working.phase}
		message={working.message}
		retrying={working.retrying}
		onretry={working.retry}
		unavailable="Live status isn't available yet."
	>
		{#if working.value && !working.value.live}
			<p
				class="px-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
				data-testid="presence-unavailable"
			>
				Live status isn't available yet.
			</p>
		{:else if (working.value?.agents ?? []).length === 0}
			<p
				class="px-4 text-sm/6 text-fg-muted min-[640px]:px-6 min-[1024px]:px-8"
				data-testid="nobody-working"
			>
				No one is working right now.
			</p>
		{:else}
			<ul
				role="list"
				class="divide-y divide-line-subtle border-y border-line-subtle"
				data-testid="working-list"
			>
				{#each working.value?.agents ?? [] as agent (agent.member.id)}
					<li class="flex items-center gap-x-3 px-4 py-3 min-[640px]:px-6 min-[1024px]:px-8">
						<Avatar name={agent.member.name} size="sm" />
						<p class="min-w-0 flex-1 text-sm/6 text-fg-body [overflow-wrap:anywhere]">
							<a
								href={resolve(`/members/${agent.member.id}` as '/')}
								class="rounded-sm font-semibold text-fg hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
								>{agent.member.name}</a
							>
							{#if agent.task}
								<span class="text-fg-muted">on</span>
								<a
									href={resolve(`/work/${agent.task.ref}` as '/')}
									class="rounded-sm text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
									><span class="font-mono text-xs/5">{agent.task.ref}</span> {agent.task.title}</a
								>
							{:else}
								<span class="text-fg-muted">no task reported</span>
							{/if}
						</p>
						<p class="shrink-0 text-xs/5 text-fg-muted">
							for {durationWords(new Date(agent.since), now)}
						</p>
					</li>
				{/each}
			</ul>
		{/if}
	</HomeSection>

	<HomeSection
		title="Recent activity"
		what="recent activity"
		phase={recent.phase}
		message={recent.message}
		retrying={recent.retrying}
		onretry={recent.retry}
		skeleton={4}
	>
		{#snippet actions()}
			<a href={resolve('/activity')} class="{outline} max-[639px]:min-h-11">View all activity</a>
		{/snippet}
		{#if feed.length === 0}
			<div class="px-4 min-[640px]:px-6 min-[1024px]:px-8" data-testid="activity-empty">
				<p class="text-sm/6 font-semibold text-fg">No activity yet</p>
				<p class="mt-1 text-sm/6 text-fg-muted">
					When your agents start working, everything they do shows up here.
				</p>
			</div>
		{:else}
			<ul
				role="list"
				class="divide-y divide-line-subtle border-y border-line-subtle"
				data-testid="recent-list"
			>
				{#each feed as row (row.id)}
					<ActivityRowItem
						{row}
						{now}
						fresh={live.some((r) => r.id === row.id) &&
							now.getTime() - new Date(row.at).getTime() < 2000}
					/>
				{/each}
			</ul>
		{/if}
	</HomeSection>

	{#if month.phase === 'ready' && month.value}
		<p
			class="px-4 py-6 text-sm/6 text-fg-secondary min-[640px]:px-6 min-[1024px]:px-8"
			data-testid="month-line"
		>
			This month: ≈ ${month.value.cost.toLocaleString('en-US')} · {month.value.tasksDone} tasks done ·
			{month.value.waitingHours} h waiting on you
			<a
				href={resolve('/insights')}
				class="ml-2 rounded-sm font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus"
				>Insights</a
			>
		</p>
	{/if}
</div>
