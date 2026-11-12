type Snapshot = {
	node: HTMLElement; path: EventTarget[]; target: Element | null; button: number; pointerType: string;
	x: number; y: number; rect: DOMRect; insideTree: boolean;
};

/** Only widen Bits' validity when dispatch geometry proves an outside mouse/pen click. */
export function isOutsideAtDispatch(snapshot: Snapshot | undefined, node: HTMLElement): boolean {
	if (!snapshot || snapshot.node !== node || snapshot.button !== 0 || snapshot.pointerType === "touch"
		|| !snapshot.target?.isConnected || snapshot.insideTree) return false;
	const { x, y, rect } = snapshot;
	return x < rect.left || x > rect.right || y < rect.top || y > rect.bottom;
}

export function resolveMenuRoot(node: HTMLElement): HTMLElement | null {
	const visited = new Set<HTMLElement>();
	while (node.isConnected && !visited.has(node)) {
		visited.add(node);
		const trigger = node.ownerDocument.querySelector(`[aria-controls~="${CSS.escape(node.id)}"]`);
		if (!trigger?.isConnected) return null;
		const parent = trigger.closest<HTMLElement>('[role="menu"]');
		if (!parent) return node;
		node = parent;
	}
	return null;
}

export function createMenuDispatch(content: () => HTMLElement | null) {
	const snapshots = new WeakMap<Event, Snapshot>();
	$effect(() => {
		const node = content();
		if (!node) return;
		const document = node.ownerDocument;
		function captureMenuDispatch(event: Event) {
			const path = event.composedPath(), root = resolveMenuRoot(node);
			const pointer = event as PointerEvent;
			const insideTree = !root || path.some(target => {
				if (!(target instanceof HTMLElement) || !target.matches('[role="menu"]')) return false;
				const owner = resolveMenuRoot(target);
				return !owner || owner === root;
			});
			snapshots.set(event, { node, path, target: event.target instanceof Element ? event.target : null, button: pointer.button,
				pointerType: pointer.pointerType, x: pointer.clientX, y: pointer.clientY,
				rect: node.getBoundingClientRect(), insideTree });
		}
		document.addEventListener("pointerdown", captureMenuDispatch, true);
		document.addEventListener("click", captureMenuDispatch, true);
		return () => {
			document.removeEventListener("pointerdown", captureMenuDispatch, true);
			document.removeEventListener("click", captureMenuDispatch, true);
		};
	});
	return { get: (event: Event) => snapshots.get(event), valid: (event: PointerEvent, node: HTMLElement) => isOutsideAtDispatch(snapshots.get(event), node) };
}
