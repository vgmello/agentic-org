# Architecture Rules

## Intent

The architecture MUST optimize for:

- high cohesion,
- low unnecessary coupling,
- explicit boundaries,
- stable dependency direction,
- composability,
- encapsulated behavior,
- and replaceability of volatile implementations.

These rules apply whenever creating, modifying, moving, or refactoring modules, packages, services, bounded contexts, domains, workflows, components, or abstractions.

These rules are architectural guardrails, not absolute enforcement gates. Use judgment, explain significant trade-offs, and do not block implementation solely because a numeric threshold is exceeded. Numeric thresholds do not automatically require a separate approval step.

Apply these rules using native .NET/C#, Rust, or TypeScript/Svelte constructs. Agentic Org's Rust/static-SvelteKit profile, target names, folder layout, shared-code examples, and reorganization sequence are in [project structure](design/project-structure.md), under [ADR-0005](adr/0005-standalone-server-v1.md).

---

# Language and Framework Independence

An architectural component may be a cohesive function, module, package, struct,
record, enum, class, UI feature, service, or public API. Choose the smallest
construct that expresses its responsibility and boundary clearly.

| Concern | .NET / C# | Rust | TypeScript / Svelte |
| --- | --- | --- | --- |
| Organization | Feature folders, namespaces, projects | Modules, crates, workspace packages | Feature directories, modules, packages, routes |
| Contracts | Interfaces, delegates, public APIs | Traits, function signatures, public APIs | Interfaces, function types, exports, component props |
| Encapsulation | Access modifiers, assembly boundaries | Private items, restricted visibility, public exports | Module exports, package entry points, component APIs |
| Composition | Parameters, delegates, constructor injection | Parameters, structs, generics, enums, closures, trait objects | Parameters, objects, factories, props, callbacks, snippets |
| Wiring | Composition roots, framework registration | Explicit construction and application wiring | Explicit construction and scoped framework integration |

Folders and namespaces do not enforce boundaries by themselves. Use visibility,
exports, suitable tooling, and review. Translate architectural intent into native
constructs rather than copying another ecosystem's machinery.

# Domain Modules Containing Vertical Slices

Prefer `application -> domain module -> feature/use case` for business behavior.

- Choose modules from cohesive business capabilities, workflows, invariants,
  terminology, and ownership. A database table alone does not define a domain.
- Keep each slice's specific validation, orchestration, persistence operations,
  response mapping, and behavior tests close together.
- Keep shared domain types, policies, and invariants with their owning module.
- Give modules deliberate public APIs and keep implementation details private.
- Scale structure to complexity. A simple slice may remain one function/file.
- Choose crate, project, package, service, database, and deployment boundaries
  separately. A feature does not require its own package.
- An end-to-end slice is one delivery responsibility across relevant UI, contracts,
  backend behavior, and verification. Preserve browser/server/service boundaries.
- Group feature-specific UI components, state, validation, and API integration by
  domain/use case. Align frontend/backend terminology while allowing different
  internal structures. Preserve framework routing conventions.
- Keep reusable visual primitives and design tokens in a shared UI foundation.
- Apply these boundaries incrementally while preserving behavior.

# Shared Code and Ownership

Place shared behavior at the narrowest scope that owns its meaning:

1. Feature-specific policy stays inside the feature.
2. Rules shared by one domain's features stay inside that domain module.
3. A cross-domain capability has a clear owner and a small public API.
4. Domain-independent primitives use a focused module or an established library.
   Extract a package when that boundary provides practical value.

Share behavior when consumers require the same semantics and compatible evolution.
Similar syntax alone does not establish a shared concept. Small duplication can
remain until ownership and common semantics are clear.

Keep upload widgets separate from file policy and storage. Keep application-specific
hash canonicalization/versioning with the contract it implements; reuse an
established hashing library where appropriate. Preserve UI licensing restrictions.

# Applying the Metrics While Writing Code

Estimate coupling before a change and as implementation takes shape. Select a
consistent architectural level: logical domain modules, projects/crates/packages,
libraries, or services. A logical module can be inside one project/crate. A class,
file, or UI component is not automatically a separately counted unit. Internal
dependencies still need cohesion and encapsulation review.

For each affected unit, identify direct dependencies and consumers, note edges
added by the change, and run the change-time check below. Count distinct units
once regardless of imported symbols or calls. Do not count transitive dependencies
as direct Ce. State whether framework/third-party dependencies are included.

Distinguish source/package dependencies, logical module contracts, and runtime
service communication. Do not combine them into one unexplained score or compare
values measured at different levels. Dependency inversion can improve source
direction while preserving a necessary runtime interaction.

Rough estimates are sufficient. Avoid false precision and metric-driven splits.

---

# Coupling and Dependency Management

## Coupling Metrics

Use the following definitions:

- **Afferent Coupling (Ca):** Number of external modules that depend on this module.
- **Efferent Coupling (Ce):** Number of external modules this module depends on.
- **Instability (I):**

  `I = Ce / (Ca + Ce)`

Where:

- `I = 0` means maximally stable.
- `I = 1` means maximally unstable.
- When `Ca + Ce = 0`, report I as undefined or not applicable.

This stability is structural resistance to change due to dependents. It does not directly measure change frequency, implementation volatility, quality, or runtime reliability. Measure all values in the same explicitly selected graph.

Do NOT interpret a high or low instability value as inherently good or bad.

The expected value depends on the architectural role of the component.

---

## Efferent Coupling Rules

Minimize the number of dependencies a module requires.

Use these thresholds as architectural guardrails:

|   Ce | Guidance                                                   |
| ---: | ---------------------------------------------------------- |
|  0–2 | Limited outgoing coupling                                         |
|  3–5 | Usually manageable                                                    |
|  6–7 | Review the design                                          |
| 8–10 | Architectural warning                                      |
|  >10 | Refactoring or decomposition SHOULD normally be considered |

### Requirements

- New modules SHOULD target `Ce <= 5`.
- `Ce > 7` MUST be architecturally justified.
- `Ce > 10` MUST trigger a design assessment before adding additional dependencies. This assessment does not automatically require human approval.
- Composition roots and orchestration boundaries may legitimately have more dependencies; calibrate counts to their role and the repository's counting convention.
- Do NOT introduce dependencies merely for convenience.
- Before adding a new dependency, determine whether the behavior can instead be achieved through:
  - an existing abstraction,
  - an existing contract,
  - responsibility relocation,
  - dependency inversion,
  - domain events,
  - messaging,
  - composition,
  - or decomposition of responsibilities.

Do NOT split modules artificially just to improve the metric.

The objective is not to minimize Ce at all costs. The objective is to prevent unnecessary knowledge and responsibility from spreading across the architecture.

---

## Afferent Coupling Rules

High `Ca` is not inherently undesirable.

Components intentionally used as stable foundations may naturally have high afferent coupling.

Examples include:

- domain primitives,
- public contracts,
- shared protocols,
- stable abstractions,
- platform interfaces,
- foundational libraries.

However:

> The higher the Ca, the greater the required stability and backward compatibility.

When modifying a high-Ca component:

- Evaluate the blast radius before making the change.
- Prefer additive changes over breaking changes.
- Avoid leaking implementation details into its public contract.
- Preserve backward compatibility whenever practical.
- Explicitly identify downstream consumers when making a breaking change.
- Be especially cautious about adding volatile responsibilities to highly depended-upon components.

A component with high Ca SHOULD remain narrow, stable, intentional, and difficult to change accidentally.

---

# Stable Dependency Principle

Dependencies SHOULD flow from more volatile components toward more stable components.

Preferred direction:

`unstable -> stable`

Example:

`UI -> Application -> Domain -> Stable Contracts`

Avoid:

`stable -> volatile`

A stable or widely depended-upon component MUST NOT depend directly on volatile implementation details unless there is a strong architectural reason.

---

## Stability Inversion

A dependency is considered suspicious when:

`I(source) + 0.30 < I(target)`

For example:

`Core.Domain (I=0.15) -> Feature.Adapter (I=0.80)`

This is a stability inversion.

When introducing such a dependency:

1. Identify the inversion.
2. Explain why the dependency may be problematic.
3. Propose a lower-coupling alternative.
4. Prefer dependency inversion where appropriate.
5. Proceed only when the dependency is intentionally justified.

Do not flag insignificant differences such as:

`0.42 -> 0.47`

The purpose is to detect meaningful architectural inversions, not minor numerical differences. The formula is a review heuristic in a consistently measured source/module graph, not proof of a defect. Also inspect actual volatility and responsibilities.

The illustrative dependency chain does not mandate horizontal project layers. Apply the principle within domain modules containing vertical slices.

---

# Dependency Inversion

When a stable/core component requires functionality implemented by a more volatile component:

DO NOT make the stable component depend directly on the volatile implementation.

Prefer:

`Stable Core -> Abstraction <- Volatile Implementation`

The abstraction SHOULD normally be owned by the component that requires the capability. It may be a trait, interface, function type, callback, or another small API. Keep it with the application/domain module that owns the requirement; pure domain logic need not own infrastructure contracts merely because it is central. Composition code selects implementations.

Example:

Bad:

`Orders.Domain -> StripePaymentClient`

Preferred:

`Orders.Domain -> PaymentGateway`

`StripeAdapter -> PaymentGateway`

Implementation details MUST point toward abstractions rather than forcing abstractions to depend on implementation details.

Dependency inversion is especially valuable when isolating:

- databases,
- message brokers,
- external APIs,
- payment providers,
- cloud SDKs,
- file systems,
- queues,
- search engines,
- third-party libraries,
- infrastructure concerns,
- integration concerns.

Do NOT use dependency inversion mechanically where it provides no meaningful architectural benefit.

---

# Composability and Behavioral Encapsulation

The architecture SHOULD foster composability.

Where practical, design software from small, cohesive building blocks that can be combined into larger behaviors without exposing unnecessary implementation details.

Prefer building from:

`small focused behavior -> composable capability -> larger workflow`

rather than starting with large components that accumulate unrelated responsibilities.

---

## Build Small, Compose Upward

When implementing new behavior, first identify the smallest meaningful cohesive capabilities.

Then compose those capabilities into larger workflows.

For example:

`ValidateOrder`

`CalculatePrice`

`ReserveInventory`

`AuthorizePayment`

may be composed into:

`PlaceOrder`

rather than implementing validation, pricing, inventory management, payment processing, and orchestration directly inside one large component.

This approach SHOULD be preferred when it improves:

- cohesion,
- readability,
- independent evolution,
- testability,
- replaceability,
- reuse,
- or architectural clarity.

However, decomposition MUST remain meaningful.

Do NOT create tiny classes, wrappers, interfaces, or abstractions that merely move individual lines of code without establishing a useful behavioral boundary.

The goal is composability, not fragmentation.

---

## Behavioral Encapsulation

Encapsulate meaningful behavior behind methods, APIs, contracts, interfaces, or other appropriate boundaries.

Components SHOULD expose capabilities rather than exposing internal mechanics.

Prefer:

`order.Cancel(reason)`

over:

`order.Status = Cancelled`

`order.CancelledAt = now`

`order.CancellationReason = reason`

The component that owns the behavior SHOULD also own the rules and invariants associated with that behavior.

A functional transition such as `cancel_order(order, reason) -> Result<UpdatedOrder, Error>` is equally valid. Plain records, DTOs, immutable values, and read models remain appropriate for data transfer and representation. A behavioral boundary need not be a class.

Avoid designs where callers must understand implementation details in order to correctly manipulate a component.

Prefer:

> Tell a component what outcome is required.

Avoid:

> Ask for its internal state and manipulate that state externally.

---

## Keep Implementation Details Private

Implementation details SHOULD remain private to the component that owns them.

Consumers SHOULD depend on:

- capabilities,
- contracts,
- stable APIs,
- meaningful abstractions,

rather than:

- internal classes,
- infrastructure details,
- persistence structures,
- SDK-specific types,
- private DTOs,
- implementation helpers.

A consumer SHOULD need to understand what a component does, not how it does it.

---

# Substitutability and Replaceability

Where multiple implementations are plausible, volatile, environment-specific, or expected to evolve independently, expose the behavior through a stable abstraction.

Example:

Preferred:

`PaymentWorkflow -> PaymentProcessor`

Possible implementations:

`StripePaymentProcessor`

`AdyenPaymentProcessor`

The consumer should depend on the capability it needs rather than on a specific implementation.

Swapping one valid implementation for another SHOULD require minimal or no changes to consuming code. Preserve required errors, ordering, cancellation, consistency, and operational guarantees; matching a type signature alone is insufficient.

Examples of behavior that commonly benefits from replaceability include:

- payment processing,
- storage,
- messaging,
- caching,
- authentication providers,
- notification delivery,
- search,
- external APIs,
- persistence,
- clock/time providers,
- feature evaluation,
- file handling,
- infrastructure-specific behavior.

---

## Do Not Over-Abstract

Do NOT introduce an interface, abstraction, strategy, factory, wrapper, provider, or additional indirection merely because an implementation could theoretically change someday.

Create an abstraction when it provides real architectural value, such as:

- isolating volatile infrastructure,
- enabling multiple implementations,
- creating a meaningful domain boundary,
- reducing coupling,
- protecting stable code from implementation details,
- allowing independent evolution,
- enabling meaningful substitution,
- improving testability where substitution is valuable.

A single simple implementation with no meaningful volatility MAY remain concrete.

Prefer:

> abstractions around behavior and volatility

rather than:

> abstractions around every class.

---

# Composition Over Inheritance

Prefer composition and delegation over inheritance when assembling behavior.

Inheritance SHOULD be used only when there is a genuine substitutable type relationship and inheritance expresses the domain clearly.

Avoid inheritance used primarily for:

- code reuse,
- utility sharing,
- injecting unrelated behavior,
- creating large class hierarchies,
- sharing implementation details.

Prefer:

`Workflow -> CapabilityA + CapabilityB + CapabilityC`

over deep inheritance hierarchies where practical.

---

# Explicit Dependencies

Dependencies MUST be visible and intentional.

Prefer explicit construction, function parameters, application wiring, or component props/callbacks as appropriate to the language.

Avoid hidden dependencies through:

- global mutable state,
- static service access,
- service locator patterns,
- ambient state,
- runtime lookup without clear reason,
- reflection used to conceal architectural relationships.

A component's required collaborators SHOULD be discoverable from its public construction or invocation model.

Framework DI, request contexts, Svelte context, and lifecycle hooks are acceptable when scope and ownership are explicit. Keep request/user-specific server state isolated to its request or an appropriate external store.

---

# Circular Dependencies

Cycles between declared source dependency boundaries are prohibited.

There MUST be no new circular dependencies between:

- domains,
- bounded contexts,
- modules,
- projects/crates/packages,
- architectural layers.

Evaluate runtime interactions separately. Request/response traffic or event exchange does not automatically imply a source cycle. Circular waits, synchronous service call cycles, and uncontrolled event feedback require correction.

If implementing a change would introduce a cycle:

1. Do NOT implement the cycle.
2. Identify the architectural cause.
3. Break the cycle using one of:
   - dependency inversion,
   - responsibility relocation,
   - events/messages,
   - extraction of a stable abstraction,
   - restructuring of module ownership,
   - workflow extraction,
   - composition around a higher-level coordinator.

A circular dependency MUST NOT be hidden through service locators, reflection, runtime resolution, or similar mechanisms.

---

# Domain Boundaries

Domain boundaries MUST remain explicit.

Cross-domain dependencies SHOULD be exceptional rather than the default.

Prefer communication through:

- explicit public contracts,
- APIs,
- commands,
- events,
- messages.

Do NOT reference another domain's internal:

- entities,
- persistence models,
- repositories,
- implementation services,
- internal DTOs,
- internal modules.

A domain MUST NOT require knowledge of another domain's internal implementation. Public domain values may cross a boundary when ownership and compatibility obligations are intentional.

Choose in-process APIs, commands, HTTP, events, or messaging according to the workflow and deployment model. Messaging adds delivery, ordering, consistency, and failure obligations; do not introduce it merely to lower a coupling metric.

Every new cross-domain dependency MUST be treated as an architectural decision.

Where possible, cross-domain interactions SHOULD depend on the smallest contract necessary.

Do not expose an entire domain model simply because one capability is required.

---

# Orchestration vs Behavior

Separate orchestration from the implementation of individual behaviors where practical.

A workflow may coordinate multiple capabilities, but SHOULD NOT absorb the internal rules of every capability it invokes.

For example:

`PlaceOrder`

may coordinate:

`ValidateOrder`

`CalculatePrice`

`ReserveInventory`

`AuthorizePayment`

but each capability SHOULD own its own internal rules.

Avoid orchestrators that become large procedural components containing the business logic of every subsystem they coordinate.

---

# Component API Design

Composable components SHOULD expose small, intentional APIs.

A well-designed component SHOULD:

- have a clear responsibility,
- expose only the operations consumers genuinely require,
- hide implementation details,
- minimize knowledge of its callers,
- minimize knowledge of unrelated collaborators,
- protect its own invariants,
- be independently understandable,
- be independently testable where practical,
- be replaceable when an equivalent implementation exists.

Large public APIs often indicate weak encapsulation.

When a component exposes many unrelated operations, reconsider whether it contains multiple responsibilities.

---

# Responsibility and Cohesion

Behavior that changes for the same reason SHOULD usually live together.

Behavior that changes for unrelated reasons SHOULD usually be separated.

Prefer cohesive capabilities over generic utility modules.

Avoid large generic components such as:

- `Common`
- `Helpers`
- `Utils`
- `SharedServices`
- `Manager`
- `Processor`

when they accumulate unrelated responsibilities.

A shared component MUST represent a meaningful shared concept, not merely be a convenient place to put reusable code.

---

# Change-Time Architecture Check

Before implementing a change that introduces a new dependency or architectural relationship, use the estimates from [Applying the Metrics While Writing Code](#applying-the-metrics-while-writing-code) and evaluate:

1. Does this dependency increase Ce?
2. Does it create a new cross-domain dependency?
3. Does it create a circular dependency?
4. Does it create a stability inversion?
5. Does it increase the blast radius of a high-Ca component?
6. Is the dependency on an abstraction or an implementation detail?
7. Can the responsibility belong somewhere else?
8. Could an event/message avoid unnecessary synchronous coupling?
9. Is there already an existing contract that should be reused?
10. Is the new dependency genuinely required?
11. Can the behavior be expressed as a smaller cohesive capability?
12. Can the larger behavior be composed from existing capabilities?
13. Is volatile behavior isolated from stable behavior?
14. Would another implementation be difficult to substitute later?
15. Is the proposed abstraction meaningful, or merely indirection?
16. Is business behavior being encapsulated or leaked into callers?
17. Is orchestration becoming responsible for implementation details?

If the proposed change creates:

- a circular dependency,
- a significant stability inversion,
- `Ce > 7`,
- or a new cross-domain dependency,

The agent MUST explicitly mention the architectural consequence before implementing it.

Where practical, the agent MUST propose a lower-coupling or more composable alternative.

Also check measurement scope, public API ownership, runtime failure/consistency obligations, browser/server boundaries, and shared state lifetime. Revise designs that create prohibited source cycles. These checks guide implementation judgment and do not automatically require a separate approval step.

---

# Expected Instability by Component Role

Use contextual expectations rather than universal numeric target ranges:

| Role | Interpretation |
| --- | --- |
| Shared contracts/primitives | Often many consumers and few dependencies |
| Domain modules | Depends on capability scope and ownership |
| Feature handlers/workflows | May coordinate several capabilities |
| Provider adapters | Depend on application contracts and provider libraries |
| Composition roots | Intentionally know concrete implementations |
| UI routes/entry points | Often many outgoing edges and few consumers |
| Shared UI primitives | May have many consumers and require compatibility care |

Not all frontend code has high I, and not all domain code has low I. Assess
observed dependencies, responsibilities, and implementation volatility together.

---

# Frontend and Server Boundaries

- Keep feature behavior close to its components and state.
- Keep visual primitives independent of unrelated business domains.
- Give shared state a clear owner, lifetime, and public API.
- Use Svelte derived state for computations and effects for external synchronization.
- Keep secrets and privileged operations in server-only code.
- Enforce authorization and authoritative validation at the owning backend boundary.
- Use client validation for feedback while retaining server enforcement.
- Validate untrusted inputs; TypeScript types alone do not validate network data.
- Define serialization/error contracts across C#, Rust, and TypeScript boundaries.
- Follow the application's chosen SSR/static deployment model.

---

# Architecture Smells

The agent SHOULD actively identify these patterns during implementation and review:

- excessive Ce,
- unexpected high Ca,
- stability inversions,
- circular dependencies,
- bidirectional domain dependencies,
- shared-domain dumping grounds,
- "common" libraries containing unrelated business logic,
- orchestration logic leaking into domain components,
- domain components depending on infrastructure,
- implementation types leaking across domain boundaries,
- unnecessary synchronous service-to-service dependencies,
- dependency chains with excessive depth,
- central services that become architectural bottlenecks,
- large classes with many unrelated responsibilities,
- abstractions that leak implementation details,
- callers manipulating internal state directly,
- unnecessary inheritance,
- abstractions created only for abstraction's sake,
- tiny fragmented classes with no meaningful behavioral boundary,
- workflows that contain the implementation logic of all of their collaborators,
- components that are difficult to replace because consumers depend on concrete implementation details,
- service locator or hidden dependency patterns,
- generic utility modules becoming architectural dumping grounds.

When detected, explain the issue in architectural terms rather than merely reporting the metric.

---

# Architectural Priorities

When trade-offs exist, prioritize in this order:

1. Correct domain boundaries.
2. High cohesion.
3. No circular dependencies.
4. Stable dependency direction.
5. Low unnecessary coupling.
6. Composable and encapsulated behavior.
7. Explicit contracts.
8. Isolation of volatile implementation details.
9. Replaceability where architectural value exists.
10. Small blast radius.
11. Numeric coupling targets.

Numeric metrics are guardrails, not goals.

Never degrade cohesion, introduce artificial abstractions, fragment meaningful behavior, or create unnecessary indirection merely to improve Ca, Ce, or I.

---

# Core Principles

The goal is not:

> Minimize every coupling number.

The goal is not:

> Create an abstraction for every implementation.

The goal is not:

> Split software into the smallest possible classes.

The goal is:

> Build cohesive, composable components with intentional dependencies and explicit boundaries.

Prefer to start from small, meaningful behaviors and compose them into larger capabilities.

Encapsulate implementation details behind stable APIs where doing so protects the architecture.

Isolate volatile behavior so it can evolve or be replaced without forcing unrelated consumers to change.

Keep volatile code depending on stable code.

Protect domain boundaries.

Prefer composition over unnecessary inheritance.

Expose behavior rather than internal state.

Minimize the blast radius of change.

Optimize for software that can evolve by replacing, recombining, or extending well-defined capabilities rather than rewriting large interconnected components.
