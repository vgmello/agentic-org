// Members and access reads accessApi itself. The route depends on the organization's data, so a change made elsewhere
// (a hand-over of ownership, a person added or deactivated) reruns this load; the page re-reads when the count moves.
let loads = 0;

export function load({ depends }) {
	depends('app:org');
	return { loads: ++loads };
}
