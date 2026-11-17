# Linux host design

How Agentic Org runs on Linux, including as the main process of a container, under
[ADR-0017](../adr/0017-linux-and-container-hosting.md). It names the platform
contracts, the one crate that implements them for every operating system, the Linux
adapter of each, what each proves, and the order of work. The macOS adapters are the
code that exists today; their requirements stay in
[ADR-0008](../adr/0008-gateway-macos-sandbox.md),
[ADR-0011](../adr/0011-protected-recovery-delivery.md) and the
[host boundary design](office-host-boundary.md).

## 1. The shared platform crate

## 2. What each executable needs from the platform

## 3. The five contracts

### 3.1 Process admission

### 3.2 Credential provider

### 3.3 Confinement of the web process

### 3.4 Service lifecycle

### 3.5 Agent isolation

## 4. Supporting capabilities

## 5. What a local attacker can still do on Linux

## 6. What this design was checked against

## 7. Working in parallel with the macOS track
