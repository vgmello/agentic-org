#!/bin/sh
# Builds the container image's executables for Linux, on any Docker host.
#
#   deploy/container/build.sh                  the four executables, into target/linux/debug
#   deploy/container/build.sh --release        the same, into target/linux/release
#   deploy/container/build.sh <cargo build arguments>   anything else, the same way
#
# The image needs Linux programs built against its own C library (glibc of Debian 13). A
# `cargo build` on a Mac makes macOS programs, which a Linux container cannot run, and one
# on another Linux distribution may need a newer glibc than the image has. So the build
# always runs in the Rust toolchain's own Debian 13 image, pinned like the image's base,
# with the repository mounted at its own path. Its output is in target/linux, apart from
# the machine's own builds, and the Dockerfile copies it from there (`BIN`). It runs as the
# calling user, so the files it makes are that user's.
#
# Needs only Docker. The first build downloads the crates into target/linux/cargo.
set -eu

image=rust:1.99.1-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07
repo=$(cd "$(dirname "$0")/../.." && pwd)

# Without package arguments: the four executables the image holds.
packages=yes
for argument in "$@"; do
    [ "$argument" = --release ] || packages=
done
if [ -n "$packages" ]; then
    set -- "$@" -p agentic-org -p agentic-org-office -p agentic-org-web -p agent-os
fi

mkdir -p "$repo/target/linux"
# RUSTUP_TOOLCHAIN names the toolchain the image has (the one rust-toolchain.toml pins), so
# that a user without write access to the image's toolchain installs nothing.
exec docker run --rm -u "$(id -u):$(id -g)" \
    -v "$repo:$repo" -w "$repo" \
    -e RUSTUP_TOOLCHAIN=1.99.1 \
    -e CARGO_HOME="$repo/target/linux/cargo" \
    -e CARGO_TARGET_DIR="$repo/target/linux" \
    -e CARGO_TERM_COLOR=never \
    "$image" cargo build --locked "$@"
