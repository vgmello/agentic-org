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

/* Keep an open menu attached to its trigger while the board or page scrolls; close it once the trigger is gone */
function followOpenPopoverMenus() {
  for (const menu of document.querySelectorAll(".menu[popover]:popover-open")) {
    const trigger = popoverTrigger(menu), r = trigger?.getBoundingClientRect();
    if (!r || r.bottom < 0 || r.top > window.innerHeight || r.right < 0 || r.left > document.documentElement.clientWidth) {
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
