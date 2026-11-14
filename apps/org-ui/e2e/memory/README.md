# Chat row memory probe (MEM-2)

Run from `apps/org-ui`, under the machine's heavy-run wrapper:

```sh
~/.agency/bin/heavy bun e2e/memory/probe.ts --browser=chromium
~/.agency/bin/heavy bun e2e/memory/probe.ts --browser=firefox
~/.agency/bin/heavy bun e2e/memory/probe.ts --browser=webkit
```

Use a free `PW_PORT` (default 4860). On macOS, Chromium and WebKit also accept `--footprint`. Keep the machine quiet when recording footprint results. Firefox checks content, scrolling and errors; it has no heap/footprint budget. Chromium reads heap and DOM counters after three forced garbage collections. The later MEM-1 windowing slices own the element/row/heap bounds; this slice checks row nodes and Portal lifetime.

The script copies source and test fixtures into an ignored `test-results/memory/<timestamp>/app` sandbox and builds with the test-only configuration there. The ordinary Vite configuration and production composition stay unchanged. Generated JSON, logs and screenshots remain under `test-results/`; the copied app is removed after the probe. Never serve or ship the memory sandbox as a release.

The seed uses Rowan's five message bodies and alternates Sam/owner messages, in a 1,000-message DM and channel. The short-row count includes text and comment nodes in the message container. The mean is the change in Chromium's DOM counter divided by the change in mounted messages between the idle conversation and the seeded conversation.

The Portal check follows Rowan's rev 2.5 pin: use exact 1-, 2- and 1,000-row variants of the same DM, leaving the sidebar and viewport unchanged. Move the pointer outside the message list, focus the shell Chat link and close all menus. N counts **all** direct body child nodes except the application's `display: contents` root, including text and comments. The allowed slope is **c_base = 11**, calibrated once from N(1)=194 and N(2)=205 on `5ada73e03781c65b15e965b3539f00cda9fb468b`. The pinned dependency hash, viewport and observed counts are in `portal-calibration.ts`. A later head must satisfy N(1,000)−N(1) ≤ that frozen constant and retain a menu trigger in every row. Opening and closing a menu must return to the same N after animations settle.

To reproduce the one-time calibration, extract `git archive 5ada73e0 apps/org-ui` into a separate ignored directory, copy this directory's seed/config there, add `.memory-build-sandbox`, and link the pinned dependencies. Build/preview using `e2e/memory/vite.memory.config.ts`. Create the sample organization; set `memory.exact=true`, `memory.conversation=c-dm-sam` and `memory.messages` to 1 then 2 before full reloads of `/chat?c=c-dm-sam`. Assert those exact row/trigger counts and measure N with the definition above. The calibration must be at least one; a candidate head's zero slope is not a new calibration.
