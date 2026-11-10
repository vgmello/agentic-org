import base64
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile

if not __debug__:
    raise SystemExit("probe assertions must be enabled")

def run(args, **kwargs):
    return subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=25,
                          env={**os.environ, "LC_ALL": "C"}, **kwargs)


def require_failure(result, cause, label):
    diagnostic = result.stderr.decode().strip()
    assert result.returncode != 0, f"{label} unexpectedly succeeded"
    assert cause in diagnostic, f"{label} failed for an unrelated cause: {diagnostic}"
    print(label, "rc=", result.returncode, "diagnostic=", diagnostic)
    return diagnostic


wrong_name_cause = "Name in credential doesn't match expectations."
missing_file_cause = "Failed to read encrypted credential data: No such file or directory"
# Exercise the same failure-cause checker against an unrelated provider outage.
outage = subprocess.CompletedProcess([], 1, b"", b"credential broker unavailable")
for label, expected_cause in [("wrong-name", wrong_name_cause), ("missing-envelope", missing_file_cause)]:
    try:
        require_failure(outage, expected_cause, label)
    except AssertionError:
        pass
    else:
        raise AssertionError(f"{label} accepted an unrelated broker failure")
print("PASS unrelated broker failures do not satisfy credential rejection controls")

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
    nobody_secret = os.urandom(32)
    nobody_blob = run(["sudo", "-n", "-u", "nobody", "systemd-creds", "encrypt", "--user", "--with-key=host", "--name=dbkey", "-", "-"], input=nobody_secret)
    assert nobody_blob.returncode == 0, nobody_blob.stderr
    nobody_own = run(["sudo", "-n", "-u", "nobody", "systemd-creds", "decrypt", "--user", "--refuse-null", "--name=dbkey", "-", "-"], input=nobody_blob.stdout)
    assert nobody_own.returncode == 0 and nobody_own.stdout == nobody_secret, nobody_own.stderr
    print("PASS different UID encrypts and decrypts its own positive control")
    other_user = run(["sudo", "-n", "-u", "nobody", "systemd-creds", "decrypt", "--user", "--refuse-null", "--name=dbkey", str(blob), "-"])
    require_failure(other_user, "Bad credential format.", "different UID decrypt")
    print("PASS different UID cannot decrypt readable ciphertext")
    root.chmod(0o700)
    blob.chmod(0o600)
    require_failure(decrypt(blob, "other-instance"), wrong_name_cause, "wrong credential name")
    print("PASS wrong credential name refused")
    damaged = root / "damaged.cred"
    data = bytearray(base64.b64decode(blob.read_bytes()))
    data[-1] ^= 1
    damaged.write_bytes(base64.b64encode(data))
    corrupt = decrypt(damaged)
    require_failure(corrupt, "Bad credential format.", "corrupt credential")
    print("PASS corrupted credential refused")
    require_failure(decrypt(root / "missing.cred"), missing_file_cause, "missing credential")
    print("PASS missing credential refused")
    saved_blob = blob.read_bytes()
    root_control = run(["sudo", "-n", "systemd-creds", "decrypt", f"--uid={os.getuid()}", "--refuse-null", "--name=dbkey", str(blob), "-"])
    assert root_control.returncode == 0 and root_control.stdout == secret, root_control.stderr
    print("PASS root --uid decrypt positive control uses legitimate provider")
    for mode in ["missing", "wrong"]:
        isolated = root / f"provider-{mode}"
        isolated.mkdir()
        command = 'mount --bind "$1" /var/lib/systemd && if [ "$4" = wrong ]; then systemd-creds setup || exit; fi; exec systemd-creds decrypt --uid="$2" --refuse-null --name=dbkey "$3" -'
        outcome = run(["sudo", "-n", "unshare", "--mount", "--propagation", "private", "sh", "-ec", command, "sh", str(isolated), str(os.getuid()), str(blob), mode])
        cause = "Failed to determine local credential key: No such file or directory" if mode == "missing" else "Decryption failed (incorrect key?)"
        diagnostic = require_failure(outcome, cause, f"{mode} provider")
        if mode == "missing":
            assert not (isolated / "credential.secret").exists(), "decrypt created replacement provider key"
        else:
            assert (isolated / "credential.secret").exists(), "wrong provider setup did not finish"
            assert "Decryption failed" in diagnostic, "wrong provider decrypt was not reached"
        assert blob.read_bytes() == saved_blob
        assert decrypt(blob).stdout == secret
        print(f"PASS {mode} provider fails closed, unchanged credential recovers with legitimate provider")
    null_blob = root / "null.cred"
    null_result = run(["sudo", "-n", "systemd-creds", "encrypt", "--with-key=null", "--name=dbkey", "-", str(null_blob)], input=secret)
    assert null_result.returncode == 0, null_result.stderr
    null_check = run(["sudo", "-n", "systemd-creds", "decrypt", "--refuse-null", "--name=dbkey", str(null_blob), "-"])
    require_failure(null_check, "Credential uses null key, but that's not allowed, refusing.", "null-key rejection")
    print("PASS null-key envelope explicitly refused")
    # Distinct transient launches prove unattended load across service restarts.
    digest = hashlib.sha256(secret).hexdigest()
    script = f"import os,pathlib,hashlib; p=pathlib.Path(os.environ['CREDENTIALS_DIRECTORY'])/'dbkey'; assert hashlib.sha256(p.read_bytes()).hexdigest()=='{digest}'; print('PASS service received exact synthetic credential without logging it')"
    for i in range(2):
        result = run(["systemd-run", "--user", "--wait", "--pipe", "--collect", f"--unit=sage-b204-{os.getpid()}-{i}", f"--property=LoadCredentialEncrypted=dbkey:{blob}", "python3", "-I", "-c", script])
        print("service", i, "rc=", result.returncode, result.stdout.decode().strip(), result.stderr.decode().strip())
        assert result.returncode == 0
    result = run(["systemd-run", "--user", "--wait", "--pipe", "--collect", f"--unit=sage-b204-{os.getpid()}-bad", f"--property=LoadCredentialEncrypted=dbkey:{damaged}", "/usr/bin/true"])
    print("damaged service rc=", result.returncode)
    assert result.returncode == 243
    print("PASS damaged credential prevents service payload start (243); no fallback")
