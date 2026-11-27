#!/usr/bin/env bash
set -euo pipefail

# Runs the standardized independent 6-dimension Svelte 5 / SvelteKit audit
# against `apps/agentic-org-ui` since the last verified PASS in `audit-checkpoint.json`.
# Always run a fresh internal code-reviewer pass and `bun run gate` first.

UI_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(git -C "$UI_DIR" rev-parse --show-toplevel)"
CHECKPOINT="$UI_DIR/audit-checkpoint.json"
MODEL="${UI_AUDIT_MODEL:-claude-opus-5-5}"
EFFORT="${UI_AUDIT_EFFORT:-xhigh}"

if ! command -v claude >/dev/null 2>&1; then
  echo "error: 'claude' CLI not found on PATH. Install Claude Code or run an equivalent fresh cross-model auditor." >&2
  exit 1
fi

LAST_SHA="$(node -e "const c = JSON.parse(require('fs').readFileSync(process.argv[1],'utf8')); console.log(c.lastPassSha);" "$CHECKPOINT")"
LAST_DATE="$(node -e "const c = JSON.parse(require('fs').readFileSync(process.argv[1],'utf8')); console.log(c.lastPassDate);" "$CHECKPOINT")"
LAST_OVERALL="$(node -e "const c = JSON.parse(require('fs').readFileSync(process.argv[1],'utf8')); console.log(c.overall);" "$CHECKPOINT")"
HEAD_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD)"
OUT_FILE="${UI_AUDIT_OUT:-/tmp/agentic-org-ui-audit-${HEAD_SHA}.md}"

echo "Running independent UI 6-dimension audit (${MODEL}, effort=${EFFORT}) for ${LAST_SHA}..${HEAD_SHA}..."
echo "Report output: ${OUT_FILE}"

PROMPT=$(cat <<EOF
You are conducting an independent, strict, evidence-based periodic 6-dimension audit of \`apps/agentic-org-ui\` at commit \`${HEAD_SHA}\` in \`${REPO_ROOT}\`.

Last verified PASS checkpoint (\`apps/agentic-org-ui/audit-checkpoint.json\`):
- Commit: \`${LAST_SHA}\` (${LAST_DATE}), overall \`${LAST_OVERALL}/10\` PASS.

Audit rules (be ruthlessly honest — NEVER grade-inflate; do NOT modify repository files; follows official process \`backlog/docs/processes/doc-105 - Process-Periodic-UI-6-Dimension-Audit.md\`):
1. Read \`.agents/rules/svelte-architecture-and-quality.md\` and inspect \`git diff ${LAST_SHA}..${HEAD_SHA} -- apps/agentic-org-ui/\` plus any touched files and compiled \`build/\` CSS/JS as needed.
2. Check for regressions across all 6 dimensions:
   - Security / Auth (auth/SSE choke-point, zero raw HTML sinks, safe URLs, built-in HR/Tess immutability, caller-scoped device storage, strict CSP)
   - Svelte 5 Reactivity / Lifecycle (runes, \`\$state.raw\` / \`\$state.snapshot\`, no dialog-reset \`\$effect\`s, \`AbortSignal\` propagation, timer/rAF/stream teardown, stable \`{#each}\` keys)
   - Architecture / Vertical Slices (zero cycles, non-UI barrels, \`layer-exceptions.allow=0\`, \`cross-module-features.allow=0\`, clean splits)
   - Bundle / Performance (shell/onboarding budgets, zero route/layout overrides, lazy-loaded overlays, O(1) streaming updates)
   - Design System / A11y (icon-only WCAG 1.4.13 tooltips with compiled CSS verification, WebKit focus restoration, scoped CSS, 7-theme semantic tokens, \`prefers-reduced-motion\`)
   - File Size / Maintainability (\`large-files.allow=0\`, \`<=500\` LOC new files, no dead code or stale baselines, regression tests per fix)
3. Score each dimension out of 10.00 and compute the Overall score.
4. Verdict:
   - \`PASS\` ONLY if EVERY single one of the 6 dimensions is genuinely \`>= 9.0/10\` AND zero open High or Medium findings exist.
   - Otherwise \`CLOSE\` or \`FAIL\` with exact \`file:line\` findings, severity, and shortest path to \`PASS\`.
EOF
)

(
  cd "$REPO_ROOT"
  claude -p --model "$MODEL" --effort "$EFFORT" --permission-mode plan "$PROMPT"
) | tee "$OUT_FILE"

echo ""
echo "Saved audit report to: ${OUT_FILE}"
echo "If verdict is PASS (all 6 dimensions >= 9.0/10, 0 High/Medium), update apps/agentic-org-ui/audit-checkpoint.json with lastPassSha=${HEAD_SHA}."
