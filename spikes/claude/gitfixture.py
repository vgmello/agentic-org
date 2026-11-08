"""Git layout for items 8 and 9: a bare work.git with a linked worktree, and an agent home inside a repo.

build(B) creates, under the directory B (use a realpath; /tmp is a symlink):
  origin/            a normal repo (main, one commit)
  work.git/          bare clone, extensions.worktreeConfig=true, core.bare moved to config.worktree
  worktrees/t1/      `git worktree add` from work.git, branch task/t1  <- the session's cwd
  homerepo/          a normal repo; homerepo/agents/x/ is an agent home inside it
  rt/                office runtime dir: gitconfig (GIT_CONFIG_GLOBAL), empty hooks/
and returns the git env the office would give the agent, plus a few facts about the setup.
"""
import os
import subprocess

import lib


def git_env(B: str) -> dict:
    return {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": f"{B}/rt/gitconfig", "GIT_CONFIG_COUNT": "4",
            "GIT_CONFIG_KEY_0": "core.hooksPath", "GIT_CONFIG_VALUE_0": f"{B}/rt/hooks",
            "GIT_CONFIG_KEY_1": "core.fsmonitor", "GIT_CONFIG_VALUE_1": "false",
            "GIT_CONFIG_KEY_2": "gc.auto", "GIT_CONFIG_VALUE_2": "0",
            "GIT_CONFIG_KEY_3": "maintenance.auto", "GIT_CONFIG_VALUE_3": "false"}


def build(B: str) -> dict:
    P = lambda *x: os.path.join(B, *x)  # noqa: E731
    for d in ("rt/hooks", "run", "homerepo/agents/x"):
        os.makedirs(P(d), exist_ok=True)
    with open(P("rt/gitconfig"), "w") as f:
        f.write("[user]\n\tname = Spike Agent\n\temail = agent@office.invalid\n")
    env = git_env(B)

    def git(*args):
        r = subprocess.run(["git", *args], env={"PATH": lib.PATH, "HOME": lib.HOME, **env},
                           capture_output=True, text=True)
        if r.returncode:
            raise RuntimeError(f"git {args}: {r.stderr}")
        return r.stdout.strip()

    git("init", "-q", "-b", "main", P("origin"))
    with open(P("origin/README"), "w") as f:
        f.write("hi\n")
    git("-C", P("origin"), "add", "README")
    git("-C", P("origin"), "commit", "-q", "-m", "init")
    git("clone", "-q", "--bare", P("origin"), P("work.git"))
    git("-C", P("work.git"), "config", "extensions.worktreeConfig", "true")
    git("-C", P("work.git"), "config", "--unset", "core.bare")
    git("-C", P("work.git"), "config", "--worktree", "core.bare", "true")
    git("-C", P("work.git"), "worktree", "add", "-q", "-b", "task/t1", P("worktrees/t1"), "main")
    git("init", "-q", "-b", "main", P("homerepo"))
    with open(P("homerepo/agents/x/memory.md"), "w") as f:
        f.write("mem\n")
    git("-C", P("homerepo"), "add", ".")
    git("-C", P("homerepo"), "commit", "-q", "-m", "init")
    wt = P("worktrees/t1")
    return {"env": env, "wt": wt,
            "setup": {"t1_status_ok": git("-C", wt, "rev-parse", "--is-inside-work-tree") == "true",
                      "work_git_is_bare": git("-C", P("work.git"), "rev-parse", "--is-bare-repository"),
                      "t1_git_dir": git("-C", wt, "rev-parse", "--git-dir").replace(B, "<s8>")}}
