---
kind: spec
title: "Capture requests and route them to whoever should scope them (UX)"
status: current
owner: lead
created: "2026-11-08"
updated: "2026-11-17"
requested_by: owner
builds_on: [work-items-ux, decisions-and-tech-debt-ux, agent-run-modes-ux]
---

# Spec · Capture requests and route them to whoever should scope them (UX)

[Organization-data 03](../design/organization-data/03-features-and-work.md), [05](../design/organization-data/05-operational-data.md), [06](../design/organization-data/06-identity-and-numbering.md), [08](../design/organization-data/08-record-contracts.md) and [12](../design/organization-data/12-task-assignment-and-history.md) own the typed task, durable metadata, references, provenance and assignment. This spec describes capture and queue presentation. The [task ports](../design/tasks-projects-docs.md#32-the-ports-org-work-defines) describe the domain interface.

## The request (the user, 2026-11-08)

Capture an idea quickly in the browser and route it to someone who can scope it, keeping why it was requested. Keep capture light and show routing reasons. A browser draft is not a background outbox.

## 0. Decisions

1. **A request is a task with `type: request`.** It shares identity, comments, checklists, assignment and immutable task history with every other type. Changing the type keeps the stable ID, reference and relationships.
2. **Every request has an accountable team; project is optional.** Capturing in a project derives the project's owner team. Projectless capture requires a valid team. No default team or dummy project is invented to save a request.
3. **All types use the same `T-` references.** A request is `T-A43` everywhere; the creating office mints the reference, and it needs no project or team in front. The word and icon Request identify the type.
4. **Capture may need only text when scope is already valid.** The dialog shows the chosen project or team, shows the derived accountability, and requires a team when the context provides none. Text not yet sent from the browser is a draft.
5. **Routing is an explicit assignment choice.** `assigned_team_id` and `assignee_id` are independent nullable fields; either, both or neither is valid. Assignment changes neither accountability nor reference. Suggestions have reasons; the person decides. Just save clears the pair explicitly rather than inferring it from employee membership.
6. **Reporter is immutable.** The admitted actor is the original requester; rerouting, scoping or approval never overwrites it. Latest updater, current scoper and recorded outcome actor are distinct facts.
7. **Scoping is a person's or agent's review, then an approval.** The person or agent the request is assigned to scopes it and records the outcome in `details.request_outcome` (document 03): a new or extended feature, documents, draft tasks and an estimate. The request uses the ordinary five task statuses; scoping is shown from the recorded outcome and history, not from a status of its own. **Approve and create** creates exactly the previewed records and links them to the request. **Decline** requires a reason (`decline_reason`) and closes the request as Cancelled; **Merge into…** names another open request (`merged_into_task_id`) and closes this one as Cancelled. Each step is a history event with actor and time. There is no automatic scoping engine.
8. **Definitions and shared knowledge live in organization Git.** A scoped feature uses F-6 and `projects/<project-id>/features/<feature-id>/feature.md`; designs, decisions and reports are stable-ID supporting documents with a required project and at most one same-project feature, release or deployment scope. No code repository is required.
9. **Shared and private differ.** Request conversation is shared task metadata. Private notes are author-only in the separate durable store, have no administrator exception, and leak no existence metadata into queue, search or activity.
10. **Assignment does not start work.** Suggestions can consider actual availability; assignment is not permission, a wake, a session start or a transfer of accepting-writer authority. Execution and budgets follow their owning contracts.

## 1. Flow 1 · Capture

```
 New request · Restaurant SaaS
 [ Let guests choose adjacent tables when booking…                  ]
 Project [Reservations P-8 ▾]   Accountable team Payments (from project)
 Type Request                  Priority Normal
 Assigned team [No assigned team ▾]
 Scope with    [Mira ▾]  Product designer: a new user-facing flow
               Alternatives: sam · architecture; Ana · booking expertise
 [Just save]  [Message instead]                           [Send to Mira ⌘↵]
```

The dialog names the selected authorized organization. Its text field gets initial focus, grows to twelve lines then scrolls. ⌘Enter sends when text and accountable scope are valid. Project, priority, work classification and routing need no extra typing when the context supplies a valid scope; the chosen team stays visible. With no project or team context the dialog shows **Accountable team — Choose team** and explains why Save needs it. Changing the project derives its owner; removing the project asks the person to confirm a valid accountable team rather than silently changing scope.

Suggestions appear about one second after typing stops. A small Suggesting… marks the work; once the person has chosen, later suggestions cannot overwrite the choice. The dialog shows a suggested agent plus up to two valid alternatives, each with role and a one-line reason. A suggested team without an individual is a valid route. A direct individual route may leave the assigned team null; the assigned team is never filled from that employee's home team. The individual need not be a contributor or belong to the assigned team. Only active same-organization teams and employees can receive new assignments.

Without a copilot, a configured rule may suggest the product designer for capability changes, the staff engineer for technical questions, or an explicitly configured project contact, marked **Suggested by rule**. With no matching role or contact the request stays unrouted with Choose who scopes it; no project-lead field or automatic fallback is invented.

Esc or close with text asks **Keep as a draft?** Keep keeps the open-page draft; Discard removes it; Back returns to editing. A draft has no reference and no shared task history. Keep draft and Just save are distinct.

Submit uses a stable creation ID and a durable operation identity. Success means the task snapshot, event and receipt are accepted; the dialog says "T-A43 sent to Mira · Open." at once, online or offline, because the creating office numbers the task. A transient banner lasts about three seconds without taking focus. A lost response stays uncertain and is resolved by looking up or retrying the same operation, never by creating a second request.

### 1.1 Capture details

- **@name routing:** only at the start of the text, typing @ opens an accessible active-agent combobox with display name, handle, role and actual availability. Expanded state and the active option are exposed to assistive technology. ↑↓ navigates, Enter or Tab selects, Esc closes; moving the caret out of the leading mention dismisses the popup. Selection chooses the individual without inferring an assigned team and removes the whole selected mention span, including a multiword name, keeping the remaining text and punctuation outside its separator. An unresolved leading prefix disables Send with a described field error and never falls back to a rule or copilot route. Choosing a different route explicitly reconciles that prefix rather than hiding its error. An @ elsewhere is ordinary text. Later role changes do not rewrite the original requester.
- **Assignment controls:** team-only, individual-only, both and unrouted states are allowed. On an existing request the complete pair is saved with explicit nulls and the expected revision. On conflict the dialog shows current versus desired pair with actor and time, keeps the draft and requires an explicit retry. Clearing the individual returns the request to its team's queue; clearing the team never infers another.
- **Message instead:** uses the chosen recipient and ordinary chat authorization. It is disabled with Choose who to message when there is no recipient, and with a connection reason when chat is unavailable. It creates no request. The confirmation links to Chat only after the message is accepted.
- **Bug hint:** a copilot or simple terms such as crash, error or broken can offer **Report a bug instead · Not a bug**. Text, project and accountability carry into bug capture; with no project a projectless bug is valid. Not a bug hides the hint for this capture.
- **Similar requests:** up to two accessible request-typed tasks matching title or body in permitted scopes show their references, for example "Looks like T-A9". **Add to…** appends an explicitly shared comment and keeps the existing reporter. Private notes, protected chat and private metadata never appear in suggestions. Where search covers titles only, the dialog says so.
- **Attachments:** hidden until attachment storage exists. Pasted text is accepted; an image-only paste reports that images are unsupported.
- **Unreachable service:** text stays in the open page, visibly Unsaved, with Copy draft and Retry. Nothing promises to keep it after the browser closes. An accepted task is different from a draft.
- **Numbering:** any office creates tasks and numbers them itself ([ADR-0019](../adr/0019-office-coded-task-numbers.md)). Creating a project is an HQ operation.

## 2. Flow 2 · The queue (Work › Requests)

Work › Requests filters the common task list by `type: request`. A project overview links to its filtered view; team views include projectless requests. Rows show reference, title, optional project, accountable team, assigned team, individual, reporter and time, status and the next available action.

```
 Requests  [Status ▾] [Accountable team ▾] [Assigned team ▾] [People ▾]
           [Project / No project ▾] [Search]                  [New request]
 T-A43   Adjacent table booking    Payments | Platform | Mira
 T-A9    Review onboarding         People   | —        | Ana
 T-A44   New starter guide         People   | People   | Unassigned
```

Saved filters distinguish **Owned by my team**, **Assigned to my team**, **Assigned to me** and **Raised by me**. Selecting a team queue does not hide directly assigned individuals from their own queues. A task reference is unique in the organization, so searching by reference finds one task or none.

The queue groups by status and assignment. It never synthesizes "routed" or "accepted" from a non-null assignee: a team-only assignment is queued for that team, not accepted by a person.

Search matches permitted shared titles and bodies and references; an implementation that searches titles only says so. Filters are remembered. Loading shows skeleton rows; empty says "No requests yet. Capture what you need and choose an accountable team"; no match offers Clear filters; a failure keeps the last list with Retry; a stopped or disconnected office is read-only with explicit stale or unsaved labels. Work without a project stays available when the organization has no projects.

## 3. Flow 3 · A request's page

The main column shows the original `details.request_text` and the shared conversation: questions, answers and activity with actors and times. The immutable reporter stays separate from later editors and scopers. Owner questions refer to the actual question and answer comments; waiting spans keep their start, end and explicit recipient rather than being inferred from current assignment. Closing a request never silently discards an unresolved question or span.

The side column shows type and reference, accountable team, optional project, feature and milestone, assigned team and individual, original requester and time, and latest updater. Assignment changes use the same expected-revision atomic pair as every task. Queue and history include previous pairs; home-team or placement changes never rewrite them. A project ownership change uses the recoverable owning-team update and keeps assignment.

**Outcome data:** `details.request_outcome` holds the feature (new or extends, with a stable `feature_id`), documents (`project_id`, `doc_key`), draft tasks (`draft_id`, title, and optional description, `assignee_id`, `team_id`, priority) and an estimate (effort, `cost_usd_cents_min`, `cost_usd_cents_max`). The page shows the canonical feature definition F-6, supporting designs and decisions, editable task proposals and an explicitly attributed estimate range. Unknown cost stays unknown; Build/Run is separate classification. A feature needs a project; a projectless request may stay a team task. Document 03 validates nested references, variant tags and non-negative estimate bounds with min ≤ max. Draft IDs are proposals, not task IDs.

The request's origin is linked through stable identities and task relationships (`relates_to`). The original words and context stay in the request's own task and history. A feature can explain its rationale and link to the request; it does not replace the request as the durable source. A task created from a request has its admitted creator as immutable reporter; the originating request is a separate relationship.

**Private notes:** a separate author-only pane holds several notes with revision-aware create, edit and delete, an explicit publish preview, and no implicit administrator or team-lead access. Publishing creates a shared comment only after the author confirms and normal task-post authorization passes; the private original stays private. No private body, ID, count or author association enters shared task events, queue, search or export, and reassigning the request does not expose a note. See PN-01 to PN-06 in document 05.

**Controls:** Change assignment uses the task assignment operation. **Ask for changes** returns the outcome to the scoper with a comment; **Approve and create**, **Decline…** and **Merge into…** follow decision 7. Approval is by the owner or a person with authority over the request's accountable team, checked by the service; the scoper never approves its own outcome. The same actions are `agent-os` request commands for agents with authority. Routing a request never wakes or schedules an agent. Closing a request with an unresolved owner question asks what to do with the question first.

## 4. States and edge cases

| Case | Behavior |
| --- | --- |
| Context has no team | Keep the draft; require a valid accountable team or a project that derives its owner before save |
| Assigned employee belongs to another team | Valid if active and in the same organization; neither assignment field is rewritten from home membership |
| Chosen agent asleep, paused or disconnected | Show actual availability; assigning promises no wake or session |
| Chosen team archived or employee inactive | Refuse the new assignment with a field error and keep the text; historical assignments and actors are preserved |
| Request rerouted | Record the full old and new pair, actor, time and revision atomically; original requester and reference are unchanged; `route_reason` and `routing_source` are kept separately from assignment |
| Assignment conflict | Change neither field; show the current state and keep the desired state; allow an explicit retry |
| Lost create or assignment response | Resolve by the durable operation identity; show the outcome as unknown until known; never duplicate the operation |
| Office cannot prove its task sequence (for example after a restore) | Creation is refused with the reason until HQ issues the office a new code |
| Control without a handler | Keep the recorded state and explain that the action is unavailable; never synthesize approval or acceptance |
| No code repository | Organization Git requests, features and supporting knowledge stay available |
| Browser closes before save | No background capture; the open-page text was explicitly Unsaved |
| Private store unavailable | The private save fails privately; it never falls back to a shared comment or history |

## 5. UX acceptance criteria (RQS)

RQS IDs are criterion identities; requests themselves use T- references.

- RQS-1 · New request opens an accessible in-page dialog, naming the authorized organization, from navigation or the command palette.
- RQS-2 · With valid project or team context, text plus ⌘Enter saves a request. An accountable team is always required; with no context the dialog asks for one. Project, assignment and classification are optional. Selecting a project derives its accountable team.
- RQS-3 · Suggestions show a valid agent or team and up to two available alternatives with reasons; the person may choose another or save unrouted. Suggestions never overwrite an explicit assignment or infer an assigned team from employee membership.
- RQS-4 · A leading @ has accessible combobox keyboard completion, active-option and expanded state, and caret dismissal. Selecting a multiword name removes the complete selected span, keeps all remaining request text and leaves the assigned team unchanged. An unresolved prefix blocks Send with no rule fallback; an interior @ is ordinary text. Message instead has explicit recipient and connection errors and confirms an accepted message separately from task creation.
- RQS-5 · Rule suggestions are labelled Suggested by rule; without configuration a visible choice remains and no project lead or automatic owner is invented.
- RQS-6 · A bug hint carries text and valid scope into bug capture, allows projectless bugs and can be dismissed for this capture.
- RQS-7 · Requests is a type-filtered task view across project and team scopes, with clear ownership and assignment columns. Grouping never synthesizes acceptance from an assignment.
- RQS-8 · Assignment shows actual availability and never promises an automatic wake.
- RQS-9 · Approving a scoped outcome shows exactly what will be created (feature, documents and tasks with their accountability, type and assignment), lets the approver edit or remove drafts, then creates those records and links them to the request in one recoverable operation. A feature requires a project; the request itself may stay projectless. No task creation or feature change is inferred from a status field.
- RQS-10 · Accepted assignment, content and shared-question changes have durable immutable task history with actors and times.
- RQS-11 · Request text, identity and history persist in organization Git plus the unpublished durable journal. Feature definitions and supporting documents use their stable-ID scopes; rationale links do not replace request history.
- RQS-12 · Scope, ask for changes, approve, decline (with a reason) and merge (into another open request) work the same in the app and in `agent-os`, with actor, time and reason in the request's history; the scoper never approves its own outcome.
- RQS-13 · Up to two similar permitted shared requests show their references; Add to appends a shared comment without changing the reporter or creating a record. Private notes never take part.
- RQS-14 · A team-only route stays in the team's queue with no invented individual owner; assignment alone is never shown as acceptance.
- RQS-15 · Queue filters separate accountable team, assigned team, assignee, reporter and project or no project, remember choices and state actual search coverage. Loading, empty, no-match, retry and read-only states keep the last data. Project and team links open the correct scope.
- RQS-16 · Origin links use stable task, feature and document identities and permitted relationships; `T-` references are complete on their own.
- RQS-17 · Disconnection keeps open-page text labelled Unsaved with Copy draft. An accepted save and pending publication are distinct; success requires the recoverable task, events and receipt. A lost response resolves by the same operation identity, with no duplicate and no browser outbox.
- RQS-18 · An accepted Send, Save or Add to shows a quiet three-second confirmation with the reference and Open, without taking focus. Attachments stay hidden; an image-only paste reports unsupported content.
- RQS-19 · Every request supports all four assignment-pair states at the expected revision. Assignment changes neither owning scope, reference, original creator nor canonical file; home-team moves do not rewrite assignments.
- RQS-20 · All task types use one `T-` sequence per creating office and the same stable-ID rule.
- RQS-21 · Private request notes satisfy PN-01 to PN-06 in document 05: exclusive author access including against administrators, separate durable recovery, no shared existence metadata, explicit publish, revision conflicts and retention and deletion.

## 6. Open questions

- Suggestion quality: how the copilot ranks agents and teams is still open.

## 7. Not in v1

- Request-specific statuses; a request uses the five task statuses.
- Waking an agent when a request is routed to it.
- Attachments on a request. Chat attachments are v1 ([chat UX](chat-ux.md)).
