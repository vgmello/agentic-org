/* Leaving a work board for Inbox and coming back. The board's place (lanes, scroll, the control you used) is kept
   in the board's own history entry, so Back restores it. Inbox entries opened from the board carry the board's
   address, so they can offer "Back to work board" and keep work links inside the same scope. */
let pending = null;
let inboxOrigin = null;
let restoreNext = null;
const places = new Map();

function replaceState(state) {
  try { history.replaceState(state, "", location.href); } catch (e) {}
}

/* Called just before a link on the board navigates away */
function rememberBoardExit(place, toHash) {
  if (!place) return;
  const from = location.hash;
  places.set(from, place);
  replaceState({ ...history.state, boardPlace: place });
  pending = { from, to: toHash };
}

/* Called at the start of every render: tag the Inbox entry that a board opened, and the Inbox entries after it */
function adoptBoardReturn(page) {
  const state = history.state ?? {};
  if (page === "inbox" && pending && location.hash === pending.to) {
    replaceState({ ...state, fromBoard: pending.from, direct: true });
  } else if (page === "inbox" && inboxOrigin && !state.fromBoard) {
    replaceState({ ...state, fromBoard: inboxOrigin, direct: false });
  }
  pending = null;
  inboxOrigin = page === "inbox" ? history.state?.fromBoard ?? null : null;
}

const boardOrigin = () => history.state?.fromBoard ?? null;

/* Back when the board is the previous entry, so its saved place comes back; otherwise go to it and restore */
function returnToBoard() {
  const state = history.state ?? {};
  if (!state.fromBoard) return false;
  if (state.direct) {
    history.back();
  } else {
    restoreNext = places.get(state.fromBoard) ?? null;
    location.hash = state.fromBoard;
  }
  return true;
}

function takeBoardReturnPlace() {
  const place = restoreNext ?? history.state?.boardPlace ?? null;
  restoreNext = null;
  return place;
}

export { adoptBoardReturn, boardOrigin, rememberBoardExit, returnToBoard, takeBoardReturnPlace };
