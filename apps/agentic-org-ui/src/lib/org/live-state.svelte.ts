// What the shell shows about the live feed, kept outside the layout so it survives the layout being replaced (going to
// the welcome screen and back) and is readable by whatever loads the organization.
export const liveState = $state({
	/** Organizations whose live feed is off, by id: their screens show what was last loaded. */
	paused: {} as Record<string, boolean>,
	/** When each organization's structure was last loaded by the screens. */
	loadedAt: {} as Record<string, Date>
});
