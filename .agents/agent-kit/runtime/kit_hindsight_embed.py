#!/usr/bin/env python3
"""Local, pinned hindsight-embed deployment requested by the project owner.

The native coding-agent bank uses Codex. The kit's separate reviewed-record bank
keeps chunk-only retention and disabled consolidation. Never inspect/log secrets.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from kit_context import write

POLICY = '.agents/agent-kit/policy/hindsight/embed.json'


def settings(repo):
    value = json.loads((repo / POLICY).read_text())
    if value != {'version': '1.2.1', 'deployment': 'hindsight-embed',
                 'profile': 'coding-agent', 'port': 9077,
                 'llm_provider': 'openai-codex', 'llm_model': 'gpt-6.1-sol',
                 'python': '3.12', 'psutil_version': '7.2.2'}:
        raise ValueError('Unexpected embed deployment policy; review it before setup')
    return value


def environment(policy, runtime, checksum):
    models = Path.home() / '.cache/agent-kit/hindsight' / checksum / 'models'
    env = {
        'HINDSIGHT_EMBED_API_VERSION': policy['version'],
        'HINDSIGHT_API_HOST': '127.0.0.1',
        'HINDSIGHT_API_LLM_PROVIDER': policy['llm_provider'],
        'HINDSIGHT_API_LLM_MODEL': policy['llm_model'],
        'HINDSIGHT_API_RETAIN_EXTRACTION_MODE': 'chunks',
        'HINDSIGHT_API_ENABLE_OBSERVATIONS': 'false',
        'HINDSIGHT_API_ENABLE_AUTO_CONSOLIDATION': 'false',
        'HINDSIGHT_API_ENABLE_GRAPH_RETRIEVAL': 'false',
        'HINDSIGHT_API_ENABLE_TEMPORAL_RETRIEVAL': 'false',
        'HINDSIGHT_API_MCP_ENABLED': 'false',
        'HINDSIGHT_API_SKIP_LLM_VERIFICATION': 'true',
        'HINDSIGHT_API_EMBEDDINGS_PROVIDER': 'onnx',
        'HINDSIGHT_API_EMBEDDINGS_LOCAL_FORCE_CPU': '1',
        'HINDSIGHT_API_EMBEDDINGS_ONNX_DEVICE': 'cpu',
        'HINDSIGHT_API_RERANKER_PROVIDER': 'onnx',
        'HINDSIGHT_API_RERANKER_LOCAL_FORCE_CPU': '1',
        'HINDSIGHT_API_RERANKER_ONNX_THREADS': '2',
        'HINDSIGHT_EMBED_API_DATABASE_URL': 'pg0://hindsight-embed-' + policy['profile'],
    }
    for name, prefix in [('embedding', 'EMBEDDINGS'), ('reranker', 'RERANKER')]:
        path = models / name
        env[f'HINDSIGHT_API_{prefix}_ONNX_MODEL_PATH'] = str(path / runtime['models'][name]['file'])
        env[f'HINDSIGHT_API_{prefix}_ONNX_TOKENIZER_NAME_OR_PATH'] = str(path)
    return env, models


def interpreter():
    executable = shutil.which('hindsight-embed')
    if executable:
        python = Path(executable).resolve().with_name('python')
        if python.is_file():
            return python
    raise ValueError('Install uv tool hindsight-embed==1.2.1 with hindsight-api==1.2.1 and psutil==7.2.2; see ADOPTION.md')


def invoke(repo, action, checksum, runtime):
    result = subprocess.run([str(interpreter()), str(Path(__file__).resolve()),
                             action, str(repo), checksum, json.dumps(runtime)],
                            capture_output=True, text=True, timeout=600)
    if result.returncode:
        # Error output is bounded; helper never prints process environments or credentials.
        raise ValueError('Local Hindsight daemon: ' + result.stderr[-2000:])
    return json.loads(result.stdout)


def provision(repo, checksum, runtime):
    return invoke(repo, 'setup', checksum, runtime)


def verify(repo, config, checksum, runtime):
    policy = settings(repo)
    if config['endpoint'] != f"http://127.0.0.1:{policy['port']}":
        raise ValueError('Memory endpoint does not match the approved local daemon')
    invoke(repo, 'verify', checksum, runtime)


def listener(policy):
    import psutil
    # lsof avoids macOS's privileged system-wide psutil connection inspection.
    found = subprocess.run(['lsof', '-nP', '-t', f"-iTCP:{policy['port']}",
                            '-sTCP:LISTEN'], capture_output=True, text=True, timeout=10)
    pids = {int(x) for x in found.stdout.split()}
    if not pids:
        return None
    if len(pids) != 1:
        raise ValueError('Ambiguous daemon listener; existing processes were preserved')
    process = psutil.Process(pids.pop())
    if process.uids().real != os.getuid() or not any('hindsight-api' in arg for arg in process.cmdline()):
        raise ValueError('Another process owns the daemon port; it was preserved')
    return process


def local(repo, action, checksum, runtime):
    from importlib.metadata import version
    from hindsight_embed.profile_manager import ProfileManager
    from hindsight_embed.daemon_embed_manager import DaemonEmbedManager
    policy = settings(repo)
    for package in ('hindsight-embed', 'hindsight-api'):
        if version(package) != policy['version']:
            raise ValueError('Embed/API version drift; reinstall the pinned tools')
    env, models = environment(policy, runtime, checksum)
    manager = ProfileManager()
    paths = manager.resolve_profile_paths(policy['profile']) if manager.profile_exists(policy['profile']) else None
    expected = {**env, 'HINDSIGHT_API_PORT': str(policy['port'])}
    if paths is not None:
        current = manager.load_profile_config(policy['profile'])
        if current != expected:
            raise ValueError('Existing daemon profile differs; preserve it and review the drift')
    elif action == 'setup':
        manager.create_profile(policy['profile'], policy['port'], env)
    else:
        raise ValueError('Daemon profile missing; rerun kit_memory.py setup')
    process = listener(policy)
    if action == 'setup' and process is None:
        from huggingface_hub import snapshot_download
        for name, model in runtime['models'].items():
            if not (models / name / model['file']).is_file():
                snapshot_download(repo_id=model['repository'], revision=model['revision'],
                                  local_dir=models / name,
                                  allow_patterns=[model['file'], 'config.json', 'tokenizer.json',
                                                  'tokenizer_config.json', 'special_tokens_map.json',
                                                  'sentencepiece.bpe.model', 'vocab.txt'])
        # Do not forward another project's Hindsight/provider settings to this daemon.
        for key in list(os.environ):
            if key.startswith('HINDSIGHT_'):
                del os.environ[key]
        os.environ.update(OMP_NUM_THREADS='2', HF_HUB_OFFLINE='1',
                          TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1', DO_NOT_TRACK='1')
        if not DaemonEmbedManager().ensure_running(env, policy['profile']):
            raise ValueError('Daemon failed to start; inspect hindsight-embed -p coding-agent daemon logs')
        process = listener(policy)
    if process is None:
        raise ValueError('Local daemon is stopped; run kit_memory.py setup to restart it')
    actual = process.environ()  # Never log the environment: it can contain credentials.
    if any(actual.get(key) != value for key, value in env.items()):
        raise ValueError('Running daemon provider/profile drift; do not infer policy from its config file')
    connections = process.net_connections(kind='tcp')
    bound = [c for c in connections if c.status == 'LISTEN' and c.laddr.port == policy['port']]
    if not bound or any(c.laddr.ip != '127.0.0.1' for c in bound):
        raise ValueError('Daemon must listen exclusively on IPv4 loopback')
    return {'deployment': 'hindsight-embed', 'profile': policy['profile'],
            'policy_sha256': checksum, 'endpoint': f"http://127.0.0.1:{policy['port']}",
            'version': policy['version'], 'service_llm_provider': policy['llm_provider'],
            'service_llm_model': policy['llm_model'], 'pid': process.pid}


if __name__ == '__main__':
    if sys.argv[1:] == ['test']:
        raise SystemExit(subprocess.call([str(interpreter()), '-m', 'unittest', 'discover',
                                          '-s', str(Path(__file__).resolve().parents[1] / 'tests'), '-v']))
    try:
        value = local(Path(sys.argv[2]), sys.argv[1], sys.argv[3], json.loads(sys.argv[4]))
        print(json.dumps(value))
    except Exception as exc:
        # Type and controlled message only; no locals, HTTP payloads or auth data.
        print(f'{type(exc).__name__}: {exc}', file=sys.stderr)
        raise SystemExit(1)
