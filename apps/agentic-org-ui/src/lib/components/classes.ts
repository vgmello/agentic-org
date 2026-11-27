// Class strings the settings screens share (components/README-screens.md: Class strings).
export const gutters = 'px-4 min-[640px]:px-6 min-[1024px]:px-8';
export const inputClass =
	'border-0 bg-surface-raised px-3 py-1.5 text-base text-fg shadow-none outline-1 outline-solid -outline-offset-1 outline-field-border sm:text-sm/6 md:text-sm/6 dark:bg-surface-raised placeholder:text-fg-muted focus-visible:ring-0 focus-visible:outline-2 focus-visible:outline-solid focus-visible:-outline-offset-2 focus-visible:outline-focus aria-invalid:outline-danger max-[639px]:min-h-11';
export const selectClass =
	'block w-full rounded-md bg-surface-raised px-3 py-1.5 text-base text-fg outline-1 -outline-offset-1 outline-field-border focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus aria-invalid:outline-danger sm:text-sm/6 max-[639px]:min-h-11 min-[640px]:h-9 [&>option]:bg-surface';
export const textareaClass =
	'block w-full rounded-md bg-surface-raised px-3 py-2 text-base text-fg outline-1 -outline-offset-1 outline-field-border placeholder:text-fg-muted focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-focus aria-invalid:outline-danger sm:text-sm/6';
export const helpClass = 'mt-2 text-xs/5 text-fg-muted';
export const errorClass = 'mt-2 text-xs/5 text-danger-text';
/** The red primary of a destructive confirmation (spec 6.4: "Remove Studio Mac (red)"). */
export const dangerButton =
	'bg-danger-emphasis text-on-emphasis hover:bg-danger-emphasis/80 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-danger-text';
export const mobileButton = 'max-[639px]:min-h-11';
/** Shared styling for portal-mounted Bits UI Tooltip.Content popovers on icon-only controls. */
export const tooltipContentClass =
	'pointer-events-none z-60 rounded-[var(--exec-sm-radius,6px)] border border-line-strong bg-[var(--side-panel-surface,var(--surface-overlay,var(--exec-surface)))] px-2 py-1 text-xs leading-[1.3] font-medium text-[var(--exec-ink)] shadow-[var(--tooltip-shadow)]';
