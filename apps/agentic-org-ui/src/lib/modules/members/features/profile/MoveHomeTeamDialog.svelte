<!--
  @component
  Move home team… from the profile (our-org-in-the-app §8, TEAM-6): one required destination, then a preview of what
  changes before anything is sent ("Nadia moves from App to Office. Current project and task assignments stay as
  recorded.", and a departing lead's team left without a lead). One operation, no teamless step. A refusal keeps the
  choice and says nothing changed. The current home team and teams they are already in can't be chosen, and say
  why. Mount it in a {#key} that changes on each opening.
-->
<script lang="ts">
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { toAppError } from '$lib/proto';
	import { teamOptions } from '$lib/modules/organization/team-edit';
	import type { OrgSnapshot } from '$lib/proto';
	import { membersApi } from '../../api';
	import { moveBlock, movePreview } from './move';

	let {
		open = $bindable(false),
		organizationId,
		member,
		fromTeamId,
		snapshot,
		onCloseAutoFocus,
		ondone
	}: {
		open: boolean;
		organizationId: string;
		member: { id: string; name: string };
		/** The current home team. */
		fromTeamId: string;
		/** The team tree and members, for the choices and what blocks one. */
		snapshot: OrgSnapshot;
		onCloseAutoFocus?: (event: Event) => void;
		/** Moved, or HQ refused (`stale`): the profile reloads either way. */
		ondone: (result: { name: string; to: string } | { stale: true }) => void;
	} = $props();

	const ids = $props.id();
	let target = $state<string | undefined>();
	let error = $state<string | undefined>();
	let busy = $state(false);

	const options = $derived(teamOptions(snapshot));
	const teamName = (id: string | undefined) => snapshot.teams.find((t) => t.id === id)?.name ?? '';
	const preview = $derived(target ? movePreview(snapshot, member, fromTeamId, target) : []);

	function changeOpen(next: boolean) {
		if (!next && busy) open = true;
	}

	async function move(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		if (!target) {
			error = 'Choose the team to move to.';
			return;
		}
		busy = true;
		error = undefined;
		try {
			await membersApi.moveHomeTeam(organizationId, member.id, target);
			open = false;
			ondone({ name: member.name, to: teamName(target) });
		} catch (e) {
			error = `Couldn't move ${member.name}: ${toAppError(e).message} Nothing changed.`;
			ondone({ stale: true });
		} finally {
			busy = false;
		}
	}
</script>

<Dialog bind:open onOpenChange={changeOpen}>
	<DialogContent
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4"
	>
		<form class="grid gap-4" onsubmit={move} novalidate>
			<DialogHeader class="pr-8">
				<DialogTitle text="Move {member.name}'s home team" />
				<DialogDescription
					>{member.name} is in {teamName(fromTeamId)} now. An employee has exactly one home team.</DialogDescription
				>
			</DialogHeader>

			<fieldset disabled={busy} aria-describedby={error ? `${ids}-error` : undefined}>
				<legend class="text-sm/6 font-medium text-fg">Move to</legend>
				<ul role="list" class="mt-2 divide-y divide-line-subtle rounded-md ring-1 ring-line">
					{#each options as option (option.team.id)}
						{@const blocked = moveBlock(snapshot, member, fromTeamId, option.team.id)}
						<li>
							<label
								class="flex cursor-pointer items-start gap-x-3 py-2 pr-3 text-sm/6 has-disabled:cursor-default has-focus-visible:outline-2 has-focus-visible:-outline-offset-2 has-focus-visible:outline-focus max-[639px]:min-h-11 max-[639px]:items-center"
								style:padding-left="{0.75 + option.depth * 1.25}rem"
							>
								<input
									type="radio"
									name="{ids}-target"
									value={option.team.id}
									bind:group={target}
									disabled={!!blocked}
									class="mt-1 size-4 shrink-0 accent-primary focus-visible:outline-none max-[639px]:mt-0"
								/>
								<span class="min-w-0">
									<span class="block text-fg [overflow-wrap:anywhere]">{option.team.name}</span>
									{#if blocked}<span class="block text-xs/5 text-fg-muted">{blocked}</span>{/if}
								</span>
							</label>
						</li>
					{/each}
				</ul>
			</fieldset>

			{#if preview.length}
				<div
					class="grid gap-1 text-sm/6 text-fg-secondary [overflow-wrap:anywhere]"
					data-testid="move-member-summary"
				>
					{#each preview as line (line)}<p>{line}</p>{/each}
				</div>
			{/if}
			{#if error}
				<p
					id="{ids}-error"
					class="text-sm/6 text-danger-text [overflow-wrap:anywhere]"
					role="alert"
				>
					{error}
				</p>
			{/if}

			<DialogFooter>
				<Button
					type="button"
					variant="outline"
					class={mobileButton}
					disabled={busy}
					onclick={() => (open = false)}>Cancel</Button
				>
				<Button type="submit" class="{mobileButton} [overflow-wrap:anywhere]" aria-busy={busy}
					>{busy ? 'Moving…' : `Move ${member.name}`}</Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
