# Agentic Org in a container

The whole instance (the host, the web process and the offices) runs as the main process
of one container ([ADR-0017](../../docs/adr/0017-linux-and-container-hosting.md),
decisions 4 and 10; [Linux host design](../../docs/design/linux-host.md), section 3.4).

## Run it

Build the four executables for Linux, the UI and the image from the repository root, then
run it. Nothing else is needed: no option, no file to write, no certificate to make.

```sh
# The four executables, for Linux, into target/linux/debug. Always right before the image.
deploy/container/build.sh
(cd apps/agentic-org-ui && bun install --frozen-lockfile && bun run build)
docker build -f deploy/container/Dockerfile -t agentic-org .
docker run -d --name agentic-org -p 127.0.0.1:8443:8443 agentic-org
```

`deploy/container/build.sh` runs `cargo build` in the Rust toolchain's own Debian 13 image
(pinned, like the image's base), on a Mac as on a Linux machine: a `cargo build` on a Mac
makes macOS programs, which the container cannot run, and one on another Linux
distribution may need a newer C library than the image has. Docker is all it needs; its
output and its crate downloads are in `target/linux`. `--release` builds into
`target/linux/release` (then `docker build --build-arg BIN=target/linux/release …`).

**Docker Engine 28 or later.** Before Engine 28, a port published on `127.0.0.1` of a
Linux Docker host could be reached by other machines of the same local network (Docker's
own advisory), so the `127.0.0.1` below would not keep the plain-HTTP port to this
machine. Docker Desktop on a Mac has a current engine.

Then open **`http://127.0.0.1:8443`** in a browser on the Docker host.

**The container serves plain HTTP. Publish it on `127.0.0.1` only**, as the command above
does. Published on another interface (`-p 8443:8443`, `-p 0.0.0.0:8443:8443`), sign-in and
every session travel unencrypted on that network. HTTPS is not available yet. Setup and
the host say the same in the log at every start ("serving plain HTTP on a published
port").

The address is exactly `http://127.0.0.1:8443`: keep the port 8443 on both sides of `-p`.
The web process answers only requests for that origin; `http://localhost:8443`, another
port or the machine's name in the address bar get `400`. It is the address and not the
name `localhost` on purpose: a name can resolve to `::1`, where the port is not published
and another program of the machine could listen. That is the same check as on a
Mac, not a loosening: the listener is on every address of the container only because a
published port arrives on the container's network interface, never on its loopback.

At its first start the container sets the instance up from
`/etc/agentic-org/setup.json` (`agentic-org serve --setup`), then serves it in the
foreground. `docker logs agentic-org` shows what setup did. `docker stop` ends it with
exit code 0, normally within a second; a worker that does not stop when asked is killed
after 1.5 seconds, so the service always ends within 2.5 seconds, inside the shortest
default grace of a container runtime (Docker Desktop's 3 seconds; Docker Engine's is 10).

**State of this build.** The host, the web process (confined as on a Linux machine: its
seccomp filter, and Landlock where the kernel has it), the offices and `agent-os` work in
the container. The port answers from the first start, before any office exists.

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
registration link comes only from root at a terminal, naming the service user (the
instance refuses to give its first link to anyone else):

```sh
docker exec -it -u 0 agentic-org agentic-org admin reset --user agentic
```

Root is admitted on the instance's socket for this one call. The same command without a
terminal (`-t`), as the service user, or as any other user prints no link.

## What the secrets volume must be

The host checks the secrets directory at every start, before it reads or writes a key,
and refuses to start, naming the directory and the rule, when:

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

A first start that is refused leaves nothing in the data volume: correct the mistake and
start again. Once an instance is set up, its configuration does not change.

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

## Optional: less for the container to hold

The service needs no capability and writes only to its two volumes, so the container
also runs with a read-only root filesystem, without new privileges and with every
capability dropped but the three that root's `admin reset` uses inside it (to read the
service user's folders as that user, and to reach its socket):

```sh
docker run -d --name agentic-org -p 127.0.0.1:8443:8443 \
  --read-only --security-opt no-new-privileges \
  --cap-drop ALL --cap-add SETUID --cap-add SETGID --cap-add DAC_OVERRIDE \
  -v agentic-org-data:/data -v agentic-org-secrets:/secrets agentic-org
```

With `--cap-drop ALL` alone the service runs the same, but
`docker exec -u 0 … admin reset --user agentic` is refused ("acting as the service user:
Operation not permitted"), and `docker exec` cannot add a capability back.

## The setup file

`/etc/agentic-org/setup.json` is the configuration of non-interactive setup, one strict
JSON object (an unknown property is refused):

| Property | Meaning |
| --- | --- |
| `version` | `1`. |
| `service` | `"foreground"`: nothing is registered; the container runtime is the supervisor. (`"registered"` is the account's service manager, on a machine.) |
| `root` | The instance's data folder, absolute. |
| `credentials` | The secrets directory, absolute: a mounted volume that setup never makes. |
| `listen` | `address`, `port`, `scheme`, `origin`, `hsts` (`"off"`, `"on"`, `"clear"`). The image's scheme is `"http_published"`: plain HTTP on an address that is not loopback, accepted only with `"service": "foreground"` and only with `127.0.0.1`, `[::1]` or `localhost` as the origin's host. The other is `"http_loopback"` (exactly `127.0.0.1` or `::1`). `"https"` is refused: no build serves it yet. |

The instance's configuration does not change after its first start: a container whose
file differs from what its volumes were set up with refuses to start.

## A development image

The image holds the workspace's own Linux build (`--build-arg BIN=target/linux/debug`, the
default). The host logs that the office worker has no publisher check, and it keeps its
keys in its development key store, `/secrets/dev-keys`, not in the release store.

What the every-start check covers in this image: where `/secrets` is (a mount point, its
owner and mode, its filesystem, apart from the data folder). What it does not cover: which
keys are there. The release store refuses a start whose keys are missing; the development
store does not, so a new, empty secrets volume on an existing data volume starts: no key
is made in place of a lost one, and each office whose key is gone is reported in the log
("has no usable key") and not started.

### What a release image still needs

- A signed release from the release pipeline, installed under `/opt/agentic-org`, and a
  release build that carries its pinned runtime release (setup refuses without one).
- HTTPS: the host's certificate for the listener (making one at first start, or
  verifying a supplied pair) and the host's web start for an HTTPS listener. Until then
  the container serves plain HTTP for a port published on the Docker host's loopback.
- `ORG_OFFICE_DEV_KEY_DIR` removed: a release build keeps its keys in `/secrets` itself.

## Known limits

- A first start that is refused before the instance is reserved leaves nothing. A
  registered-mode setup (on a machine, not this image) that fails later, when its service
  is registered, keeps its record: correct the cause and run the same file again; another
  file is refused until the instance is removed.
- The image is a development image (above): a lost secrets volume is not refused at
  start, and the keys are in the development store.
- HTTPS is not available; the container serves plain HTTP for a port published on the
  Docker host's `127.0.0.1`.

## The test (LNX-2)

`deploy/container/lnx2.sh` builds the image and checks all of the above in real
containers: the plain run, the link, a worker with an `agent-os` send and read, the
browser address over plain HTTP from the Docker host (the API, the UI's index, `400` for
any other Host, the web process's confinement in the host log), the modes in `/secrets`,
that nothing is written outside the volumes, the stop, the replaced container, the empty
secrets volume, anonymous volumes, and the store's refusals. It builds the executables and
its own office program for Linux itself (`build.sh`); it needs the UI built, Docker
Engine 28 or later and `curl`.
