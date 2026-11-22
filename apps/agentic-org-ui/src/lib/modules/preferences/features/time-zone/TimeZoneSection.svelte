<!--
  @component
  Your preferences › Time zone (app-shell-ux "Account time display"): the zone this account's times are shown
  in. A searchable list of IANA zones, each with its city, its offset now and its local time. The saved zone
  wins; otherwise the browser's valid zone is offered; otherwise times show "unsaved UTC" and say how to fix it.
  A display preference only: it never changes budget periods or stored instants.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import Callout from '$lib/components/Callout.svelte';
	import Section from '$lib/components/Section.svelte';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { accountZone } from '../../account-zone.svelte';
	import {
		formatClock,
		formatFull,
		offsetLabel,
		searchZones,
		zoneOption,
		zoneOptions
	} from '../../time';

	let { organizationId }: { organizationId: string } = $props();

	const SHOWN = 40;
	let now = $state(new Date());
	onMount(() => {
		const tick = setInterval(() => (now = new Date()), 30_000);
		void accountZone.ensure(organizationId);
		return () => clearInterval(tick);
	});

	const options = $derived(zoneOptions(now));
	let query = $state('');
	// The zone chosen in the list; until the person picks one, the zone in use.
	let picked = $state<string | undefined>();
	const choice = $derived(picked ?? accountZone.zone);
	const matches = $derived(searchZones(options, query));
	const listed = $derived.by(() => {
		const first = matches.slice(0, SHOWN);
		// The chosen zone stays on the list while it matches, so the selection never vanishes from view.
		if (!first.some((o) => o.id === choice) && matches.some((o) => o.id === choice))
			return [zoneOption(choice, now), ...first.slice(0, SHOWN - 1)];
		return first;
	});
	const current = $derived(zoneOption(accountZone.zone, now));
	const unchanged = $derived(accountZone.status === 'saved' && choice === accountZone.zone);

	let saving = $state(false);
	let failure = $state<string | undefined>();
	let confirmed = $state<string | undefined>();

	async function save() {
		saving = true;
		failure = undefined;
		confirmed = undefined;
		try {
			await accountZone.save(organizationId, choice);
			picked = undefined;
			confirmed = choice;
		} catch (e) {
			failure = e instanceof Error ? e.message : 'The office did not say why.';
		} finally {
			saving = false;
		}
	}
</script>

<Section
	title="Time zone"
	description="Times in lists, headers and logs show in this zone, in 24-hour form. It changes only how times are shown, never when something happened."
>
	<div class="space-y-6 px-4 min-[640px]:px-6 min-[1024px]:px-8">
		<div data-testid="zone-status">
			{#if accountZone.status === 'unsaved-utc'}
				<Callout>
					<p class="text-sm/6 text-fg">
						Times show in <strong class="font-semibold">unsaved UTC</strong>.
					</p>
					<p class="text-sm/6 text-fg-muted">
						Time zone not detected. Choose yours below and save it.
					</p>
				</Callout>
			{:else}
				<p class="text-sm/6 text-fg-secondary">
					Showing times in
					<strong class="font-semibold text-fg [overflow-wrap:anywhere]"
						>{current.id.replaceAll('_', ' ')}</strong
					>{#if current.id !== 'UTC'}, {current.offset}{/if}. It is
					<time datetime={now.toISOString()} title={formatFull(now, accountZone.zone)}
						>{formatClock(now, accountZone.zone)}</time
					>
					there now.
					{#if accountZone.status === 'saved'}Saved for your account.{:else}Detected from this
						browser and not saved yet.{/if}
				</p>
			{/if}
		</div>

		<div class="max-w-xl">
			<Label for="zone-query" class="block text-sm/6 font-medium text-fg">Search time zones</Label>
			<Input
				id="zone-query"
				type="search"
				autocomplete="off"
				bind:value={query}
				aria-describedby="zone-query-help"
				class="mt-2 max-[639px]:h-11 border-0 bg-surface-raised px-3 py-1.5 text-base text-fg shadow-none outline-1 outline-solid -outline-offset-1 outline-field-border sm:text-sm/6 md:text-sm/6 dark:bg-surface-raised placeholder:text-fg-muted focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-focus"
			/>
			<p id="zone-query-help" class="mt-2 text-xs/5 text-fg-muted">
				A city, a region or an offset, for example Kolkata, Europe or +05:30
			</p>
		</div>

		<fieldset class="max-w-2xl">
			<legend class="text-sm/6 font-medium text-fg">Choose a time zone</legend>
			<p class="sr-only" role="status">
				{matches.length === 0
					? 'No time zones match.'
					: `${matches.length} ${matches.length === 1 ? 'time zone matches' : 'time zones match'}.`}
			</p>
			{#if matches.length === 0}
				<p class="mt-3 text-sm/6 text-fg-muted" data-testid="zone-none">
					No time zone matches “{query.trim()}”. Try a city, or an offset such as +01:00.
				</p>
			{:else}
				<ul
					role="list"
					tabindex="-1"
					class="mt-3 max-h-80 divide-y divide-line-subtle overflow-y-auto border-y border-line"
					data-testid="zone-list"
				>
					{#each listed as option (option.id)}
						<li>
							<label
								class="flex min-h-11 cursor-pointer items-center gap-x-3 px-2 py-2 hover:bg-surface-raised has-checked:bg-primary/10 has-focus-visible:outline-2 has-focus-visible:-outline-offset-2 has-focus-visible:outline-focus"
							>
								<input
									type="radio"
									name="time-zone"
									value={option.id}
									checked={choice === option.id}
									onchange={() => (picked = option.id)}
									class="size-4 shrink-0 accent-primary"
								/>
								<span class="min-w-0 flex-1 text-sm/6 text-fg [overflow-wrap:anywhere]">
									{option.city}
									{#if option.region}<span class="text-xs/5 text-fg-muted">{option.region}</span
										>{/if}
								</span>
								<span class="shrink-0 text-right text-xs/5 text-fg-muted">
									<span class="block">{option.offset}</span>
									<span class="block"><span class="sr-only">Local time </span>{option.local}</span>
								</span>
							</label>
						</li>
					{/each}
				</ul>
				{#if matches.length > SHOWN}
					<p class="mt-2 text-xs/5 text-fg-muted">
						Showing {SHOWN} of {matches.length}. Keep typing to narrow the list.
					</p>
				{/if}
			{/if}
		</fieldset>

		<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
			<Button
				onclick={save}
				disabled={saving || unchanged}
				aria-busy={saving}
				aria-describedby="zone-save-help"
				class="min-w-36 max-[639px]:min-h-11">{saving ? 'Saving…' : 'Save time zone'}</Button
			>
			<p id="zone-save-help" class="text-xs/5 text-fg-muted">
				{#if unchanged}This is your saved time zone.{:else}Will show times as {choice.replaceAll(
						'_',
						' '
					)}, {offsetLabel(now, choice)}.{/if}
			</p>
		</div>
		{#if failure}
			<p role="alert" class="text-sm/6 text-danger-text">
				Couldn't save the time zone. {failure} Your choice is still selected: try again.
			</p>
		{:else if confirmed}
			<p role="status" class="text-sm/6 text-fg-secondary">
				Saved. Times now show in {confirmed.replaceAll('_', ' ')}.
			</p>
		{/if}
	</div>
</Section>
