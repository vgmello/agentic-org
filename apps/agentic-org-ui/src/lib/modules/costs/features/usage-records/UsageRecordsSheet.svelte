<!--
  @component
  The effective usage records behind one cost figure (costs-and-insights-ux §1 "Each figure opens its records",
  COST-4), newest first, in a side sheet so the breakdown stays where it was. Load more reads the next page and keeps
  the place. States: loading, no records ("No usage records yet", not a claim of zero use), error with Retry.
  Open it with `show(target, period)`; closing returns focus to the figure that opened it.
-->
<script lang="ts">
	import CostsError from '../states/CostsError.svelte';
	import { tick } from 'svelte';
	import SkeletonRows from '$lib/components/SkeletonRows.svelte';
	import { mobileButton } from '$lib/components/classes';
	import { Button } from '$lib/components/ui/button';
	import {
		Sheet,
		SheetContent,
		SheetDescription,
		SheetHeader,
		SheetTitle
	} from '$lib/components/ui/sheet';
	import { toAppError } from '$lib/office/api';
	import { costsApi } from '../../api';
	import { formatCount } from '../../money';
	import { periodLabel } from '../../present';
	import type { PeriodRequest, RecordsTarget, UsageRecord } from '../../types';
	import UsageRecordRow from './UsageRecordRow.svelte';

	let {
		organizationId,
		zone,
		onCloseAutoFocus
	}: {
		organizationId: string;
		/** The viewer's zone, for record times. */
		zone: string;
		onCloseAutoFocus?: (event: Event) => void;
	} = $props();

	let open = $state(false);
	let target = $state<RecordsTarget>({ key: { all: true }, label: '' });
	let period = $state<PeriodRequest>({ kind: 'this_month', timeZone: 'UTC' });
	let phase = $state<'loading' | 'ready' | 'error'>('loading');
	let records = $state.raw<UsageRecord[]>([]);
	let total = $state(0);
	let cursor = $state<string | undefined>();
	let more = $state<'idle' | 'loading' | 'error'>('idle');
	let message = $state('');
	let moreMessage = $state('');
	let run = 0;
	const listId = $props.id();

	export function show(next: RecordsTarget, nextPeriod: PeriodRequest) {
		target = next;
		period = nextPeriod;
		open = true;
		void load();
	}

	async function load() {
		const mine = ++run;
		phase = 'loading';
		records = [];
		cursor = undefined;
		more = 'idle';
		try {
			const page = await costsApi.records(organizationId, period, target.key);
			if (mine !== run) return;
			records = page.records;
			total = page.total;
			cursor = page.nextCursor;
			phase = 'ready';
		} catch (e) {
			if (mine !== run) return;
			message = toAppError(e).message;
			phase = 'error';
		}
	}

	async function loadMore() {
		if (!cursor || more === 'loading') return;
		const mine = run;
		const first = records.length;
		more = 'loading';
		try {
			const page = await costsApi.records(organizationId, period, target.key, cursor);
			if (mine !== run) return;
			records = [...records, ...page.records];
			cursor = page.nextCursor;
			more = 'idle';
			// Focus moves to the first new record, so keyboard reading continues where it left off.
			await tick();
			document.getElementById(listId)?.querySelectorAll<HTMLElement>(':scope > li')[first]?.focus();
		} catch (e) {
			if (mine !== run) return;
			moreMessage = toAppError(e).message;
			more = 'error';
		}
	}

	const title = $derived(
		'all' in target.key ? 'All usage records' : `Usage records: ${target.label}`
	);
</script>

<Sheet bind:open>
	<SheetContent side="right" class="w-full gap-0 sm:max-w-lg" {onCloseAutoFocus}>
		<SheetHeader class="border-b border-line pr-14">
			<SheetTitle class="text-base/7 font-semibold [overflow-wrap:anywhere]">{title}</SheetTitle>
			<SheetDescription class="text-sm/6">
				{periodLabel(period.kind)} · effective records, newest first{phase === 'ready'
					? ` · ${formatCount(total)} ${total === 1 ? 'record' : 'records'}`
					: ''}
			</SheetDescription>
		</SheetHeader>
		<div class="min-h-0 flex-1 overflow-y-auto" data-testid="records-sheet">
			{#if phase === 'loading'}
				<SkeletonRows label="usage records" rows={5} />
			{:else if phase === 'error'}
				<div class="py-6">
					<CostsError level={3} title="Couldn't load the usage records" {message} onretry={load} />
				</div>
			{:else if records.length === 0}
				<div class="px-4 py-12 text-center">
					<h3 class="text-base/7 font-semibold text-fg">No usage records yet</h3>
					<p class="mx-auto mt-1 max-w-sm text-sm/6 text-fg-muted">
						Nothing has been recorded for this in the period. That is not a claim that nothing was
						used.
					</p>
				</div>
			{:else}
				<ul role="list" class="divide-y divide-line-subtle" id={listId}>
					{#each records as record (record.id)}
						<li
							class="px-4 py-4 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus"
							tabindex="-1"
							data-record={record.id}
						>
							<UsageRecordRow {record} {zone} />
						</li>
					{/each}
				</ul>
				<div class="px-4 py-4">
					<p class="text-xs/5 text-fg-muted" role="status">
						Showing {formatCount(records.length)} of {formatCount(total)}
					</p>
					{#if more === 'error'}
						<p class="mt-2 text-xs/5 text-danger-text" role="alert">{moreMessage}</p>
					{/if}
					{#if cursor}
						<Button
							variant="outline"
							class="mt-3 min-w-28 {mobileButton}"
							aria-busy={more === 'loading'}
							disabled={more === 'loading'}
							onclick={loadMore}
							>{more === 'loading'
								? 'Loading…'
								: more === 'error'
									? 'Try again'
									: 'Load more'}</Button
						>
					{/if}
				</div>
			{/if}
		</div>
	</SheetContent>
</Sheet>
