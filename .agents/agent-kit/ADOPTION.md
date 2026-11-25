# Agentic Org kit adoption

The agent kit in `.agents/agent-kit/` and `.agents/skills/` supports the workflow in
[AGENTS.md](../AGENTS.md) and [branch and merge](../docs/processes/branch-and-merge.md);
it does not change them.

## What is installed

- Base kit 0.10.0, pinned to
  https://github.com/vgmello/agents/tree/01e0d2f8ec338cf84dcbf15fe158ee5181fbd0b1 .
  Local adaptations of upstream runtime files are recorded with their upstream and
  current hashes in `.starter-kit-manifest.json`.
- Mode `codex-only` (`policy/model-routing.json`): Sol with the profile's effort levels.
- Repowise 0.61.0 for structural discovery, in an isolated per-checkout environment.
- Hindsight 1.2.1 as the local `hindsight-embed` daemon for memory recall.
- Four project role briefs (`project-roles/`), five project skills (`project-skills/`),
  the skill catalog (`skill-catalog.json`), project maps and real verification commands
  (`verification.json`).

## Prepare a checkout

Every checkout and worktree is prepared on its own; never copy caches, receipts or
logs between checkouts. From the repository root:

```sh
python3 .agents/agent-kit/runtime/kit_context.py setup --query canonical --expect-path docs/design/organization-data/test_schema.py
python3 .agents/agent-kit/runtime/kit_memory.py setup
python3 .agents/agent-kit/runtime/kit_skills.py validate
python3 .agents/agent-kit/runtime/kit_doctor.py --project . --live --checks --json
```

Results go to the ignored `.agents/agent-kit/logs/`. Report three things separately and only
from current evidence: CONFIGURED, CHECKS_VERIFIED and LIVE_VALIDATED. A saved login or
a version probe does not prove that a model is available. Native hooks need trust in a
fresh client session; they do not replace explicit checks.

## Local memory daemon

The pinned install and check commands:

```sh
uv tool install --python python3.12 --with hindsight-api==1.2.1 --with psutil==7.2.2 hindsight-embed==1.2.1
HINDSIGHT_API_LLM_PROVIDER=openai-codex HINDSIGHT_API_LLM_MODEL=gpt-6.1-sol npx --yes @vectorize-io/hindsight-coding-agents@0.8.7 install codex --server daemon
python3 .agents/agent-kit/runtime/kit_memory.py setup
hindsight-embed -p coding-agent daemon status
python3 .agents/agent-kit/runtime/kit_hindsight_embed.py test
```

`hindsight-embed` uses the profile `coding-agent`, embedded pg0, the loopback address
`http://127.0.0.1:9077`, local CPU embeddings and reranking, and the existing Codex
sign-in for its native memory bank. No API key is stored. Setup checks the live
process, its owner and listener, the package versions and the saved profile; drift or a
foreign port owner fails closed without stopping that process. The kit's own bank of
reviewed Git records keeps chunks only, with no model calls. Memory is a hint: current
source and the owning documents decide.

Re-running the native installer can replace its environment wiring; check it after an
update.
