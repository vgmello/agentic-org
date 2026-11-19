// Moves focus to a page heading by id (app-shell-ux §6a): headings take focus as a place to read from, without a
// control ring, so they become programmatically focusable first.
export function focusHeading(id: string): boolean {
	const heading = document.getElementById(id);
	if (!heading) return false;
	heading.tabIndex = -1;
	heading.focus({ preventScroll: true });
	return true;
}
