/**
 * Bits UI `Tooltip.Trigger` includes `type: "button"` in its child `props`, which is invalid HTML
 * when spread onto an `<a>` anchor element. Strip `type` while preserving all ARIA/event handlers.
 */
export function tooltipLinkProps(props: Record<string, unknown>): Record<string, unknown> {
	const out: Record<string, unknown> = {};
	for (const [key, value] of Object.entries(props)) {
		if (key !== 'type') out[key] = value;
	}
	return out;
}
