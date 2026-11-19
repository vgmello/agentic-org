<!--
  @component
  Profile › Role (roles-ux §1): the referenced role with its revision and body, or, with no role referenced, "No role
  on record. Title: <title>." and the free-text role as written. Choosing a role is done in Agent setup.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import Loaded from '$lib/components/Loaded.svelte';
	import { membersApi } from '../../api';
	import { profileContext } from '../profile/context.svelte';
	import { roleBlocks } from '../role/role-body';
	import { tabResource } from './reload.svelte';

	const ctx = profileContext();
	const role = tabResource((o, m) => membersApi.role(o, m));
	const linkClass =
		'rounded-sm text-sm/6 font-semibold text-link hover:text-brand-text focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-focus';
</script>

<Loaded resource={role} label="role" error="Couldn't load {ctx.profile.name}'s role" rows={3}>
	{#snippet children(view)}
		<div class="px-4 py-6 min-[640px]:px-6 min-[1024px]:px-8">
			{#if view.role}
				<h3 class="flex flex-wrap items-baseline gap-x-2 text-base/7 font-semibold text-fg">
					{view.role.name}<span class="text-sm/6 font-normal text-fg-muted"
						>rev {view.role.revision}</span
					>
				</h3>
				<div class="mt-4 max-w-3xl space-y-3 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
					{#each roleBlocks(view.role.body) as block, i (i)}
						{#if block.kind === 'heading'}
							<h4 class="pt-2 text-sm/6 font-semibold text-fg">{block.text}</h4>
						{:else if block.kind === 'list'}
							<ul role="list" class="list-disc space-y-1 pl-5 marker:text-fg-subtle">
								{#each block.items as item, j (j)}<li>{item}</li>{/each}
							</ul>
						{:else}
							<p>{block.text}</p>
						{/if}
					{/each}
				</div>
			{:else}
				<h3 class="text-base/7 font-semibold text-fg">
					No role on record.{view.title ? ` Title: ${view.title}.` : ''}
				</h3>
				{#if view.roleText}
					<p class="mt-1 max-w-3xl text-sm/6 text-fg-secondary [overflow-wrap:anywhere]">
						Role as written: {view.roleText}
					</p>
				{/if}
				<p class="mt-1 max-w-3xl text-sm/6 text-fg-muted">
					A role describes what an employee is for. It grants no authority.
				</p>
			{/if}
			<p class="mt-4">
				<a href={resolve('/settings/agents')} class={linkClass}
					>{view.role ? 'Change the role in Agent setup' : 'Choose a role in Agent setup'}</a
				>
			</p>
		</div>
	{/snippet}
</Loaded>
