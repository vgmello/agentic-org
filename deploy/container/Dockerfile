# Agentic Org as the main process of a container (ADR-0017 decisions 4 and 10, LNX-2).
#
# Build, from the repository root, after building the four executables for Linux and the UI:
#
#   deploy/container/build.sh
#   (cd apps/agentic-org-ui && bun install --frozen-lockfile && bun run build)
#   docker build -f deploy/container/Dockerfile -t agentic-org .
#
# build.sh builds them in the Rust toolchain's Debian image, into target/linux/debug, on a
# Mac as on a Linux machine: a plain `cargo build` on a Mac makes macOS programs.
#
# Run (nothing else is needed; see README.md in this folder for volumes and the first link):
#
#   docker run -d --name agentic-org -p 127.0.0.1:8443:8443 agentic-org
#
# This is a DEVELOPMENT image: BIN names the workspace's own Linux build, the host logs
# that the worker has no publisher check, and the host keeps its keys in its development
# key store inside the secrets volume. A release image takes a signed release from the
# release pipeline instead (README.md, "What a release image still needs").

# Debian 13 (glibc 2.41), pinned by digest: the Linux adapters need glibc, not musl.
ARG BASE=debian:stable-slim@sha256:fbfb4059d2127120aa05b6e6064523d2102552f0cf0026ddf0701c12f1f8f3e0
FROM ${BASE}

# Where the four prebuilt Linux executables (deploy/container/build.sh) and the built UI
# are, relative to the build context.
ARG BIN=target/linux/debug
ARG UI=apps/agentic-org-ui/build

# tini is PID 1: it forwards the stop signal to `serve` and reaps the agent processes that
# outlive their parents. Pinned by version, as the base is by digest.
ARG TINI_VERSION=0.19.0-3+b8
RUN apt-get update \
 && apt-get install -y --no-install-recommends tini=${TINI_VERSION} \
 && rm -rf /var/lib/apt/lists/*

# The service user: a dedicated high UID and GID, unlikely to match an account of the
# Docker host. Its home is the data volume, so that everything the instance keeps (its
# records, its data folder, its sockets) is in a volume and nothing is in the image's
# own filesystem. The release is root-owned: the service user cannot replace a program.
RUN groupadd --gid 61000 agentic \
 && useradd --uid 61000 --gid 61000 --home-dir /data --no-create-home \
            --shell /usr/sbin/nologin agentic \
 && install -d -o 61000 -g 61000 -m 0700 /data /secrets \
 && install -d -o root -g root -m 0755 /opt/agentic-org /opt/agentic-org/bin /etc/agentic-org

COPY --chown=root:root --chmod=0755 \
     ${BIN}/agentic-org ${BIN}/agentic-org-office ${BIN}/agentic-org-web ${BIN}/agent-os \
     /opt/agentic-org/bin/
# The web process serves the UI build beside its executable, and nothing else.
COPY --chown=root:root ${UI}/ /opt/agentic-org/bin/ui/
RUN chmod -R u=rwX,go=rX /opt/agentic-org/bin/ui
COPY --chown=root:root --chmod=0644 deploy/container/setup.json /etc/agentic-org/setup.json

# `agent-os` is first on an agent's PATH. ORG_OFFICE_DEV_KEY_DIR is the development key
# store of a development build, inside the secrets volume; a release build ignores it.
ENV PATH=/opt/agentic-org/bin:/usr/local/bin:/usr/bin:/bin \
    USER=agentic LOGNAME=agentic \
    ORG_OFFICE_DEV_KEY_DIR=/secrets/dev-keys

# Declared volumes are mounts even when the operator names none, which the credential
# store requires of /secrets (a plain directory of the image is refused).
VOLUME ["/data", "/secrets"]

USER 61000:61000
WORKDIR /
# Plain HTTP (`http_published` in setup.json): publish it on the Docker host's 127.0.0.1.
EXPOSE 8443

# No HEALTHCHECK: the product has no command yet that fails when the instance does not
# serve (`agentic-org service status` reports and exits 0).
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["agentic-org", "serve", "--setup", "/etc/agentic-org/setup.json"]
