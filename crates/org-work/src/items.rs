//! Shared item calls (§4.6): comments, watching, links and deleting.

use org_proto::org::OWNER_HANDLE;
use org_proto::work::{
    CommentKind, ItemKind, ItemStatus, LinkKind, LinkTarget, WorkCommentParams, WorkDeleteParams,
    WorkEventKind, WorkLinkParams, WorkUnlinkParams, WorkWatchParams, WORK_MAX_LINKS,
    WORK_TEXT_MAX_CHARS,
};

use crate::aspects::Aspect;
use crate::common::{load, not_deleted, require_ulid, unused};
use crate::engine::{Ctx, Origin, Plan};
use crate::fail::{already_exists, invalid, not_found, Fail};
use crate::fixes::{self, Edge};
use crate::model::{link_view, StoredComment, StoredLink};
use crate::text::{multi_line, one_line};
use crate::waiting;

/// The longest `doc_key` a link may name, in characters.
pub const DOC_KEY_MAX_CHARS: usize = 256;

/// `work.comment`: a note, a question (optionally to a member) or an answer to a question on
/// the same item. A question to the owner opens a "waiting on the owner" span; its first answer
/// closes it (bookkeeping events in the same call).
pub(crate) fn comment(ctx: &mut Ctx, p: &WorkCommentParams) -> Result<Plan, Fail> {
    let item = load(ctx, &p.item_id)?;
    let kind = p.comment_kind.unwrap_or(CommentKind::Note);
    let question = match (&p.answers, kind) {
        (Some(q), CommentKind::Answer) => Some(
            item.comment(q)
                .filter(|c| c.kind == CommentKind::Question)
                .cloned()
                .ok_or_else(|| not_found("no such question on this item"))?,
        ),
        _ => None,
    };

    require_ulid("comment_id", &p.comment_id)?;
    let body = multi_line("body", &p.body, WORK_TEXT_MAX_CHARS)?;
    match kind {
        CommentKind::Note | CommentKind::Question | CommentKind::Answer => {}
        _ => return Err(invalid("`comment_kind` must be note, question or answer")),
    }
    if p.asks.is_some() && kind != CommentKind::Question {
        return Err(invalid("`asks` goes only with a question"));
    }
    if p.answers.is_some() && kind != CommentKind::Answer {
        return Err(invalid("`answers` goes only with an answer"));
    }
    if kind == CommentKind::Answer && p.answers.is_none() {
        return Err(invalid("an answer names the question it answers"));
    }
    let owner_id = ctx.org.owner_id()?;
    let asks = match p.asks.as_deref() {
        Some(OWNER_HANDLE) => Some(owner_id.clone()),
        Some(member) => {
            ctx.check_recipient(member)?;
            Some(member.to_owned())
        }
        None => None,
    };
    not_deleted(&item)?;
    unused(ctx, &[p.comment_id.as_str()])?;

    let caller = ctx.caller_id().to_owned();
    let mut draft = ctx.draft(item);
    let watch = p.watch && !draft.item.watchers.contains(&caller);
    if watch {
        draft.item.watchers.push(caller);
    }
    draft.item.comments.push(StoredComment {
        id: p.comment_id.clone(),
        kind,
        asks: asks.clone(),
        answered: false,
    });
    let cause = draft.record(
        WorkEventKind::Comment {
            comment_id: p.comment_id.clone(),
            body,
            comment_kind: kind,
            asks: asks.clone(),
            answers: p.answers.clone(),
            watch,
        },
        Origin::Caller,
    );
    if kind == CommentKind::Question && asks.as_deref() == Some(owner_id.as_str()) {
        waiting::question_asked(&mut draft, &owner_id, &p.comment_id, cause);
    } else if let Some(q) = question {
        if let Some(c) = draft.item.comments.iter_mut().find(|c| c.id == q.id) {
            c.answered = true;
        }
        if q.asks.as_deref() == Some(owner_id.as_str()) {
            waiting::question_answered(&mut draft, &owner_id, &q.id, cause);
        }
    }
    Ok(Plan::of(vec![draft]))
}

/// `work.watch`: start or stop watching as the caller. The creator always watches. A call that
/// changes nothing records nothing.
pub(crate) fn watch(ctx: &mut Ctx, p: &WorkWatchParams) -> Result<Plan, Fail> {
    let item = load(ctx, &p.item_id)?;
    not_deleted(&item)?;
    let caller = ctx.caller_id().to_owned();
    if !p.watching && caller == item.created_by {
        return Err(invalid("the creator always watches the item"));
    }
    if item.watchers.contains(&caller) == p.watching {
        return Ok(Plan::unchanged(item));
    }
    let mut draft = ctx.draft(item);
    if p.watching {
        draft.item.watchers.push(caller.clone());
    } else {
        draft.item.watchers.retain(|w| *w != caller);
    }
    draft.record(
        WorkEventKind::Watched {
            member_id: caller,
            watching: p.watching,
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

/// `work.link`: a `relates_to` link from an item to another item or to a doc, or (phase 2) a
/// `fixes` link from a task to a bug of its project.
pub(crate) fn link(ctx: &mut Ctx, p: &WorkLinkParams) -> Result<Plan, Fail> {
    let item = load(ctx, &p.from_item_id)?;
    let target = match &p.to {
        LinkTarget::Item { item_id } => Some(
            ctx.txn
                .get(item_id)?
                .ok_or_else(|| not_found("no such work item to link to"))?,
        ),
        LinkTarget::Doc { project_id, .. } => {
            ctx.org
                .project(project_id)?
                .ok_or_else(|| not_found("no such project"))?;
            None
        }
        _ => None,
    };

    require_ulid("link_id", &p.link_id)?;
    match p.kind {
        LinkKind::RelatesTo => {}
        LinkKind::Fixes => {
            if item.kind != ItemKind::Task {
                return Err(invalid("a `fixes` link goes from a task to a bug"));
            }
            match (&target, &item.project_id) {
                (Some(bug), Some(project)) => crate::bugs::check_fixable(project, bug)?,
                _ => return Err(invalid("a `fixes` link goes from a task to a bug")),
            }
        }
        LinkKind::PaysBack => {
            return Err(invalid("`pays_back` links come with debt (a later phase)"))
        }
        LinkKind::PartOf | LinkKind::FromRequest | LinkKind::ReportedFrom => {
            return Err(invalid(
                "links of this kind are made only by their own calls",
            ))
        }
        _ => return Err(invalid("unknown link kind")),
    }
    let to = match &p.to {
        LinkTarget::Item { item_id } => {
            if *item_id == item.id {
                return Err(invalid("an item can't link to itself"));
            }
            LinkTarget::Item {
                item_id: item_id.clone(),
            }
        }
        LinkTarget::Doc {
            project_id,
            doc_key,
        } => LinkTarget::Doc {
            project_id: project_id.clone(),
            doc_key: one_line("doc_key", doc_key, DOC_KEY_MAX_CHARS)?,
        },
        _ => return Err(invalid("unknown link target")),
    };
    not_deleted(&item)?;
    if target.as_ref().is_some_and(|t| t.deleted) {
        return Err(invalid("the item to link to is deleted"));
    }
    if item.live_links().count() >= WORK_MAX_LINKS {
        return Err(invalid(format!(
            "an item has at most {WORK_MAX_LINKS} links"
        )));
    }
    unused(ctx, &[p.link_id.as_str()])?;
    if item.live_links().any(|l| l.kind == p.kind && l.to == to) {
        return Err(already_exists("the item already has this link"));
    }

    let stored = StoredLink {
        id: p.link_id.clone(),
        kind: p.kind,
        to,
        created_by: ctx.caller_id().to_owned(),
        created_at_ms: 0,
        removed: false,
    };
    let mut draft = ctx.draft(item);
    let stored = StoredLink {
        created_at_ms: draft.at_ms(),
        ..stored
    };
    let view = link_view(&draft.item.id, &stored, &|_| false);
    draft.item.links.push(stored);
    let cause = draft.record(WorkEventKind::Linked { link: view }, Origin::Caller);
    // A task already in progress that now fixes a triaged bug starts it (§2.4).
    let mut drafts = vec![];
    if p.kind == LinkKind::Fixes && draft.item.status == ItemStatus::InProgress {
        fixes::follow_task(ctx, &mut drafts, &draft.item, &[Edge::Started], &cause)?;
    }
    drafts.insert(0, draft);
    Ok(Plan::of(drafts))
}

/// `work.unlink`: removes a link. `part_of` (use `checklist.unlink_task`) and `from_request`
/// links can't be removed this way.
pub(crate) fn unlink(ctx: &mut Ctx, p: &WorkUnlinkParams) -> Result<Plan, Fail> {
    let item = match ctx.txn.item_of_link(&p.link_id)? {
        Some(item) if item.live_links().any(|l| l.id == p.link_id) => item,
        _ => return Err(not_found("no such link")),
    };
    let link = item
        .live_links()
        .find(|l| l.id == p.link_id)
        .expect("loaded with the link");
    match link.kind {
        LinkKind::PartOf => {
            return Err(invalid(
                "a `part_of` link ends with `checklist.unlink_task`",
            ))
        }
        LinkKind::FromRequest => return Err(invalid("a `from_request` link can't be removed")),
        _ => {}
    }
    not_deleted(&item)?;
    let mut draft = ctx.draft(item);
    if let Some(l) = draft.item.links.iter_mut().find(|l| l.id == p.link_id) {
        l.removed = true;
    }
    draft.record(
        WorkEventKind::Unlinked {
            link_id: p.link_id.clone(),
        },
        Origin::Caller,
    );
    Ok(Plan::of(vec![draft]))
}

/// `work.delete` (app-only, checked before this): the item becomes a tombstone. Its number
/// stays used; a made-task step whose task this is becomes a plain step again.
pub(crate) fn delete(ctx: &mut Ctx, p: &WorkDeleteParams) -> Result<Plan, Fail> {
    let item = load(ctx, &p.item_id)?;
    // Any change but the office's bookkeeping makes a delete stale.
    ctx.check_expected(&item, p.expected_version, |a| *a != Aspect::Waiting)?;
    not_deleted(&item)?;

    let parent = match item.part_of().map(|l| l.to.clone()) {
        Some(LinkTarget::Item { item_id }) => ctx.txn.get(&item_id)?,
        _ => None,
    };
    let mut draft = ctx.draft(item);
    draft.item.deleted = true;
    let cause = draft.record(WorkEventKind::Deleted, Origin::Caller);
    // The time figures ignore a deleted item from here on: its spans end with it (§2.8).
    waiting::round_closed(&mut draft, &cause);
    let mut drafts = vec![draft];
    if let Some(parent) = parent.filter(|p| !p.deleted) {
        let task_id = p.item_id.clone();
        let step_id = parent
            .live_steps()
            .find(|s| s.task_id.as_deref() == Some(task_id.as_str()))
            .map(|s| s.id.clone());
        if let Some(step_id) = step_id {
            let mut pd = ctx.draft(parent);
            let s = pd.item.step_mut(&step_id).expect("found above");
            s.task_id = None;
            s.version += 1;
            pd.record(
                WorkEventKind::StepTaskUnlinked { step_id, task_id },
                Origin::Caller,
            );
            drafts.push(pd);
        }
    }
    Ok(Plan::of(drafts))
}
