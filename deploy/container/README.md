# Agentic Org in a container

The whole instance (the host, the web process and the offices) runs as the main process
of one container ([ADR-0017](../../docs/adr/0017-linux-and-container-hosting.md),
decisions 4 and 10; [Linux host design](../../docs/design/linux-host.md), section 3.4).

## Run it

Build the four executables for Linux, the UI and the image from the repository root, then
run it on a network of its own. Nothing else is needed: no file to write, no certificate
to make.

```sh
# The four executables, for Linux, into target/linux/debug. Always right before the image.
deploy/container/build.sh
(cd apps/agentic-org-ui && bun install --frozen-lockfile && bun run build)
docker build -f deploy/container/Dockerfile -t agentic-org .
docker network create agentic-org
docker run -d --name agentic-org --network agentic-org -p 127.0.0.1:8443:8443 agentic-org
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

**Every machine that can reach the listen address can reach sign-in and sessions in
plain text.** This applies to `http_published` on a bare macOS or Linux machine as well
as in a container. On a bare machine, `0.0.0.0` listens on all IPv4 interfaces, including
the LAN interface. In Docker, **publish it on `127.0.0.1` only**, as the command above
does. HTTPS is pending in X7. Setup and the host log the exposure at every start.

**Other containers reach the port whatever `-p` says.** `-p` decides what the Docker host
publishes; a container on the same Docker network connects to the instance's own address
and port 8443 directly, unencrypted, and the web process's address checks stop browsers,
not those programs. The command above gives the instance a network of its own
(`--network agentic-org`), so that at least no container started without a network (on
Docker's default bridge) shares one with it. Whether a container on another network can
still reach a published container port depends on the engine: measured for this build,
Docker Engine 29.1.3 on Linux isolates the networks (no route), and Docker Desktop 29.9.3
on a Mac does not (a container on any network reaches it). LNX-2 reports which case its
engine is. **Until HTTPS (X7), every container of the Docker host is inside the boundary
of the sign-in: run the instance on a Docker host whose other containers you trust with
the administrator's password and sessions, and attach nothing else to its network.**

The address is exactly `http://127.0.0.1:8443`: keep the port 8443 on both sides of `-p`.
The web process answers only requests for that origin; `http://localhost:8443`,
`http://[::1]:8443`, another port or the machine's name in the address bar get `400`, and
the setup file accepts no other origin. It is the address and not the name `localhost` on
purpose: a name can resolve to `::1`, where the port is not published and another program
of the machine could listen. That is the same check as on a Mac, not a loosening: the
listener is on every address of the container only because a published port arrives on
the container's network interface, never on its loopback.

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
docker run -d --name agentic-org --network agentic-org -p 127.0.0.1:8443:8443 \
  -v agentic-org-data:/data -v agentic-org-secrets:/secrets agentic-org
```

| Volume | Holds |
| --- | --- |
| `/data` | Everything the instance keeps that is not a secret: the service user's home is `/data`, the instance's data folder is `/data/instance`, its records are in `/data/.local/share/agentic-org` and its sockets in `/data/.cache/agentic-org/tmp`. |
| `/secrets` | The host's keys and its instance marker, owned by the service user, with folders at mode `700` and files at mode `600`. A copy of the data volume holds no key. |

A new container on the same two volumes is the same instance with the same keys.
On the first start the host writes `.instance` in the secrets volume with the
instance ID. A new data volume paired with an already bound secrets volume is refused
before setup reserves an instance. A recorded instance must match the marker before
the host opens its keys. A volume with existing credential files and no marker is
also refused because its owner cannot be established. Keep the original volumes together
and use an unused secrets volume for a new instance.

A secrets volume written by a development build from before the marker existed holds
keys and no `.instance`, so it is refused this way. No release wrote one. To keep such a
volume with the data volume it belongs to, write the instance ID from the setup record
(`instance_id` in `/data/.local/share/agentic-org/setup/default/setup.v1.json`) into
`/secrets/.instance` as the service user, with mode `600` and nothing else in it (no newline), while the
container is stopped.

Once the instance has an office, a **new, empty** secrets volume is refused with
"credentials missing". The new volume stays empty. Restore the matching secrets volume
to recover the keys.

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
then hold the keys. It is refused, and so is a secrets folder in the data volume when the
data folder `/data/instance` is a volume of its own mounted inside it: the instance's
records are in `/data` too ("the credential directory is in the volume that holds the
account's records"). Mount nothing inside `/secrets` either: a start with anything mounted
inside it, at any depth, is refused, naming the mount point. And mount the secrets volume
once, at `/secrets` only: the same volume, or a `volume-subpath` of it, mounted anywhere
else in the container (`-v S:/secrets -v S:/data/extra`) is refused, naming that other
mount point ("the secrets volume is also mounted here").

A first start that is refused before the instance is reserved saves no setup record,
reserves nothing and writes nothing in `/secrets`. It leaves only empty folders and an empty lock
file of the service user: `/data/.local/share/agentic-org` (an empty `locator` folder and
`setup/default/setup.lock`) and `/data/.cache/agentic-org/tmp`. A corrected start uses
them. Correct the mistake and start again. Once an instance is set up, its
configuration does not change.

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

- `seccomp.json`: Docker's default profile (moby v28.0.0) with four rules added at its
  end: `unshare` and `clone` (both argument orders) are allowed when `CLONE_NEWUSER` is
  among their flags, and `mount`, `umount2` and `pivot_root` are allowed.
- `apparmor-agentic-org`: `docker-default` plus `userns`, and `mount` and `pivot_root`
  allowed in place of `docker-default`'s `deny mount`, for a Docker host that has
  AppArmor. Load it with `sudo apparmor_parser -r -W deploy/container/apparmor-agentic-org`.
  Docker's own profile lets the Docker daemon signal the container under whatever
  AppArmor label the daemon runs with; this file is static, so it admits an unconfined
  daemon and the Docker snap's (`snap.docker.dockerd`). Where `dockerd` runs under another
  label (`cat /proc/$(pidof dockerd)/attr/current`), add
  `signal (receive) peer=<that label>,` beside those lines before loading it, or
  `docker kill` cannot reach the service.

```sh
docker run -d --name agentic-org --network agentic-org -p 127.0.0.1:8443:8443 \
  --security-opt seccomp=deploy/container/seccomp.json \
  --security-opt apparmor=agentic-org agentic-org
```

What they allow, and why it stays sound: a sandbox like bubblewrap creates a user
namespace and then mounts in the mount namespace it made (a private `/`, `/dev`, a `tmpfs`,
then `pivot_root`). Neither profile lets the container itself mount anything: the kernel
admits a mount only to a process with `CAP_SYS_ADMIN` over the mount namespace, which a
container process has only in a user namespace it created (Docker drops `CAP_SYS_ADMIN`;
never add it back), and there only for the filesystems a user namespace may mount. Every
other denial of Docker's profiles stays. The cost is kernel surface: a process of the
container can then use the mount code of those filesystems (overlayfs, tmpfs, bind mounts,
devpts) inside its own namespaces, where container escapes have been found before
(CVE-2023-0386, CVE-2023-2640 and CVE-2023-32629 in overlayfs). Use the profiles only on a
Docker host whose kernel is kept patched, and only for agents that need such a sandbox.

Verified on Ubuntu 26.04 (AppArmor enforcing, Docker 29.1.3) by
`deploy/container/profiles-check.sh` (LNX-2 step 10, `LNX2_PROFILES=1`): under both
profiles the service serves and stops within 2.5 s with exit code 0, and
`bwrap --unshare-all --ro-bind / / --dev /dev --tmpfs /tmp true` runs in the container as
the service user; with Docker's default profiles it cannot create its namespace.

**A sandbox that mounts a fresh `/proc` does not run in the container** (`bwrap ... --proc
/proc`: `Can't mount proc on /proc: Operation not permitted`). The kernel refuses a new
`/proc` while Docker masks paths of the container's own, and those masks stay: never add
`--security-opt systempaths=unconfined`. With the paths unmasked, root in the container
could bind `/proc/sys` to another place in a mount namespace of its own and write kernel
settings there, where the AppArmor path rules do not look. A sandbox that binds the
container's `/proc` (`--ro-bind /proc /proc`) is not affected. `profiles-check.sh` checks
that neither the service user nor root in the container can mount outside a user
namespace, nor write `/proc/sys` through such a mount.

Leave `--security-opt apparmor=…` out on a Docker host without AppArmor. Never use
`seccomp=unconfined`, `apparmor=unconfined`, `systempaths=unconfined`, `--privileged` or
`--cap-add SYS_ADMIN`. The image does not include bubblewrap or any vendor tool's other
packages.

## Optional: less for the container to hold

The service needs no capability and writes only to its two volumes, so the container
also runs with a read-only root filesystem, without new privileges and with every
capability dropped but the three that root's `admin reset` uses inside it (to read the
service user's folders as that user, and to reach its socket):

```sh
docker run -d --name agentic-org --network agentic-org -p 127.0.0.1:8443:8443 \
  --read-only --security-opt no-new-privileges \
  --cap-drop ALL --cap-add SETUID --cap-add SETGID --cap-add DAC_OVERRIDE \
  -v agentic-org-data:/data -v agentic-org-secrets:/secrets agentic-org
```

The hardened run serves HTTP, prints root's registration link at a terminal, and
stops with exit code 0 within 2.5 seconds. With `--cap-drop ALL` alone the service
still serves HTTP. Root's reset at a terminal returns "acting as the service user:
Operation not permitted" because `docker exec` cannot add a dropped capability.

## The setup file

`/etc/agentic-org/setup.json` is the configuration of non-interactive setup, one strict
JSON object (an unknown property is refused):

| Property | Meaning |
| --- | --- |
| `version` | `1`. |
| `service` | `"foreground"`: nothing is registered; the container runtime is the supervisor. (`"registered"` is the account's service manager, on a machine.) |
| `root` | The instance's data folder, absolute. |
| `credentials` | The secrets directory, absolute: a mounted volume that setup never makes. |
| `listen` | `address`, `port`, `scheme`, `origin`, `hsts` (`"off"`, `"on"`, `"clear"`). The image's scheme is `"http_published"`: plain HTTP on an address that is not loopback, accepted only with `"service": "foreground"` and only with `127.0.0.1` as the origin's host (`http://127.0.0.1:<port>`): not `localhost`, which can resolve to `::1`, and not `[::1]`. The other is `"http_loopback"` (exactly `127.0.0.1` or `::1`). `"https"` is refused: no build serves it yet. |

The instance's configuration does not change after its first start: a container whose
file differs from what its volumes were set up with refuses to start.

## A development image

The image holds the workspace's own Linux build (`--build-arg BIN=target/linux/debug`, the
default). The host logs that the office worker has no publisher check. It keeps its keys
exactly as a release build does: in `/secrets`, the credential directory its setup record
names, checked at every start by every rule above (a lost secrets volume included), in
`/secrets/office-keys` and `/secrets/host-secrets`. It uses no development key folder: the
image sets no `ORG_OFFICE_DEV_KEY_DIR`, and one given with `-e` is not used.

### What a release image still needs

- A signed release from the release pipeline, installed under `/opt/agentic-org`, and a
  release build that carries its pinned runtime release (setup refuses without one).
- HTTPS: the host's certificate for the listener (making one at first start, or
  verifying a supplied pair) and the host's web start for an HTTPS listener. Until then
  the container serves plain HTTP for a port published on the Docker host's loopback.

## Known limits

- A first start that is refused before the instance is reserved saves no setup record,
  reserves nothing and writes nothing in `/secrets`. It leaves only empty folders and an empty
  lock file of the service user: `/data/.local/share/agentic-org` (an empty `locator`
  folder and `setup/default/setup.lock`) and `/data/.cache/agentic-org/tmp`. A corrected start uses
  them. A
  registered-mode setup (on a machine, not this image) that fails later, when its service
  is registered, keeps its record: correct the cause and run the same file again; another
  file is refused until the instance is removed.
- The image is a development image (above): the worker has no publisher check.
- Until HTTPS, other containers of the Docker host can reach its plain-HTTP port: on its
  network always, and on Docker Desktop from every network (above).
- HTTPS is not available; the container serves plain HTTP for a port published on the
  Docker host's `127.0.0.1`.

## The test (LNX-2)

`deploy/container/lnx2.sh` builds the image and runs it in real containers. It checks
registration and sign-in, the web process's confinement, and the administrator creating
the organization through the web API: the host starts the HQ's office worker under the
writer custody it reserved, and the worker serves its socket again after a restart and
in a replacement container. (`agent-os` reaches the office again once an agent is placed
in it and `agent-os` addresses a selected office; the send and read return with B6.) It checks the private modes in `/secrets`, including a
mode-`600` recovery credential created by the host under `/secrets/host-secrets` after
organization creation is requested. It verifies that writes stay within the volumes,
a replacement container keeps its state, and that empty or mismatched secrets volumes
are refused without changing the original credentials. It also checks HTTP, root's
reset and clean shutdown in the hardened run, plus root's refusal with all capabilities
dropped. The network check reports which containers can reach the HTTP port.

Set `LNX2_PROFILES=1` on a Docker host that enforces AppArmor with the profile loaded
to also run `profiles-check.sh` (step 10). The script builds the Linux executables with
`build.sh`. It needs the UI built, Docker Engine 28 or
later, and `curl`.
