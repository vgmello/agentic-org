# Security Policy

## Scope

Security reports are especially welcome for:

- Host and office authentication, session handling, and enrollment (`apps/agentic-org-web`, `crates/org-host`)
- PKI, key management, and office-to-host trust boundaries (`crates/org-pki`, `crates/org-platform-host`)
- Agent process isolation, OS sandboxing, and privilege boundaries (`crates/org-supervise`, `crates/org-platform`)
- Content-Security-Policy (CSP) headers and browser BFF boundaries (`config/csp/`, `apps/agentic-org-web`)

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Instead, please report vulnerabilities privately via GitHub Security Advisories on this repository (`Security` -> `Advisories` -> `Report a vulnerability`).

Please include:

- A clear description of the vulnerability and its potential impact
- Steps to reproduce or a minimal proof of concept
- Affected commit SHA, binary/crate, and OS/runtime environment
- Any suggested mitigation or patch, if known

## Response Expectations

- We aim to acknowledge receipt within **72 hours**.
- We will coordinate a fix and disclosure timeline with you before publishing details.
- Please give us reasonable time to investigate and release a fix before public disclosure.
