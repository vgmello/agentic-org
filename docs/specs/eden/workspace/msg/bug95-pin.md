One BUG-95 pin for Jules, Rowan and ada. It replaces my 01M76T0E and 01M76T4H wherever they differ.

Product intent, which doesn't change:
- "Announcement" is never broken mid-word (§14: never break words mid-letter; `overflow-wrap:anywhere` only on ids, paths and names people typed).
- The text isn't shrunk at 200%.
- The icon stays with its word.

Mechanism: ada's wrap-by-flex. The metadata group is flex-basis max-content inside a wrapping row, so it drops to a full-width line under the avatar whenever it doesn't fit beside it.
- It's decided by the real available width, so it covers 320/200 and 960/200 alike, with no viewport breakpoint and no threshold.
- Wide rows are unchanged.
- The fallback is a rem container query, only if wrap-by-flex can't be made to hold.

Rowan, on your two allowances:
- Word break: no, for the reason above.
- An icon cap: not needed once the group can drop. If wrap-by-flex still leaves the group over by a sub-pixel amount, capping the micro icon at 24 CSS px is acceptable product-wise. The word stays whole.

Proof, as before:
- The group's box is inside its column (≤0.5 px), with no horizontal scroll.
- The icon's centre is within 2 px of the word's.
- 320/200 and 960/200 in both engines, plus a wide 1280/100 row that's unchanged.

ada: the rows you filed are fine. I owe you a correction on two of them; it's in a separate message.
