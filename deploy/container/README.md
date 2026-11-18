# Agentic Org in a container

The whole instance (the host, the web process and the offices) runs as the main process
of one container ([ADR-0017](../../docs/adr/0017-linux-and-container-hosting.md),
decisions 4 and 10; [Linux host design](../../docs/design/linux-host.md), section 3.4).

## Run it

Build the four executables and the image from the repository root, then run it. Nothing
else is needed: no option, no file to write, no certificate to make.

```sh
cargo build --locked -p agentic-org -p agentic-org-office -p agentic-org-web -p agent-os
docker build -f deploy/container/Dockerfile -t agentic-org .
docker run -d --name agentic-org -p 127.0.0.1:8443:8443 agentic-org
```

At its first start the container sets the instance up from
`/etc/agentic-org/setup.json` (`agentic-org serve --setup`), then serves it in the
foreground. `docker logs agentic-org` shows what setup did. `docker stop` ends it within
a second or two; the runtime's default grace of 10 seconds is never needed.

**State of this build.** The host, the offices and `agent-os` work in the container.
The browser address (`https://localhost:8443`) is **not served yet**: the host can
neither make nor verify the listener's certificate, and its web start serves loopback
HTTP only. Setup says so in the log ("Browser access: not available yet"). A
configuration that names a certificate pair is refused, never stored unverified.

## Keep the instance: two named volumes

The image declares two volumes. Without `-v`, Docker gives the container two anonymous
volumes, which is enough to try it and is lost with the container. To keep an instance
across a replaced container, name them:

```sh
docker run -d --name agentic-org -p 127.0.0.1:8443:8443 \
  -v agentic-org-data:/data -v agentic-org-secrets:/secrets agentic-org
```

| Volume | Holds |
| --- | --- |
| `/data` | Everything the instance keeps that is not a secret: the service user's home is `/data`, the instance's data folder is `/data/instance`, its records are in `/data/.local/share/agentic-org` and its sockets in `/data/.cache/agentic-org/tmp`. |
| `/secrets` | The host's keys, owned by the service user, folders `700` and files `600`. Never in `/data`, so a copy of the data volume holds no key. |

A new container on the same two volumes is the same instance with the same keys. A new
container on the data volume with a **new, empty** secrets volume does not make new keys:
the offices whose keys are gone are not started.

The instance's data folder is `/data/instance`, not `/data`: the instance's records must
be in the volume too, and the product refuses a data folder that contains them.

## The first registration link

Setup prints no link and nothing secret reaches the container's log. The administrator's
registration link comes from root at a terminal, naming the service user:

```sh
docker exec -it -u 0 agentic-org agentic-org admin reset --user agentic
```

Root is admitted on the instance's socket for this one call. The same command without a
terminal (`-t`), as the service user, or as any other user prints no link.

## What the secrets volume must be

The credential store checks its directory at every start and refuses, naming the
directory and the rule, when:

- it is not a mount point: a plain directory of the image (overlayfs) or a plain
  directory inside another volume. The image's declared volumes are mounts, named or not;
- its root is not owned by the service user (UID 61000) with mode `0700` (a `lost+found`
  in the volume's root is tolerated). A bind mount of a host folder must be prepared so:
  `install -d -o 61000 -g 61000 -m 0700 <folder>`;
- its filesystem is not ext4, XFS, Btrfs or F2FS. Refused by design: overlayfs, tmpfs
  (Docker and Kubernetes secrets), NFS and other network filesystems, FUSE and virtiofs
  (a bind mount of a macOS folder through Docker Desktop), ZFS, bcachefs and ecryptfs;
- a directory above it is writable by others, or is reached through a symbolic link (a
  symlinked `/home`, as on ostree systems, for a bind mount below it);
- the Docker host's kernel is older than 5.8: the mount-point rule needs `statx`'s
  mount-root attribute.

Named and anonymous Docker volumes live on the Docker host's own filesystem (ext4 on a
default Linux host and inside Docker Desktop's virtual machine on a Mac) and pass.

Do not place the secrets folder inside the folder that is mounted as `/data`
(`-v ./agentic:/data -v ./agentic/secrets:/secrets`): a copy of the data folder would
then hold the keys.

## Who can reach the volumes

The service runs as UID and GID 61000 (`agentic`), never as root, and the release under
`/opt/agentic-org` is root-owned. On a Linux Docker host without user-namespace
remapping, a host process with UID 61000 can reach the volumes; on a shared host enable
Docker's `userns-remap` or rootless mode.

## Optional: profiles for agents' sandboxes

The container runs with Docker's default seccomp and AppArmor profiles. Two files here
are **optional** and exist for one purpose: an agent's vendor tool whose own sandbox
creates a user namespace, which Docker's default seccomp profile refuses and which
Ubuntu 24.04 and later also restrict through AppArmor.

- `seccomp.json`: Docker's default profile (moby v28.0.0) with three rules added at its
  end: `unshare` and `clone` (both argument orders) are allowed when `CLONE_NEWUSER` is
  among their flags.
- `apparmor-agentic-org`: `docker-default` plus `userns`, for a Docker host that has
  AppArmor. Load it with `sudo apparmor_parser -r -W deploy/container/apparmor-agentic-org`.

```sh
docker run -d --name agentic-org -p 127.0.0.1:8443:8443 \
  --security-opt seccomp=deploy/container/seccomp.json \
  --security-opt apparmor=agentic-org agentic-org
```

Leave `--security-opt apparmor=…` out on a Docker host without AppArmor. Never use
`seccomp=unconfined`, `apparmor=unconfined` or `--privileged`.

Without them the host, the web process's confinement, the offices and `agent-os` all
work. What does not work is creating a user namespace inside the container, so an agent
whose vendor sandbox needs one is not started. Whether a given vendor sandbox needs more
than its user namespace (mounts inside it) is not known until the runtime adapters exist.

## The setup file

`/etc/agentic-org/setup.json` is the configuration of non-interactive setup, one strict
JSON object (an unknown property is refused):

| Property | Meaning |
| --- | --- |
| `version` | `1`. |
| `service` | `"foreground"`: nothing is registered; the container runtime is the supervisor. (`"registered"` is the account's service manager, on a machine.) |
| `root` | The instance's data folder, absolute. |
| `credentials` | The secrets directory, absolute: a mounted volume that setup never makes. |
| `listen` | `address`, `port`, `scheme` (`"https"` or `"http_loopback"`), `origin`, `hsts` (`"off"`, `"on"`, `"clear"`). |
| `certificate` | Optional, HTTPS only: `chain` and `key`, absolute paths. Refused by this build. |

The instance's configuration does not change after its first start: a container whose
file differs from what its volumes were set up with refuses to start.

## A development image

The image holds the workspace's own build output (`--build-arg BIN=target/debug`, the
default). The host logs that the office worker has no publisher check, and it keeps its
keys in its development key store, `/secrets/dev-keys`, not in the release store: the
release store's own start-time refusals (an empty store where keys are expected) are
not exercised by this image, though setup checks `/secrets` by the release store's rules.

### What a release image still needs

- A signed release from the release pipeline, installed under `/opt/agentic-org`, and a
  release build that carries its pinned runtime release (setup refuses without one).
- The host's certificate for the HTTPS listener: making one at first start, or verifying
  a supplied pair; and the host's web start for an HTTPS listener.
- `ORG_OFFICE_DEV_KEY_DIR` removed: a release build keeps its keys in `/secrets` itself.

## The test (LNX-2)

`deploy/container/lnx2.sh` builds the image and checks all of the above in real
containers: the plain run, the link, a worker with an `agent-os` send and read, the modes
in `/secrets`, that nothing is written outside the volumes, the stop, the replaced
container, the empty secrets volume, anonymous volumes, and the store's refusals. It
needs the workspace built, including `cargo build -p agentic-org --example lnx2-office`.
