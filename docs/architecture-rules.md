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

These rules are architectural guardrails, not absolute enforcement gates. Use judgment, explain significant trade-offs, and do not block implementation solely because a numeric threshold is exceeded.

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

Do NOT interpret a high or low instability value as inherently good or bad.

The expected value depends on the architectural role of the component.

---

## Efferent Coupling Rules

Minimize the number of dependencies a module requires.

Use these thresholds as architectural guardrails:

|   Ce | Guidance                                                   |
| ---: | ---------------------------------------------------------- |
|  0–2 | Excellent autonomy                                         |
|  3–5 | Healthy                                                    |
|  6–7 | Review the design                                          |
| 8–10 | Architectural warning                                      |
|  >10 | Refactoring or decomposition SHOULD normally be considered |

### Requirements

- New modules SHOULD target `Ce <= 5`.
- `Ce > 7` MUST be architecturally justified.
- `Ce > 10` MUST trigger a design review before adding additional dependencies.
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

The purpose is to detect meaningful architectural inversions, not minor numerical differences.

---

# Dependency Inversion

When a stable/core component requires functionality implemented by a more volatile component:

DO NOT make the stable component depend directly on the volatile implementation.

Prefer:

`Stable Core -> Abstraction <- Volatile Implementation`

The abstraction SHOULD normally be owned by the component that requires the capability.

Example:

Bad:

`Orders.Domain -> StripePaymentClient`

Preferred:

`Orders.Domain -> IPaymentGateway`

`StripeAdapter -> IPaymentGateway`

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

`PaymentWorkflow -> IPaymentProcessor`

Possible implementations:

`StripePaymentProcessor`

`AdyenPaymentProcessor`

The consumer should depend on the capability it needs rather than on a specific implementation.

Swapping one valid implementation for another SHOULD require minimal or no changes to consuming code.

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

Prefer constructor or explicit parameter dependencies where appropriate.

Avoid hidden dependencies through:

- global mutable state,
- static service access,
- service locator patterns,
- ambient state,
- runtime lookup without clear reason,
- reflection used to conceal architectural relationships.

A component's required collaborators SHOULD be discoverable from its public construction or invocation model.

---

# Circular Dependencies

Circular dependencies are prohibited.

There MUST be no new circular dependencies between:

- domains,
- bounded contexts,
- services,
- modules,
- packages,
- architectural layers.

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

A domain MUST NOT require knowledge of another domain's internal implementation.

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

Before implementing a change that introduces a new dependency or architectural relationship, evaluate:

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

Claude MUST explicitly mention the architectural consequence before implementing it.

Where practical, Claude MUST propose a lower-coupling or more composable alternative.

---

# Expected Instability by Component Role

Use these values as rough expectations, not hard limits:

| Component Type                | Typical I |
| ----------------------------- | --------: |
| Core/shared contracts         |   0.0–0.2 |
| Platform primitives           |   0.0–0.3 |
| Core domain components        |   0.1–0.4 |
| Business/domain services      |   0.2–0.6 |
| Application services          |   0.4–0.7 |
| Integration/adapters          |   0.6–0.9 |
| Orchestration/BFF/API Gateway |   0.7–1.0 |
| UI/frontend                   |   0.8–1.0 |

Do NOT attempt to force every component toward low instability.

Highly volatile edge components may correctly have `I` close to `1`.

Highly reused foundational components should generally trend toward `I` close to `0`.

---

# Architecture Smells

Claude SHOULD actively identify these patterns during implementation and review:

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
