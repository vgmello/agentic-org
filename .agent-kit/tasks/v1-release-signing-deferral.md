# Owner-directed release-signing phase order

2026-11-16. The owner confirmed release signing needs setup and explicitly said:
"I want to fully skip this for now and only handle that last after everything else is implemented".

Continue all other implementation first. Defer Apple signing credential setup,
release-signing public-key provisioning and genuine signed-release production
verification to the final release phase. Missing signing inputs are no longer a
prerequisite for functional development, local code acceptance or integration of
reviewed increments into v1-integration. Earlier plans describing authentic signing
as a current per-feature blocker are superseded solely on this scheduling point.

Preserve implemented signing/release verification and fail-closed production
admission. This direction does not authorize an unsigned production bypass,
generated production signing keys, relaxed trust/image checks or invented passing
production setup. Existing generated debug fixtures remain explicitly scoped
development/test evidence. Implement and review everything else with complete
functional/security/durability tests, current app/workspace/kit/browser checks,
original-author validation and fresh independent initial/final exact-commit review.
Other unresolved authorization/platform/data prerequisites keep their own scope;
checkpoint them and continue independent work as directed.

Final release work remains: genuine Apple signing and release public trust setup,
complete signed bundles/positive release floor, matching image/platform identity,
normal signed-release setup/service and affected end-to-end production verification.
List these as deferred release acceptance, not failed implementation or completed
production readiness. Do not ask for signing inputs again during ordinary feature
implementation. Root owns architecture, phased acceptance, interfaces and integration.
