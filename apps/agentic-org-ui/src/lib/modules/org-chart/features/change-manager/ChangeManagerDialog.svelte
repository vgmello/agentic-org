<!--
  @component
  Change manager… (offices-and-teams-ux §7.8, REPORT-3, REPORT-4): active employees, people and agents, plus
  No explicit manager. The employee isn't offered; those who report to them, directly or below, are listed but
  disabled with "Reports to <name>". Saved against the record's revision. A refusal (the record changed, or the choice
  would now make a loop) keeps the choice, says what changed, and asks the caller to refresh the lines, after which
  the choices and the revision are the latest. Team leads are never changed here (REPORT-5). Mount it in a {#key}
  that changes on each opening.
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { errorClass, helpClass, inputClass, mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Dialog,
		DialogContent,
		DialogDescription,
		DialogFooter,
		DialogHeader,
		DialogTitle
	} from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { kindWord } from '$lib/modules/members';
	import { toAppError } from '$lib/modules/offices/office-api';
	import { orgChartApi } from '../../api';
	import BusyLabel from '../../components/BusyLabel.svelte';
	import { listText, type ManagerChoice } from '../../tree';
	import type { ChartEmployee } from '../../types';

	let {
		open = $bindable(false),
		organizationId,
		employee,
		choices,
		onCloseAutoFocus,
		onrefused,
		ondone
	}: {
		open: boolean;
		organizationId: string;
		/** The employee as last read: its revision is what a save is checked against. */
		employee: ChartEmployee;
		choices: ManagerChoice[];
		onCloseAutoFocus?: (event: Event) => void;
		/** HQ refused: re-read the chart; this dialog stays open with the choice kept. */
		onrefused: () => Promise<void>;
		/** HQ recorded the change. */
		ondone: (result: { name: string; manager?: string }) => void;
	} = $props();

	const ids = $props.id();
	const NONE = '';
	/** The choice starts at the current manager; after a refresh it is kept as typed (REPORT-4). */
	let choice = $state(untrack(() => employee.reportsToId ?? NONE));
	let query = $state('');
	let busy = $state(false);
	let error = $state<string | undefined>();

	const managerName = (id: string | undefined) =>
		choices.find((c) => c.employee.id === id)?.employee.name;
	const current = $derived(employee.reportsToId ?? NONE);
	const needle = $derived(query.trim().toLowerCase());
	const shown = $derived(
		needle
			? choices.filter(
					(c) =>
						c.employee.id === choice ||
						c.employee.name.toLowerCase().includes(needle) ||
						c.employee.handle.includes(needle.replace(/^@/, ''))
				)
			: choices
	);
	const leads = $derived(employee.leads.map((t) => t.name));

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		if (choice === current) {
			error = `That is already ${employee.name}'s manager. Choose another to change it.`;
			return;
		}
		busy = true;
		error = undefined;
		const managerId = choice === NONE ? null : choice;
		try {
			await orgChartApi.changeManager(organizationId, employee.id, managerId, employee.revision);
			open = false;
			ondone({ name: employee.name, manager: managerId ? managerName(managerId) : undefined });
		} catch (e) {
			error = toAppError(e).message;
			await onrefused();
		} finally {
			busy = false;
		}
	}

	const option =
		'flex cursor-pointer items-start gap-x-3 px-3 py-2 text-sm/6 has-disabled:cursor-default has-focus-visible:outline-2 has-focus-visible:-outline-offset-2 has-focus-visible:outline-focus max-[639px]:min-h-11 max-[639px]:items-center';
	const radio = 'mt-1 size-4 shrink-0 accent-primary focus-visible:outline-none max-[639px]:mt-0';
</script>

<Dialog bind:open onOpenChange={(next) => !next && busy && (open = true)}>
	<DialogContent
		{onCloseAutoFocus}
		class="max-h-[calc(100dvh-2rem)] grid-cols-[minmax(0,1fr)] overflow-y-auto max-[639px]:max-w-[calc(100%-1rem)] max-[639px]:p-4"
	>
		<form class="grid gap-4" onsubmit={save} novalidate>
			<DialogHeader class="pr-8">
				<DialogTitle text="Change {employee.name}'s manager" />
				<DialogDescription>
					Reporting lines are explicit. This changes who {employee.name} reports to and nothing else{leads.length
						? `: ${employee.name} still leads ${listText(leads)}, which is a separate responsibility`
						: ''}.
				</DialogDescription>
			</DialogHeader>

			{#if choices.length > 8}
				<div>
					<Label for="{ids}-find">Find</Label>
					<Input
						id="{ids}-find"
						type="search"
						bind:value={query}
						autocomplete="off"
						class="mt-2 {inputClass}"
						aria-describedby="{ids}-find-help"
					/>
					<p id="{ids}-find-help" class={helpClass}>By name or @handle.</p>
				</div>
			{/if}

			<fieldset disabled={busy} aria-describedby={error ? `${ids}-error` : undefined}>
				<legend class="text-sm/6 font-medium text-fg">Reports to</legend>
				<ul role="list" class="mt-2 divide-y divide-line-subtle rounded-md ring-1 ring-line">
					<li>
						<label class={option}>
							<input
								type="radio"
								name="{ids}-manager"
								value={NONE}
								bind:group={choice}
								class={radio}
							/>
							<span class="min-w-0">
								<span class="block text-fg">No explicit manager</span>
								<span class="block text-xs/5 text-fg-muted">Shown under the owner bar</span>
							</span>
						</label>
					</li>
					{#each shown as c (c.employee.id)}
						<li>
							<label class={option}>
								<input
									type="radio"
									name="{ids}-manager"
									value={c.employee.id}
									bind:group={choice}
									disabled={!!c.disabled}
									aria-describedby={c.disabled ? `${ids}-why-${c.employee.id}` : undefined}
									class={radio}
								/>
								<span class="min-w-0">
									<span class="block text-fg [overflow-wrap:anywhere]"
										>{c.employee.name}{c.employee.id === current ? ' (current)' : ''}</span
									>
									<span class="block text-xs/5 text-fg-muted [overflow-wrap:anywhere]"
										>{kindWord(c.employee.kind)} · @{c.employee.handle}{c.employee.homeTeam
											? ` · ${c.employee.homeTeam.name}`
											: ''}</span
									>
									{#if c.disabled}<span
											id="{ids}-why-{c.employee.id}"
											class="block text-xs/5 text-fg-muted">{c.disabled}</span
										>{/if}
								</span>
							</label>
						</li>
					{/each}
				</ul>
				{#if needle && shown.length === 0}
					<p class={helpClass} role="status">Nobody matches “{query.trim()}”.</p>
				{/if}
			</fieldset>

			{#if error}
				<p id="{ids}-error" class="{errorClass} text-sm/6 [overflow-wrap:anywhere]" role="alert">
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
				<Button type="submit" class={mobileButton} aria-busy={busy}
					><BusyLabel {busy} idle="Save manager" working="Saving…" /></Button
				>
			</DialogFooter>
		</form>
	</DialogContent>
</Dialog>
