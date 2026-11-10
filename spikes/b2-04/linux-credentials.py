import base64
import os
from pathlib import Path
import subprocess
import tempfile

def run(args, **kwargs):
    return subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=25, **kwargs)

with tempfile.TemporaryDirectory(prefix="sage-b204-creds-") as tmp:
    root = Path(tmp)
    root.chmod(0o700)
    # Synthetic credential, generated inside this disposable probe only.
    secret = os.urandom(32)
    blob = root / "dbkey.cred"
    encrypted = run(["systemd-creds", "encrypt", "--user", "--with-key=host", "--name=dbkey", "-", str(blob)], input=secret)
    print("user encrypt rc=", encrypted.returncode, "diagnostic=", encrypted.stderr.decode().strip())
    if encrypted.returncode:
        raise SystemExit(1)
    blob.chmod(0o600)
    def decrypt(path, name="dbkey"):
        return run(["systemd-creds", "decrypt", "--user", "--refuse-null", f"--name={name}", str(path), "-"])
    assert decrypt(blob).stdout == secret
    print("PASS user credential roundtrip, ciphertext on disk")
    print("LIMIT same-UID caller can decrypt raw credential too; no host/gateway identity separation")
    root.chmod(0o755)
    blob.chmod(0o644)  # Ciphertext only, to test identity binding rather than file permissions.
    other_user = run(["sudo", "-n", "-u", "nobody", "systemd-creds", "decrypt", "--user", "--refuse-null", "--name=dbkey", str(blob), "-"])
    print("different UID decrypt rc=", other_user.returncode, "diagnostic=", other_user.stderr.decode().strip())
    assert other_user.returncode != 0
    assert "sudo:" not in other_user.stderr.decode(), "identity test was not executed"
    print("PASS different UID cannot decrypt readable ciphertext")
    root.chmod(0o700)
    blob.chmod(0o600)
    assert decrypt(blob, "other-instance").returncode != 0
    print("PASS wrong credential name refused")
    damaged = root / "damaged.cred"
    data = bytearray(base64.b64decode(blob.read_bytes()))
    data[-1] ^= 1
    damaged.write_bytes(base64.b64encode(data))
    assert decrypt(damaged).returncode != 0
    print("PASS corrupted credential refused")
    assert decrypt(root / "missing.cred").returncode != 0
    print("PASS missing credential refused")
    null_blob = root / "null.cred"
    null_result = run(["sudo", "-n", "systemd-creds", "encrypt", "--with-key=null", "--name=dbkey", "-", str(null_blob)], input=secret)
    assert null_result.returncode == 0, null_result.stderr
    null_check = run(["sudo", "-n", "systemd-creds", "decrypt", "--refuse-null", "--name=dbkey", str(null_blob), "-"])
    assert null_check.returncode != 0
    print("null-key rejection diagnostic=", null_check.stderr.decode().strip())
    print("PASS null-key envelope explicitly refused")
    # Distinct transient launches prove unattended load across service restarts.
    script = "import os,pathlib; p=pathlib.Path(os.environ['CREDENTIALS_DIRECTORY'])/'dbkey'; assert len(p.read_bytes())==32; print('PASS service read 32 bytes without logging them')"
    for i in range(2):
        result = run(["systemd-run", "--user", "--wait", "--pipe", "--collect", f"--unit=sage-b204-{os.getpid()}-{i}", f"--property=LoadCredentialEncrypted=dbkey:{blob}", "python3", "-c", script])
        print("service", i, "rc=", result.returncode, result.stdout.decode().strip(), result.stderr.decode().strip())
        assert result.returncode == 0
    result = run(["systemd-run", "--user", "--wait", "--pipe", "--collect", f"--unit=sage-b204-{os.getpid()}-bad", f"--property=LoadCredentialEncrypted=dbkey:{damaged}", "/usr/bin/true"])
    print("damaged service rc=", result.returncode)
    assert result.returncode != 0
    print("PASS invalid provider data prevents service start; no fallback")
