"""Bake immutable CPU model snapshots into the service image; no remote code."""
import json
from pathlib import Path

from huggingface_hub import snapshot_download

root = Path(__file__).resolve().parent
policy = json.loads((root / "runtime.json").read_text())
for name, model in policy["models"].items():
    snapshot_download(repo_id=model["repository"], revision=model["revision"],
                      local_dir=root / "models" / name,
                      allow_patterns=[model["file"], "config.json", "tokenizer.json",
                                      "tokenizer_config.json", "special_tokens_map.json",
                                      "sentencepiece.bpe.model", "vocab.txt"])
