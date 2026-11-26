/* Menus that open from inside a scrolling region (board cards, lane headers) live in the top layer, so a
   scroller's edge or a card's stacking context never clips them. They open below the trigger, flip above it
   when there is more room there, and stay inside the viewport. */
const GAP = 6;
const EDGE = 8;

const isPopoverMenu = (menu) => menu?.hasAttribute("popover");
const popoverTrigger = (menu) => document.querySelector(`[aria-controls="${CSS.escape(menu.id)}"]`);

function placePopoverMenu(menu, trigger) {
  const t = trigger.getBoundingClientRect();
  const vw = document.documentElement.clientWidth, vh = window.innerHeight;
  menu.style.maxHeight = "";
  const { width, height } = menu.getBoundingClientRect();
  const below = vh - t.bottom - GAP - EDGE, above = t.top - GAP - EDGE;
  const up = height > below && above > below;
  const room = Math.max(0, up ? above : below);
  if (height > room) menu.style.maxHeight = `${room}px`;
  const shown = Math.min(height, room);
  menu.style.top = `${up ? t.top - GAP - shown : t.bottom + GAP}px`;
  menu.style.left = `${Math.max(EDGE, Math.min(t.left, vw - width - EDGE))}px`;
  menu.dataset.side = up ? "top" : "bottom";
}

function openPopoverMenu(menu, trigger) {
  menu.hidden = false;
  if (!menu.matches(":popover-open")) menu.showPopover();
  placePopoverMenu(menu, trigger);
}

function closePopoverMenu(menu) {
  if (menu.matches(":popover-open")) menu.hidePopover();
}

/* The area a trigger can be seen in: the viewport, narrowed to its scroller ([data-popover-scroller]) and, at the top,
   to below any sticky header in that scroller ([data-popover-inset]) that sits over the trigger's column */
function visibleArea(trigger) {
  const area = { top: 0, bottom: window.innerHeight, left: 0, right: document.documentElement.clientWidth };
  const scroller = trigger.closest("[data-popover-scroller]");
  if (!scroller) return area;
  const s = scroller.getBoundingClientRect(), t = trigger.getBoundingClientRect();
  Object.assign(area, { top: Math.max(area.top, s.top), bottom: Math.min(area.bottom, s.bottom), left: Math.max(area.left, s.left), right: Math.min(area.right, s.right) });
  for (const inset of scroller.querySelectorAll("[data-popover-inset]")) {
    const r = inset.getBoundingClientRect();
    if (r.left < t.right && r.right > t.left && r.top <= t.top) area.top = Math.max(area.top, r.bottom);
  }
  return area;
}
/* A trigger counts as gone once its middle has left that area (scrolled under a sticky header, or out of the board) */
function triggerHidden(trigger) {
  const r = trigger?.getBoundingClientRect();
  if (!r || !r.width) return true;
  const area = visibleArea(trigger), x = r.left + r.width / 2, y = r.top + r.height / 2;
  return y < area.top || y > area.bottom || x < area.left || x > area.right;
}
/* Keep an open menu attached to its trigger while the board or page scrolls; close it once the trigger is out of sight */
function followOpenPopoverMenus() {
  for (const menu of document.querySelectorAll(".menu[popover]:popover-open")) {
    const trigger = popoverTrigger(menu);
    if (triggerHidden(trigger)) {
      closePopoverMenu(menu);
      menu.hidden = true;
      trigger?.setAttribute("aria-expanded", "false");
    } else {
      placePopoverMenu(menu, trigger);
    }
  }
}
document.addEventListener("scroll", followOpenPopoverMenus, { capture: true, passive: true });
window.addEventListener("resize", followOpenPopoverMenus);

const FOCUSABLE = 'a[href], button:not([disabled]), select:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/* Tab leaves an open menu the way it would leave its trigger: Shift+Tab returns to the trigger,
   Tab moves on to the next control after it. */
function tabOutOfPopoverMenu(menu, backwards) {
  const trigger = popoverTrigger(menu), wrap = menu.closest(".menu-wrap") ?? menu;
  if (backwards || !trigger) { trigger?.focus(); return; }
  const next = [...document.querySelectorAll(FOCUSABLE)].find((el) =>
    !wrap.contains(el) && (wrap.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING)
    && el.tabIndex >= 0 && el.getClientRects().length && !el.closest("[hidden], [inert]"));
  (next ?? trigger).focus();
}

export { closePopoverMenu, isPopoverMenu, openPopoverMenu, tabOutOfPopoverMenu };
