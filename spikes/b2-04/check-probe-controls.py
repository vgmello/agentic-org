import os
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise SystemExit(message)


for probe in sorted(root.glob("linux-*.py")):
    result = subprocess.run([sys.executable, "-O", str(probe)], capture_output=True, text=True, timeout=10)
    require(result.returncode != 0 and "probe assertions must be enabled" in result.stderr,
            f"optimized probe did not refuse before execution: {probe.name}")
print("PASS all Linux probes refuse optimized execution before state changes")

environment = {key: value for key, value in os.environ.items() if key != "B204_DISPOSABLE_VM"}
result = subprocess.run(["bash", str(root / "run-linux.sh")], env=environment, capture_output=True, text=True, timeout=10)
require(result.returncode == 2 and "Refusing persistent" in result.stderr,
        "manual runner did not refuse non-disposable invocation")
print("PASS manual runner requires disposable VM acknowledgement before state changes")

# Mutation control: bypass the actual admission decision, keeping the changed ELF
# and the rest of the launch path intact. The negative launch must catch this.
source = (root / "linux-exec.py").read_text()
decision = "admitted = hashlib.sha256(actual.read_bytes()).digest() == trusted_digest"
require(source.count(decision) == 1, "admission decision changed; update the mutation control")
with tempfile.TemporaryDirectory(prefix="sage-b204-mutation-") as tmp:
    directory = Path(tmp)
    (directory / "linux-bin").symlink_to(root / "linux-bin", target_is_directory=True)
    mutation = directory / "linux-exec.py"
    mutation.write_text(source.replace(decision, "admitted = True"))
    result = subprocess.run([sys.executable, "-I", str(mutation)], capture_output=True, text=True, timeout=20)
    require(result.returncode != 0 and "assert not accepted and sent == 0" in result.stderr,
            "digest-admission bypass was not caught by the changed-image regression")
    require("PASS modified executable rejected" not in result.stdout,
            "mutation falsely reported executable rejection")
print("PASS bypassing real digest admission is caught by the changed-executable regression")
