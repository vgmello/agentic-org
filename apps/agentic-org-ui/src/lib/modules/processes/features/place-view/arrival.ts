// What the Processes list does when it is reached from a save: focus the saved document's row and say where it
// went ("Created Web review for Web (team)."). The editor sets it just before it navigates; the list takes it once.
export interface Arrival {
	documentId: string;
	message: string;
}

let pending: Arrival | undefined;

export function arriveWith(arrival: Arrival) {
	pending = arrival;
}

export function takeArrival(): Arrival | undefined {
	const arrival = pending;
	pending = undefined;
	return arrival;
}
