// A project page opened by an action (New project) focuses the project's heading once the project has loaded, so
// what was created is announced (app-shell-ux §6a: completing a flow moves focus to the next sensible place). The
// flag lives for one navigation: the frame takes it when it mounts.
let pending: string | undefined;

export function focusProjectOnArrival(projectId: string) {
	pending = projectId;
}

/** True once, for the project an action asked to focus. */
export function takeArrival(projectId: string): boolean {
	if (pending !== projectId) return false;
	pending = undefined;
	return true;
}
