---
kind: spec
title: "Capture requests and route them to whoever should scope them (UX)"
status: for-review
revision: 7
owner: lead
created: "2026-11-08"
updated: "2026-11-15"
requested_by: owner
reviewers: [ac-staff-engineer-ada, owner]
builds_on: [work-items-ux, decisions-and-tech-debt-ux, agent-run-modes-ux]
---

# Spec · Capture requests and route them to whoever should scope them (UX)

Selected target UX, revision 7. [Organization-data 03](../design/organization-data/03-features-and-work.md), [05](../design/organization-data/05-operational-data.md), [06](../design/organization-data/06-identity-and-numbering.md), [08](../design/organization-data/08-record-contracts.md) and [12](../design/organization-data/12-task-assignment-and-history.md) own the typed task, durable metadata, references, provenance and assignment contracts. This spec describes capture and queue presentation; it does not implement target services or select all prior request-lifecycle engines. The [authority map](../design/documentation-authority.md) and [W7 current-contract mapping](../design/tasks-projects-docs.md#32-the-ports-org-work-defines) distinguish existing APIs from adaptation work.

**Revision 7 (2026-11-15):** clarifies accessible leading-mention completion, unresolved-prefix routing and exact preservation of request text.

**Revision 6 (2026-11-15):** rewrites capture, queue, assignment, reference/storage examples and RQS criteria around `type: request`, required accountability, optional project, scoped T- references, atomic team/individual assignment, durable history and separate private notes. Earlier `REQ-`/`FEAT-` sequences, office-only persistence and automatic approval/wake claims are retired from target guidance. [Revision 5](https://github.com/vgmello/agentic-org/blob/c6e520a36c4dc7c738c68915fac5e7d5d1cbe804/docs/specs/feature-requests-ux.md) preserves the detailed earlier lifecycle proposal. Stable RQS IDs are retained; deferred criteria are labeled individually.

## The request (the user, 2026-11-08)

Capture an idea quickly in the browser and route it to someone who can scope it, retaining why it was requested. Keep capture light and show routing reasons. Native/global capture remains a later feature; a browser draft is not a background outbox.

## 0. Decisions

1. **A request is a task with `type: request`.** It shares identity, comments, checklists, assignment and immutable task history with every other type. Type changes keep stable ID/number/relationships.
2. **Every request has an accountable team; project is optional.** Project capture derives the project's owner team. Projectless capture requires a valid team. Never invent a default team or a dummy project to save a request.
3. **All types use the same T- references.** A request is T-A43 everywhere; the creating office mints the reference and it needs no project or team in front. The word/icon Request identifies the type; it is not a separate REQ counter. Old stable IDs/readable references require an explicit import mapping, never blind prefix substitution.
4. **Capture may need only text when scope is already valid.** Show the chosen project/team, derive accountability visibly, and require team selection when context provides none. Browser-unsent text remains a draft; accepted local content is saved even when number registration is pending.
5. **Routing is an explicit assignment choice.** `assigned_team_id` and `assignee_id` are nullable independent fields; either, both or neither is valid. Assignment changes neither accountability nor number. Suggestions have reasons; the user decides. Just save clears the desired pair explicitly rather than inferring from employee membership.
6. **Reporter is immutable.** The admitted actor is the original requester; rerouting/scoping/approval never overwrites it. Latest updater, current scoper and recorded outcome actor are distinct facts.
7. **Scoping is a retained workflow proposal.** The earlier New → Routed → Scoping → Scoped → Approved, Declined/Merged flow remains useful UX intent, but target transition/approval/bulk creation and notification/wake handlers need an explicit lifecycle/API decision. Outcome data is already selected under details.request_outcome in document 03. Data selection alone does not deploy them.
8. **Definitions and shared knowledge live in organization Git.** A scoped feature uses F-6 and `projects/<project-id>/features/<feature-id>/feature.md`; designs/decisions/reports have stable-ID supporting documents with required project and at most one same-project feature/release/deployment scope. No code repository is required.
9. **Shared and private differ.** Request conversation is shared task metadata. Private notes require author-only CRUD in the separate durable store, have no administrator exception and cannot leak existence metadata into queue/search/activity. Existing note comments remain shared.
10. **No automatic execution from assignment.** Suggestions can consider actual availability; assignment is not permission, wake acknowledgment, session start or a transfer of accepting-writer authority. Execution and budgets follow their owning contracts.

## 1. Flow 1 · Capture

```
 New request · Restaurant SaaS
 [ Let guests choose adjacent tables when booking…                  ]
 Project [Reservations P-8 ▾]   Accountable team Payments (from project)
 Type Request                  Priority Normal
 Assigned team [No assigned team ▾]
 Scope with    [Eden ▾]  Product designer: a new user-facing flow
               Alternatives: ada · architecture; Ana · booking expertise
 [Just save]  [Message instead]                           [Send to Eden ⌘↵]
```

The dialog names the selected authorized organization. Its text field gets initial focus, grows to twelve lines then scrolls. ⌘Enter sends when text and accountable scope are valid. Optional project, priority, work classification and routing do not require extra typing when context supplies a valid scope; the chosen team remains visible. With no project/team context show **Accountable team — Choose team** and explain why Save needs it. Changing project derives its owner; removing project asks the user to confirm a valid accountable team rather than silently changing scope.

Suggestions appear after about one second without typing. A small Suggesting… indicates asynchronous work; once the user selects something later suggestions cannot overwrite it. Show suggested agent plus up to two valid alternatives, with role and a one-line reason. A suggested team without an individual is a valid route. A direct individual route may leave assigned team null. Do not populate assigned team from that employee's home team. The individual need not be a contributor or belong to the assigned team. Only active same-organization teams/employees can receive new assignments.

Without a copilot, an available configured rule may suggest the product designer for capability changes, staff engineer for technical questions or an explicitly configured project contact. Mark **Suggested by rule**. Missing role/contact leaves it unrouted with Choose who scopes it; do not invent a project lead field or automatic fallback. Suggestions do not promise an installed routing service.

Esc/close with text asks **Keep as a draft?** Keep retains the open-page draft (or explicitly saves an unrouted request only when valid scope and durable acceptance are available); Discard removes it; Back returns to editing. A draft has no number or shared task history. The UI must distinguish Keep draft from Just save.

Submit uses a stable creation ID and durable operation identity. Success requires durable task snapshot/event/receipt acceptance; it says “T-A43 sent to Eden · Open.” at once, online or offline, because the creating office numbers the task. A transient banner lasts about three seconds without taking focus. A lost response remains uncertain and resolves by querying/retrying the same operation, never a second request. The target mutation/registration service remains implementation work.

### 1.1 Capture details *(rev 3)*

- **@name routing:** only at the start, typing @ opens an accessible active-agent combobox/listbox with display name, handle, role and actual availability. Expanded state and the active option are exposed to assistive technology. ↑↓ navigates, Enter/Tab selects, Esc closes; moving the caret out of the leading mention dismisses the popup. Selection chooses the individual without inferring assigned team and removes the full selected mention span, including a multiword name; preserve the remaining text and punctuation outside its explicit separator. An unresolved leading prefix disables Send with a described field error and cannot silently fall back to a rule/copilot route. A different explicit route visibly reconciles that prefix rather than merely hiding its error. An @ elsewhere is ordinary text. Later role changes do not rewrite the saved original requester.
- **Assignment controls:** allow team-only, individual-only, both and unrouted states. On an existing request save the complete pair with explicit nulls and expected revision. On conflict show current versus desired pair and actor/time; retain draft and require explicit retry. Clearing individual returns it to a retained team queue; clearing team never infers another.
- **Message instead:** uses the chosen recipient and ordinary chat authorization, disabled with Choose who to message when none exists and a connection reason when unavailable. It does not create a request. Protected messaging storage/visibility remains governed by document 15; task history does not become chat storage. Confirmation links to Chat only after delivery acceptance.
- **Bug hint:** copilot or simple terms such as crash/error/broken can offer **Report a bug instead · Not a bug**. Carry text/project/accountability into bug capture; with no project allow a valid projectless bug. No area/General picker is inserted. Not a bug hides the hint for this capture.
- **Similar requests:** up to two accessible request-typed tasks matching title/body in permitted scopes show qualified references, e.g. “Looks like T-A9”. **Add to…** appends an explicitly shared comment and preserves the existing reporter. Do not include private notes, protected chat or private metadata in suggestions. Current `work.list` filter availability must be checked before claiming full-body search; target indexing remains adaptation work.
- **Attachments:** hidden until storage is designed. Pasted text is accepted; an image-only paste reports that images are unsupported rather than silently claiming an attachment was saved.
- **Unreachable service:** keep text in the open page, visibly Unsaved, with Copy draft and Retry. No native window, global shortcut, tray, background browser outbox or promised persistence through browser closure is required. An accepted locally saved task is different from a draft and can await number/publication independently.
- **Numbering:** any office may save local tasks/features. Its replicated counter is not an allocator; the shared organization authority assigns final references. A browser success does not require a final HQ number but does require an available accepting service. Creation of a project itself remains HQ-only.

## 2. Flow 2 · The queue (Work › Requests)

Work › Requests filters the common task projection by `type: request`. Project overview can link to its filtered view; team views include projectless requests. Rows use the scoped reference, title, optional project, accountable team, assigned team, individual, reporter/time, recorded status and supported next action.

```
 Requests  [Status ▾] [Accountable team ▾] [Assigned team ▾] [People ▾]
           [Project / No project ▾] [Search]                  [New request]
 T-A43       Adjacent table booking     Payments | Platform | Eden
 T-A9    Review onboarding         People   | —        | Ana
 T-A44   New starter guide         People   | People   | Unassigned
```

Saved filters distinguish **Owned by my team**, **Assigned to my team**, **Assigned to me** and **Raised by me**. Selecting a team queue does not hide directly assigned individuals from their own queues. Same local T- numbers can occur in different project/team scopes; search resolves the qualified reference rather than treating T-A43 as globally unique.

When a selected lifecycle adapter supports them, retain grouped **Needs you**, **With agents**, **Not routed**, **Approved**, **Declined and merged** presentation. Without it, group by recorded status/assignment; do not synthesize routed/accepted states from one non-null assignee. A team-only assignment is queued for that team, not necessarily accepted by a person. The old “Sent to Eden · not accepted yet” and “Eden is scoping” distinction requires actual acceptance evidence.

Search matches permitted shared title/body and selected references when supported; a title-only implementation declares that limitation. Remember filters. Loading shows skeleton rows; empty says “No requests yet. Capture what you need and choose an accountable team”; no match offers Clear filters; failure retains the last list with Retry; stopped/disconnected state is read-only with explicit stale/unsaved labels. No-project work remains available even when the organization has no projects.

## 3. Flow 3 · A request's page

Main column shows original `details.request_text` and shared conversation: questions, answers and activity with actors/times. Immutable reporter remains separate from later editor/scoper. Shared owner questions reference the actual question/answer comments; waiting spans retain start/end and explicit recipient rather than being inferred from current assignment. Closure cannot silently discard an unresolved question/span.

Side column shows type/reference, accountable team, optional project/feature/milestone, assigned team/individual, original requester/time and latest updater. Assignment changes use the same expected-revision atomic pair as every task. Queue/history include previous pairs; home-team or placement changes never rewrite them. Project ownership changes use the recoverable owning-team update contract and preserve assignment.

**Outcome data is selected:** `details.request_outcome` retains feature new/extends with stable feature_id, docs with project_id/doc_key, draft tasks with draft_id/title and optional description/assignee_id/team_id/priority, and estimate with effort/cost_usd_cents_min/cost_usd_cents_max. Show canonical feature definition F-6, supporting designs/decisions, editable task proposals and an explicitly attributed estimate/range. Missing/unknown cost information stays unknown; Build/Run is separate classification. A project-scoped feature needs a project; a projectless request may stay a team task rather than being forced into a feature. Before any explicitly selected Approve/Create workflow, pick/confirm project for the feature, preview each new task's accountability/type/assignment, allow removal/editing, and show exactly what is created. Document 03 validates nested references/variant tags and nonnegative estimate bounds with min≤max. Draft IDs remain proposals, not accepted task IDs. Bulk creation atomicity/retry and lifecycle handlers still need an approved mutation contract; this spec invents no new command.

Link origin through stable registered identities and retained task history/relates_to evidence; do not author unsupported `requests: [REQ-n]` or `from_request` target fields. Preserve original words and context in the request's canonical task/history. A feature can explain rationale and link to that request; it does not duplicate the request as its only durable source. New tasks created by an admitted actor have that actor as immutable creator; originating-request identity is a separate relationship, not a fabricated reporter.

**Private notes:** separate author-only pane, multiple notes with revision-aware CRUD, explicit publish preview and no implicit admin/team-lead access. A normal shared note comment is never presented as private. Publishing creates a shared comment only after author confirmation and normal task-post authorization; private original retains visibility. No private body/ID/count/author association enters shared task events, queue/search/export, and request reassignment does not expose the note. See PN-01–06 in document 05.

**Retained lifecycle controls:** Route/Change assignment is supported only through an actual adapter. Accept/scoping, Ask for changes, Approve and create tasks, Decline with reason and Merge into another open request are earlier workflow requirements whose transitions, authorization, outcomes and notification handling remain pending selection. Existing `request.*` wire/handlers remain pre-migration implementation evidence in W7 §4; exact current executable/command support follows [executable migration](../design/executable-migration.md). Do not publish invented target CLI examples. Automatic wake/scheduling is not selected by routing a request.

## 4. States and edge cases

| Case | Behavior |
| --- | --- |
| Context has no team | Retain draft; require a valid accountable team or select a project deriving its owner before save |
| Assigned employee belongs to another team | Valid if active/same organization; do not rewrite either assignment field from home membership |
| Chosen agent asleep, paused or disconnected | Show actual availability; assigning does not promise a wake or session. Execution admission/budget controls own any later wake |
| Chosen team archived or employee inactive | Refuse new assignment with a field error and retain text; preserve historical assignments/actor evidence |
| Request rerouted | Atomically record full old/new pair, actor/time and revision; original requester and number remain unchanged; retain route_reason/routing_source separately from assignment |
| Assignment conflict | Change neither field; show current state and retained desired state, allow explicit retry |
| Lost create/assignment response | Resolve using durable operation identity; show unknown/pending outcome until known, never duplicate the operation |
| Office cannot prove its task sequence (for example after a restore) | Creation is refused with the reason until HQ issues the office a new code; nothing is numbered from a replica |
| Unsupported lifecycle control | Preserve recorded state/outcome and explain unavailable action; never synthesize approval/acceptance |
| Import has only a snapshot | Show history baseline and earlier history unavailable; preserve selected request outcomes through details/task_details, old events through history, and unknown unsupported values in staging |
| No code repository | Organization Git request/feature/supporting knowledge remains available |
| Browser closes before save | No background capture guarantee; open-page text was explicitly Unsaved |
| Private store unavailable | Private save fails privately; never fall back to shared comment/history |

## 5. UX acceptance criteria (RQS)

RQS IDs are criterion identities; requests themselves use T- references. **Deferred** preserves earlier workflow requirements without selecting their engines.

- RQS-1 · New request opens an accessible in-page dialog naming the authorized organization from navigation/command palette. No native/global shortcut or separate capture window is required.
- RQS-2 · With valid project/team context, text plus ⌘Enter saves a request. Accountable team is always required; absent context asks for one. Project/assignment/classification remain optional. Project selection derives its accountable team.
- RQS-3 · Suggestions show a valid agent/team and up to two available alternatives with reasons; the user may choose another or save unrouted. Suggestions never silently overwrite explicit assignment or infer assigned team from employee membership.
- RQS-4 · Leading @ has accessible combobox/listbox keyboard completion, active-option/expanded state and caret dismissal. Selecting a multiword name removes the complete selected span, preserves all remaining request text and leaves assigned team unchanged. An unresolved prefix blocks Send without rule fallback; an interior @ is ordinary text. Message instead has explicit recipient/connection errors and confirms accepted ordinary messaging separately from task creation.
- RQS-5 · Available rule suggestions are labeled Suggested by rule; absent configuration leaves a visible choice rather than inventing a project lead or automatic owner fallback.
- RQS-6 · A bug hint carries text and valid scope into type bug capture, allows projectless bugs, inserts no mandatory area and can be dismissed for this capture.
- RQS-7 · Requests is a type-filtered task view across project/team scopes, with clear ownership/assignment columns. **Deferred:** Needs you/With agents/Approved/Declined-Merged grouping requires actual lifecycle/acceptance evidence.
- RQS-8 · Assignment shows actual availability and never promises an automatic wake. **Deferred:** on-call wake/inbox delivery, failed-wake escalation and associated execution admission require their own selected services.
- RQS-9 · **Deferred:** scoped outcomes/approval/bulk creation preserve editable proposals, estimates and explicit review. A feature requires a project, while the request itself may remain projectless; no task creation or feature transition is inferred from a status field.
- RQS-10 · Accepted assignment/content/shared-question changes have durable immutable task history with actors/times. **Deferred:** reroute acceptance, decline/merge and approval-specific transitions need lifecycle contracts; unknown imported history is labeled honestly.
- RQS-11 · Request text/identity/history persist canonically in organization Git plus the unpublished durable journal. Feature definitions/supporting docs use their selected stable-ID scope contracts; rationale links do not replace request history or add unsupported requests front matter.
- RQS-12 · Existing request wire/CLI behavior remains versioned current-contract evidence; target capture/assignment/private-note APIs require explicit adaptation. **Deferred:** universal target accept/ask/reroute/scope/approve/decline/merge command parity is not claimed.
- RQS-13 · Up to two similar permitted shared requests show qualified references; Add to explicitly appends a shared comment without changing reporter or creating a new record. Private notes never participate.
- RQS-14 · **Deferred:** Sent but not accepted versus Scoping requires a recorded acceptance event/state; assignment alone does not prove acceptance. Team-only routing stays a team queue without invented individual ownership.
- RQS-15 · Queue filters separate accountable team, assigned team, assignee, reporter and project/no project, remember choices, and declare actual search coverage. Loading/empty/no-match/retry/read-only states preserve last data. Project/team links open the correct scope.
- RQS-16 · Origin links use stable task/feature/document identities and permitted relationships; T- references are complete on their own. **Deferred:** open-only merge/decline rules and automatic From-request task creation require a selected lifecycle schema.
- RQS-17 · Disconnection retains open-page text labeled Unsaved with Copy draft. Accepted local save and pending publication are distinct; success requires recoverable task/events/receipt. Lost responses resolve by the same operation identity, without duplicate creation or an invented browser outbox.
- RQS-18 · Accepted Send/Save/Add-to uses a quiet three-second confirmation with the reference and Open, without stealing focus. Attachments stay hidden until storage design; image-only paste reports unsupported content.
- RQS-19 · Every request supports all four nullable assignment-pair states at expected revision. Assignment changes neither owning scope, number, original creator nor canonical file; home-team moves do not rewrite assignments.
- RQS-20 · All task types use one T- sequence per creating office and the same stable-ID rule; there are no separate REQ or B references.
- RQS-21 · Private request notes satisfy PN-01–06 in document 05: exclusive author access including against admins, separate durable recovery, no shared existence metadata, explicit publish, revision conflicts and retention/deletion. Existing shared note comments remain shared; private runtime/API/UI remain unimplemented.

## 6. Open questions

- A29: selected entity decision is one typed task with required accountability; earlier separate request model/counter is retired. Target approval/outcome/merge transition semantics remain a separate lifecycle decision.
- A30/U21: native/global capture remains deferred. The earlier preference for approval before proposed work is created is retained UX intent; it does not implement bulk task creation or force projectless requests into a project.
- C22/A76: suggestion quality and full-body shared search require explicit implementation; declare real capability. Attachment storage remains deferred. Rule suggestions require actual valid configuration.
- Target APIs must cover atomic assignment, durable acceptance/numbering and honest offline states without changing current command codecs. Selected request fields/outcomes map through details/task_details, including routing_source (old routed_by), optional independently known routing actor, merge target and decline reason. Unsupported link kinds/unknown variants remain in controlled import staging until explicitly mapped.
- Closure/merge handling for unresolved questions/waits, authorization and notification delivery require lifecycle decisions. Author-private notes are selected required data but remain runtime/store/access/UI implementation work.
