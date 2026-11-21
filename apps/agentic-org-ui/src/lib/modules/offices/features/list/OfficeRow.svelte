<!--
  @component
  One office in the Offices list (offices-and-teams-ux §6.1, OFC-1, OFC-13): its name (HQ marked), where it runs with
  freshness, how many agents it holds, one status pill, and Retry while the connected office doesn't answer. The whole row
  opens the office's details; its label says every fact, so the pill and count aren't read twice.
-->
<script lang="ts">
	import ChevronRight from '@lucide/svelte/icons/chevron-right';
	import DividedRow from '$lib/components/DividedRow.svelte';
	import { Button } from '$lib/components/ui/button';
	import {
		isHq,
		repairReasonCopy,
		rowLabel,
		secondaryText,
		type DisplayStatus
	} from '$lib/modules/offices/model';
	import type { OfficeInfo } from '$lib/proto';
	import OfficeRepairLocation from '../details/OfficeRepairLocation.svelte';
	import OfficeStatusBadge from '../status/OfficeStatusBadge.svelte';
	import { compactRow } from './compact-row';

	let {
		office,
		shown,
		agents,
		countError,
		here,
		status,
		why,
		retry,
		onopen,
		onretry
	}: {
		/** The office as listed. */
		office: OfficeInfo;
		/** The office as its row words it (an office that joined: where it is and when it was last seen). */
		shown: OfficeInfo;
		/** Undefined while the agents are counted, or when they couldn't be. */
		agents: number | undefined;
		countError: boolean;
		/** This is the office the app is connected to. */
		here: boolean;
		status: DisplayStatus;
		/** Why it stopped, with the countdown. */
		why: string | undefined;
		/** Retry reaching the connected office: absent when the row offers none; `busy` while it works. */
		retry: { busy: boolean } | undefined;
		onopen: (button: HTMLButtonElement) => void;
		onretry: () => void;
	} = $props();
</script>

<DividedRow
	data-testid="office-row"
	data-office={office.id}
	data-office-name={office.name}
	class="{compactRow.gutters} {compactRow.gap} flex flex-wrap items-center justify-end gap-x-4 gap-y-2 hover:bg-tint/[0.025]"
>
	<button
		type="button"
		class="{compactRow.name} min-w-[min(6ch,100%)] flex-1 basis-[6ch] text-left outline-none [&:focus-visible>.absolute]:outline-2 [&:focus-visible>.absolute]:outline-solid [&:focus-visible>.absolute]:-outline-offset-2 [&:focus-visible>.absolute]:outline-focus max-[639px]:order-first max-[639px]:w-full max-[639px]:flex-none"
		aria-label={rowLabel(shown, agents, here, status, why)}
		aria-haspopup="dialog"
		onclick={(event) => onopen(event.currentTarget)}
	>
		<span class="relative block truncate text-sm/6 font-semibold text-fg" title={office.name}
			>{office.name}{#if isHq(office) && office.name !== 'HQ'}<span class="sr-only"
					>&#32;·&#32;</span
				><span
					class="ml-2 inline-block rounded-md bg-brand/10 px-1.5 align-[0.0625rem] text-xs/5 font-medium text-brand-text ring-1 ring-brand/20 ring-inset"
					data-testid="hq-badge">HQ</span
				>{/if}</span
		>
		<span class="mt-1 flex flex-wrap items-baseline gap-x-2 text-xs/5 text-fg-muted">
			<span class="max-w-full [overflow-wrap:anywhere]" title={why} data-testid="office-location">
				{#if office.status_reason === 'unsupported_version' && why}
					{repairReasonCopy.before}<OfficeRepairLocation {office} />{repairReasonCopy.after}
				{:else}{status === 'unreachable'
						? "Can't reach this office"
						: (why ?? secondaryText(shown) + (here ? " (you're here)" : ''))}{/if}
			</span>
			<span data-testid="agent-count" aria-hidden="true">
				{#if agents !== undefined}{agents} {agents === 1 ? 'agent' : 'agents'}
				{:else if countError}—
				{:else}<span
						class="inline-block h-4 w-14 animate-pulse rounded bg-surface-overlay align-middle motion-reduce:animate-none"
						data-testid="count-skeleton"
					></span>{/if}
			</span>
		</span>
		<span class="absolute inset-0" aria-hidden="true"></span>
	</button>
	<!-- The button's label already says the status and the count: not read twice. -->
	<span
		class="ml-auto inline-flex max-w-full flex-none flex-wrap items-center justify-end gap-x-2 gap-y-2"
		data-testid="office-actions"
	>
		<span class="contents" aria-hidden="true"><OfficeStatusBadge {status} /></span>
		{#if retry}
			<!-- Stays mounted while it works (aria-disabled, not disabled), so keyboard focus isn't lost. -->
			<Button
				variant="outline"
				size="xs"
				data-testid="retry-office"
				class="relative z-10 aria-disabled:cursor-not-allowed aria-disabled:opacity-60"
				aria-label="{retry.busy ? 'Retrying…' : 'Retry'} reaching {office.name}"
				aria-disabled={retry.busy}
				aria-busy={retry.busy}
				onclick={(event) => {
					if (retry.busy) return;
					event.currentTarget.focus({ preventScroll: true });
					onretry();
				}}>{retry.busy ? 'Retrying…' : 'Retry'}</Button
			>
		{/if}
		<ChevronRight
			class="{compactRow.chevron} ml-2 size-5 shrink-0 text-fg-muted"
			aria-hidden="true"
		/>
	</span>
</DividedRow>
