export interface MemberPreviewClampResult {
	flipAbove: boolean;
	shiftX: number;
	shiftY: number;
	maxHeight: number;
}

/**
 * Computes collision-clamped placement offsets for the hover/focus member preview card
 * within the reading container and viewport bounds (WCAG 1.4.13).
 */
export function computeMemberPreviewClamp(
	anchorNode: HTMLElement,
	readingEl: HTMLElement | null
): MemberPreviewClampResult | undefined {
	const card = anchorNode.querySelector<HTMLElement>('.triage-from-card');
	if (!card) return undefined;
	const cardRect = card.getBoundingClientRect();
	if (cardRect.width === 0 || cardRect.height === 0) return undefined;

	const anchorRect = anchorNode.getBoundingClientRect();
	const containerRect = readingEl?.getBoundingClientRect();
	const pad = 8;
	const minLeft = Math.max(pad, (containerRect?.left ?? 0) + pad);
	const maxRight = Math.min(
		window.innerWidth - pad,
		(containerRect?.right ?? window.innerWidth) - pad
	);
	const minTop = Math.max(pad, (containerRect?.top ?? 0) + pad);
	const maxBottom = Math.min(
		window.innerHeight - pad,
		(containerRect?.bottom ?? window.innerHeight) - pad
	);
	const maxHeight = Math.max(48, Math.floor(maxBottom - minTop));
	const cardH = Math.min(cardRect.height, maxHeight);

	const spaceBelow = maxBottom - (anchorRect.bottom + 4);
	const spaceAbove = anchorRect.top - 4 - minTop;
	const flipAbove = spaceBelow < cardH && spaceAbove > spaceBelow;

	const naturalTop = flipAbove ? anchorRect.top - 4 - cardH : anchorRect.bottom + 4;
	let shiftY = 0;
	if (naturalTop + cardH > maxBottom) shiftY = maxBottom - (naturalTop + cardH);
	if (naturalTop + shiftY < minTop) shiftY = minTop - naturalTop;

	let shiftX = 0;
	if (cardRect.right > maxRight) {
		shiftX = Math.max(minLeft - cardRect.left, maxRight - cardRect.right);
	} else if (cardRect.left < minLeft) {
		shiftX = minLeft - cardRect.left;
	}

	return {
		flipAbove,
		shiftX: Math.round(shiftX),
		shiftY: Math.round(shiftY),
		maxHeight
	};
}
