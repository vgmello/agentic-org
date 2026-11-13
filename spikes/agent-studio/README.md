# Agent HQ — expandable agent studio

A self-contained 2D office prototype with six interactive robot roles. Characters share a jointed sprite rig; role-specific heads and torsos, furniture, and floor tiles preserve the illustrated office style while allowing the room to grow.

## Run

Requires Node.js 20 or newer.

```sh
cd spikes/agent-studio
npm ci
npm run dev
```

Open http://127.0.0.1:3000. The application is buildless: `dist/` contains the authored HTML, JavaScript, CSS, and images. No runtime services or API keys are required.

## Interact and expand

- Click a robot or its crew card to select it. Say hello, request role-specific work, or suggest a breather.
- Add desk creates an available workspace. Add agent fills an available workspace first, then creates one when needed.
- The room extends by rows without repositioning existing workstations.
- Drag to pan, scroll or pinch to zoom, and use the fit control (or F) to see the whole office.
- Removing an agent leaves its workspace available. Help includes a reset to the original six agents.
- Layouts are stored in this browser under `agent-hq-modular-v1`. Animation pauses when the page is hidden and respects reduced-motion preferences.

Current limits are 36 agents and 48 workspaces. This is a local visual simulation; the characters are not connected to an AI backend.

## Validate

```sh
npm test
npm run render:qa
```

The test suite covers autonomous behavior, click reactions, sprite rendering, workspace allocation, expansion, persistence, limits, and camera transforms. The render command writes representative 6-, 9-, and 12-agent scenes into the ignored `qa-output/` directory.

Canvas-based frame inspection and automated behavior tests were completed. Full browser interaction verification was unavailable in the authoring environment.

## Structure

| File | Responsibility |
| --- | --- |
| `dist/office-model.js` | Roles, workspace allocation, capacity, saved layouts, and camera transforms |
| `dist/brain.js` | Independent autonomous routines, smooth pose transitions, and reactions |
| `dist/puppet.js` | Sprite-atlas extraction and the shared 12-part character rig |
| `dist/room.js` | Isometric floor, walls, furniture, lighting, and depth ordering |
| `dist/app.js` | Crew cards, direct selection, action controls, pan/zoom, and persistence |
| `dist/assets/` | Reference artwork and generated character, furniture, and floor assets |
| `scripts/serve.mjs` | Dependency-free local static server |
| `tests/` | Behavior, layout, rendering, and camera checks |

Role definitions supply appearance and workstation types. New agents reuse the same animation rig and receive independent state machines. The default layout uses three columns; additional workspaces extend the room length.

This standalone prototype is kept under `spikes/` for later integration into the main application. The hosted Site's account-specific deployment manifest is intentionally excluded from this export.

