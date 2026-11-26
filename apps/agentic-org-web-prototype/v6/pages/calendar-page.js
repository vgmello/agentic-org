/* Personal calendar page (#/calendar): every meeting you attend, with people and agents. */
import { calendarHtml } from "./calendar.js";
import { hdr } from "../ui.js";

function pageCalendar() {
  return hdr("Calendar", "Your meetings with people and agents") + calendarHtml("me");
}

export { pageCalendar };
