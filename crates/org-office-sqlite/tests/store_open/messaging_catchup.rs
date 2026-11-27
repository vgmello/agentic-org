//! Channel catch-up ladder conformance (`TASK-99`): read-marker watermarks, delta reads,
//! monotonic `chat.mark_read`, the since-read digest window and thread-first header reads.
//!
//! Every test runs identically on [`EngineChoice::Sqlite`] and [`EngineChoice::Memory`].

use org_proto::chat::{
    ChatChannelCreateParams, ChatChannelMembershipParams, ChatChannelsParams, ChatInboxItem,
    ChatInboxParams, ChatMarkReadParams, ChatMarkReadResult, ChatMessagesParams,
    ChatMessagesResult, ChatSendParams, ConversationSummary, DmOpenParams, SNIPPET_MAX_CHARS,
};
use org_proto::msg::Meta;
use org_proto::{ApiError, ErrorCode};
use org_store::{Ack, Change};

use super::messaging::seed_org_with_two_people;
use super::*;
use org_office::messaging::features::ChannelCatchup;
use org_office::messaging::records::{self, MessageAttachmentRecord, MessageRecord};
use org_office::tasks::ports::TasksRead;
use org_office::testkit_support::store_open::{OfficeEngine, ReadOp, StoreOp};

/// `2026-11-21T12:mm:ss.000Z`, `sec` seconds past noon.
fn at(sec: u32) -> String {
    format!("2026-11-21T12:{:02}:{:02}.000Z", sec / 60, sec % 60)
}

struct Fx {
    store: OfficeStore,
    owner: Id,
    ada: Id,
    next: u64,
}

impl Fx {
    fn new(place: &Place, engine: EngineChoice) -> Self {
        let (store, owner, ada) = seed_org_with_two_people(place, engine);
        let fx = Self {
            store,
            owner,
            ada,
            next: 5000,
        };
        fx.store
            .run(ChannelCreateOp {
                here: identity(),
                owner: fx.owner.clone(),
                caller: fx.owner.clone(),
                params: ChatChannelCreateParams {
                    id: None,
                    name: "ops".into(),
                    members: vec![],
                },
                now_utc: at(0),
            })
            .unwrap()
            .unwrap();
        fx.store
            .run(ChannelMembershipOp {
                owner: fx.owner.clone(),
                caller: fx.ada.clone(),
                params: ChatChannelMembershipParams {
                    channel: "#ops".into(),
                },
                join: true,
                now_utc: at(0),
            })
            .unwrap()
            .unwrap();
        fx
    }

    fn post(&mut self, from: &Id, to: &str, body: &str, sec: u32) -> String {
        self.reply(from, to, body, None, false, sec)
    }

    fn reply(
        &mut self,
        from: &Id,
        to: &str,
        body: &str,
        reply_to: Option<&str>,
        also_in_channel: bool,
        sec: u32,
    ) -> String {
        self.next += 1;
        let mid = id(self.next).as_str().to_owned();
        self.store
            .run(ChatSendOp {
                here: identity(),
                owner: self.owner.clone(),
                caller: from.clone(),
                params: ChatSendParams {
                    id: mid.clone(),
                    to: to.to_owned(),
                    kind: "msg".into(),
                    priority: None,
                    subject: None,
                    body: body.to_owned(),
                    meta: Meta::new(),
                    reply_to: reply_to.map(str::to_owned),
                    also_in_channel,
                    reason: None,
                    attachment_ids: Vec::new(),
                },
                org_seq: 2,
                now_utc: at(sec),
                files: org_office::messaging::files::MessageFiles::none(),
            })
            .unwrap()
            .unwrap();
        mid
    }

    fn channels(&self, viewer: &Id, unread_only: bool) -> Vec<ConversationSummary> {
        let (res, _) = self
            .store
            .read(ListChannelsRead {
                viewer: viewer.clone(),
                owner: self.owner.clone(),
                params: ChatChannelsParams {
                    include_archived: false,
                    unread_only,
                },
                now_utc: at(900),
            })
            .unwrap();
        res.channels
    }

    fn ops(&self, viewer: &Id) -> ConversationSummary {
        self.channels(viewer, false)
            .into_iter()
            .find(|c| c.name.as_deref() == Some("ops"))
            .expect("#ops listed")
    }

    fn list(
        &self,
        caller: &Id,
        params: ChatMessagesParams,
    ) -> Result<ChatMessagesResult, ApiError> {
        self.store
            .read(ChatMessagesRead {
                owner: self.owner.clone(),
                caller: caller.clone(),
                params,
                now_utc: at(900),
            })
            .unwrap()
            .0
    }

    fn mark(
        &self,
        caller: &Id,
        channel: &str,
        up_to: Option<&str>,
        dry_run: bool,
        sec: u32,
    ) -> Result<ChatMarkReadResult, ApiError> {
        self.store
            .run(ChatMarkReadOp {
                here: identity(),
                owner: self.owner.clone(),
                caller: caller.clone(),
                params: ChatMarkReadParams {
                    conversation_id: channel.to_owned(),
                    last_read_message_id: up_to.map(str::to_owned),
                    dry_run,
                },
                now_utc: at(sec),
            })
            .unwrap()
    }

    fn inbox(&self, caller: &Id) -> Vec<ChatInboxItem> {
        let (res, _) = self
            .store
            .read(ChatInboxRead {
                owner: self.owner.clone(),
                caller: caller.clone(),
                params: ChatInboxParams {
                    include_all: true,
                    ..Default::default()
                },
            })
            .unwrap();
        res.unwrap().items
    }

    fn catchup(
        &self,
        caller: &Id,
        channel: &str,
        limit: usize,
    ) -> Result<ChannelCatchup, ApiError> {
        self.store
            .read(ChannelCatchupRead {
                owner: self.owner.clone(),
                caller: caller.clone(),
                channel_ref: channel.to_owned(),
                limit,
                now_utc: at(900),
            })
            .unwrap()
            .0
    }
}

fn ops_params() -> ChatMessagesParams {
    ChatMessagesParams {
        conversation: Some("#ops".into()),
        ..Default::default()
    }
}

fn ids(res: &ChatMessagesResult) -> Vec<&str> {
    res.messages.iter().map(|m| m.id.as_str()).collect()
}

#[test]
fn rung1_channel_list_counts_new_messages_by_watermark_excluding_your_own() {
    for engine in ENGINES {
        let place = Place::new();
        let mut fx = Fx::new(&place, engine);
        let (owner, ada) = (fx.owner.clone(), fx.ada.clone());
        let m1 = fx.post(&owner, "#ops", "one", 1);
        let m2 = fx.post(&owner, "#ops", "two", 2);
        let _m3 = fx.post(&owner, "#ops", "three", 3);
        let a1 = fx.post(&ada, "#ops", "ada's own", 4);

        // No marker yet: every message from someone else is new; your own never are.
        let ops = fx.ops(&ada);
        assert_eq!(ops.new_message_count, 3, "{engine:?}");
        assert_eq!(
            ops.unread_count, 0,
            "{engine:?}: channel posts reach no inbox"
        );
        assert_eq!(ops.last_read_message_id, None);
        assert_eq!(ops.last_read_at, None);
        assert_eq!(ops.last_message_id.as_deref(), Some(a1.as_str()));
        assert_eq!(ops.last_message_at.as_deref(), Some(at(4).as_str()));
        assert_eq!(
            fx.ops(&owner).new_message_count,
            1,
            "{engine:?}: Ada's post"
        );

        // `unread_only` keeps #ops and drops the empty auto channels.
        let unread: Vec<_> = fx
            .channels(&ada, true)
            .into_iter()
            .map(|c| c.name.unwrap_or_default())
            .collect();
        assert_eq!(unread, vec!["ops".to_owned()], "{engine:?}");

        // A watermark counts only what follows it.
        let marked = fx.mark(&ada, "#ops", Some(&m2), false, 10).unwrap();
        assert!(marked.advanced);
        let ops = fx.ops(&ada);
        assert_eq!(ops.new_message_count, 1, "{engine:?}: only m3");
        assert_eq!(ops.last_read_message_id.as_deref(), Some(m2.as_str()));
        assert_eq!(ops.last_read_at.as_deref(), Some(at(10).as_str()));
        assert!(fx.ops(&ada).has_unread());

        fx.mark(&ada, "ops", None, false, 11).unwrap();
        assert_eq!(fx.ops(&ada).new_message_count, 0, "{engine:?}");
        assert!(fx.channels(&ada, true).is_empty(), "{engine:?}");

        // A mention adds an inbox item: `unread_only` keeps the channel while either count is
        // above zero, and marking the channel read leaves the inbox item unread.
        let mention = fx.post(&owner, "#ops", "please look @ada", 12);
        let ops = fx.ops(&ada);
        assert_eq!(
            (ops.new_message_count, ops.unread_count),
            (1, 1),
            "{engine:?}"
        );
        let inbox_before = fx.inbox(&ada);
        assert!(inbox_before.iter().any(|i| i.id == mention));
        fx.mark(&ada, "#ops", None, false, 13).unwrap();
        let ops = fx.ops(&ada);
        assert_eq!(
            (ops.new_message_count, ops.unread_count),
            (0, 1),
            "{engine:?}"
        );
        assert_eq!(fx.inbox(&ada), inbox_before, "{engine:?}: inbox untouched");
        assert_eq!(fx.channels(&ada, true).len(), 1, "{engine:?}");
        let _ = m1;
    }
}

#[test]
fn rung2_delta_reads_are_oldest_first_after_the_anchor_and_refuse_bad_shapes() {
    for engine in ENGINES {
        let place = Place::new();
        let mut fx = Fx::new(&place, engine);
        let (owner, ada) = (fx.owner.clone(), fx.ada.clone());
        // Inserted out of time order: reads follow (created_at, local_seq, id), never insertion.
        let late = fx.post(&owner, "#ops", "late", 30);
        let early = fx.post(&owner, "#ops", "early", 10);
        let mine = fx.post(&ada, "#ops", "mine", 20);
        let tie_a = fx.post(&owner, "#ops", "tie a", 40);
        let tie_b = fx.post(&owner, "#ops", "tie b", 40);

        let all = fx.list(&ada, ops_params()).unwrap();
        assert_eq!(ids(&all), vec![&early, &mine, &late, &tie_a, &tie_b]);
        assert_eq!((all.total, all.next_after_id.as_deref()), (None, None));
        // Plain reads still answer the newest page and page backward by `local_seq`.
        let newest = fx
            .list(
                &ada,
                ChatMessagesParams {
                    limit: Some(2),
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(ids(&newest), vec![&tie_a, &tie_b], "{engine:?}");
        let older = ChatMessagesParams {
            limit: Some(2),
            cursor: newest.next_cursor,
            ..ops_params()
        };
        let older = fx.list(&ada, older).unwrap();
        assert_eq!(ids(&older), vec![&mine, &late], "{engine:?}");
        assert!(older.next_cursor.is_some() && older.total.is_none());

        // since_read with no marker: every message from someone else, oldest first.
        let since = fx
            .list(
                &ada,
                ChatMessagesParams {
                    since_read: true,
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(
            ids(&since),
            vec![&early, &late, &tie_a, &tie_b],
            "{engine:?}"
        );
        assert_eq!(since.total, Some(4));

        // after_id: strictly after the anchor, yours included.
        let after = fx
            .list(
                &ada,
                ChatMessagesParams {
                    after_id: Some(early.clone()),
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(
            ids(&after),
            vec![&mine, &late, &tie_a, &tie_b],
            "{engine:?}"
        );

        // Forward pages: `page_after_id = next_after_id` continues where the read stopped.
        let page1 = fx
            .list(
                &ada,
                ChatMessagesParams {
                    since_read: true,
                    limit: Some(2),
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(ids(&page1), vec![&early, &late]);
        assert_eq!(page1.next_after_id.as_deref(), Some(late.as_str()));
        assert_eq!(page1.total, Some(4));
        let page2 = fx
            .list(
                &ada,
                ChatMessagesParams {
                    since_read: true,
                    page_after_id: page1.next_after_id.clone(),
                    limit: Some(2),
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(ids(&page2), vec![&tie_a, &tie_b], "{engine:?}");
        assert_eq!((page2.total, page2.next_after_id), (Some(4), None));

        // A watermark moves the since-read anchor.
        fx.mark(&ada, "#ops", Some(&late), false, 50).unwrap();
        let since = fx
            .list(
                &ada,
                ChatMessagesParams {
                    since_read: true,
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(ids(&since), vec![&tie_a, &tie_b], "{engine:?}");

        let refused = |params: ChatMessagesParams| fx.list(&ada, params).unwrap_err().code;
        let thread_only = ChatMessagesParams {
            thread_id: Some(early.clone()),
            since_read: true,
            ..Default::default()
        };
        assert_eq!(refused(thread_only), ErrorCode::InvalidRequest);
        let not_uuid = ChatMessagesParams {
            after_id: Some("latest".into()),
            ..ops_params()
        };
        assert_eq!(refused(not_uuid), ErrorCode::InvalidRequest);
        let unknown = ChatMessagesParams {
            after_id: Some(id(9999).as_str().to_owned()),
            ..ops_params()
        };
        assert_eq!(refused(unknown), ErrorCode::NotFound);
        let backward = ChatMessagesParams {
            since_read: true,
            cursor: Some(3),
            ..ops_params()
        };
        assert_eq!(refused(backward), ErrorCode::InvalidRequest);
        let both = ChatMessagesParams {
            top_level_only: true,
            thread_id: Some(early.clone()),
            ..ops_params()
        };
        assert_eq!(refused(both), ErrorCode::InvalidRequest);
        // A plain channel name resolves like `#name`.
        let plain = ChatMessagesParams {
            conversation: Some("ops".into()),
            ..Default::default()
        };
        assert_eq!(
            fx.list(&ada, plain).unwrap().messages.len(),
            5,
            "{engine:?}"
        );
    }
}

/// A message in a DM between Ada and the owner: never part of `#ops`.
fn dm_message(fx: &mut Fx, sec: u32) -> String {
    let dm = fx
        .store
        .run(DmOpenOp {
            here: identity(),
            owner: fx.owner.clone(),
            caller: fx.ada.clone(),
            params: DmOpenParams {
                participants: vec![fx.owner.as_str().to_owned()],
                name: None,
                conversation_id: None,
            },
            now_utc: at(sec),
        })
        .unwrap()
        .unwrap();
    let ada = fx.ada.clone();
    fx.post(&ada, &dm.conversation.id, "dm", sec)
}

#[test]
fn rung2_delta_pages_keep_their_total_and_never_reach_behind_the_anchor() {
    for engine in ENGINES {
        let place = Place::new();
        let mut fx = Fx::new(&place, engine);
        let (owner, ada) = (fx.owner.clone(), fx.ada.clone());
        // Others' posts interleaved with Ada's own, which a since-read read never counts.
        let m0 = fx.post(&owner, "#ops", "m0", 1);
        let _a0 = fx.post(&ada, "#ops", "a0", 2);
        let m1 = fx.post(&owner, "#ops", "m1", 3);
        let m2 = fx.post(&owner, "#ops", "m2", 4);
        let a1 = fx.post(&ada, "#ops", "a1", 5);
        let m3 = fx.post(&owner, "#ops", "m3", 6);
        let page = |fx: &Fx, base: &ChatMessagesParams, cursor: Option<&String>| {
            let params = ChatMessagesParams {
                limit: Some(2),
                page_after_id: cursor.cloned(),
                ..base.clone()
            };
            fx.list(&ada, params).unwrap()
        };

        // since_read: `total` counts from the read marker on every page, not from the cursor.
        let since = ChatMessagesParams {
            since_read: true,
            ..ops_params()
        };
        let p1 = page(&fx, &since, None);
        assert_eq!(ids(&p1), vec![&m0, &m1], "{engine:?}");
        assert_eq!(
            (p1.total, p1.next_after_id.as_deref()),
            (Some(4), Some(m1.as_str())),
            "{engine:?}"
        );
        let p2 = page(&fx, &since, p1.next_after_id.as_ref());
        assert_eq!(ids(&p2), vec![&m2, &m3], "{engine:?}");
        assert_eq!((p2.total, p2.next_after_id), (Some(4), None), "{engine:?}");

        // Arrivals mid-read raise the total and land on later pages: none dropped or repeated.
        let a2 = fx.post(&ada, "#ops", "a2", 7);
        let m4 = fx.post(&owner, "#ops", "m4", 8);
        let m5 = fx.post(&owner, "#ops", "m5", 9);
        let p2 = page(&fx, &since, p1.next_after_id.as_ref());
        assert_eq!(ids(&p2), vec![&m2, &m3], "{engine:?}");
        assert_eq!(
            (p2.total, p2.next_after_id.as_deref()),
            (Some(6), Some(m3.as_str())),
            "{engine:?}"
        );
        let p3 = page(&fx, &since, p2.next_after_id.as_ref());
        assert_eq!(ids(&p3), vec![&m4, &m5], "{engine:?}");
        assert_eq!((p3.total, p3.next_after_id), (Some(6), None), "{engine:?}");

        // after_id stays the anchor (yours included); the cursor only picks the page.
        let after_m1 = ChatMessagesParams {
            after_id: Some(m1.clone()),
            ..ops_params()
        };
        let q1 = page(&fx, &after_m1, None);
        assert_eq!(ids(&q1), vec![&m2, &a1], "{engine:?}");
        assert_eq!(q1.total, Some(6), "{engine:?}: m2 a1 m3 a2 m4 m5");
        let q2 = page(&fx, &after_m1, Some(&m3));
        assert_eq!(ids(&q2), vec![&a2, &m4], "{engine:?}");
        assert_eq!(
            (q2.total, q2.next_after_id.as_deref()),
            (Some(6), Some(m4.as_str())),
            "{engine:?}"
        );

        // A cursor older than the anchor never reaches behind it.
        let after_m2 = ChatMessagesParams {
            after_id: Some(m2.clone()),
            ..ops_params()
        };
        let stale = page(&fx, &after_m2, Some(&m0));
        assert_eq!(ids(&stale), vec![&a1, &m3], "{engine:?}");
        assert_eq!(stale, page(&fx, &after_m2, None), "{engine:?}");
        fx.mark(&ada, "#ops", Some(&m2), false, 20).unwrap();
        let since_stale = page(&fx, &since, Some(&m0));
        assert_eq!(ids(&since_stale), vec![&m3, &m4], "{engine:?}");
        assert_eq!(since_stale.total, Some(3), "{engine:?}: m3 m4 m5");

        // page_after_id pages a delta read only, as a UUIDv7 naming a message in this
        // conversation; an unknown message and another conversation's are refused alike.
        let dm_msg = dm_message(&mut fx, 21);
        let refused = |params: ChatMessagesParams| fx.list(&ada, params).unwrap_err();
        let plain = ChatMessagesParams {
            page_after_id: Some(m1.clone()),
            ..ops_params()
        };
        assert_eq!(refused(plain).code, ErrorCode::InvalidRequest);
        let not_uuid = ChatMessagesParams {
            page_after_id: Some("latest".into()),
            ..since.clone()
        };
        assert_eq!(refused(not_uuid).code, ErrorCode::InvalidRequest);
        let unknown = refused(ChatMessagesParams {
            page_after_id: Some(id(9999).as_str().to_owned()),
            ..since.clone()
        });
        let foreign = refused(ChatMessagesParams {
            page_after_id: Some(dm_msg.clone()),
            ..since.clone()
        });
        assert_eq!(unknown.code, ErrorCode::NotFound, "{engine:?}");
        assert_eq!(
            (foreign.code, foreign.message),
            (unknown.code, unknown.message),
            "{engine:?}"
        );
        let foreign_anchor = ChatMessagesParams {
            after_id: Some(dm_msg),
            ..ops_params()
        };
        assert_eq!(refused(foreign_anchor).code, ErrorCode::NotFound);
    }
}

#[test]
fn rung2_mark_read_defaults_to_latest_moves_only_forward_and_never_touches_the_inbox() {
    for engine in ENGINES {
        let place = Place::new();
        let mut fx = Fx::new(&place, engine);
        let (owner, ada) = (fx.owner.clone(), fx.ada.clone());

        // An empty channel has nothing to mark: a clean no-op that creates no marker.
        let empty = fx.mark(&ada, "#ops", None, false, 1).unwrap();
        assert!(!empty.advanced, "{engine:?}");
        assert_eq!(empty.last_read_message_id, None);
        assert_eq!(fx.ops(&ada).last_read_at, None);

        let m1 = fx.post(&owner, "#ops", "one @ada", 2);
        let m2 = fx.post(&owner, "#ops", "two", 3);
        let m3 = fx.post(&owner, "#ops", "three", 4);
        let inbox_before = fx.inbox(&ada);
        assert_eq!(inbox_before.len(), 1, "{engine:?}: the mention");

        // A dry run answers what it would do and changes nothing.
        let dry = fx.mark(&ada, "#ops", None, true, 5).unwrap();
        assert!(dry.advanced);
        assert_eq!(dry.last_read_message_id.as_deref(), Some(m3.as_str()));
        let ops = fx.ops(&ada);
        assert_eq!(
            (ops.last_read_message_id, ops.new_message_count),
            (None, 3),
            "{engine:?}"
        );

        // Omitted `up_to` is the latest message; by conversation ID, `#name` or name alike.
        let conv_id = fx.ops(&ada).id;
        let marked = fx.mark(&ada, &conv_id, Some(&m2), false, 6).unwrap();
        assert!(marked.advanced);
        assert_eq!(marked.last_read_message_id.as_deref(), Some(m2.as_str()));
        // Older than the marker: a no-op that keeps it (never a regression) and, like a dry
        // run, writes no journal entry at all.
        let head = |fx: &Fx| {
            fx.store.sync().unwrap();
            fx.store.status().flushed
        };
        let before_noops = head(&fx);
        let older = fx.mark(&ada, "ops", Some(&m1), false, 7).unwrap();
        assert!(!older.advanced, "{engine:?}");
        assert_eq!(older.last_read_message_id.as_deref(), Some(m2.as_str()));
        assert_eq!(older.read_at, at(6), "{engine:?}: the marker's own time");
        let same = fx.mark(&ada, "#ops", Some(&m2), false, 8).unwrap();
        assert!(!same.advanced, "{engine:?}: idempotent");
        assert!(fx.mark(&ada, "#ops", None, true, 8).unwrap().advanced);
        assert_eq!(
            head(&fx),
            before_noops,
            "{engine:?}: no-ops and dry runs write nothing"
        );
        let latest = fx.mark(&ada, "#ops", None, false, 9).unwrap();
        assert!(head(&fx) > before_noops, "{engine:?}: a move is journaled");
        assert!(latest.advanced);
        assert_eq!(latest.last_read_message_id.as_deref(), Some(m3.as_str()));
        let ops = fx.ops(&ada);
        assert_eq!(ops.last_read_message_id.as_deref(), Some(m3.as_str()));
        assert_eq!(ops.last_read_at.as_deref(), Some(at(9).as_str()));
        assert_eq!(fx.inbox(&ada), inbox_before, "{engine:?}: inbox untouched");

        // Unknown and foreign messages are refused; so is an unknown channel.
        let unknown = fx.mark(&ada, "#ops", Some(id(9999).as_str()), false, 10);
        assert_eq!(unknown.unwrap_err().code, ErrorCode::NotFound);
        let dm = fx
            .store
            .run(DmOpenOp {
                here: identity(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: DmOpenParams {
                    participants: vec![owner.as_str().to_owned()],
                    name: None,
                    conversation_id: None,
                },
                now_utc: at(10),
            })
            .unwrap()
            .unwrap();
        let dm_msg = fx.post(&ada, &dm.conversation.id, "dm", 11);
        // Another conversation's message is refused exactly like an unknown one: no oracle.
        let foreign = fx.mark(&ada, "#ops", Some(&dm_msg), false, 12).unwrap_err();
        let unknown = fx
            .mark(&ada, "#ops", Some(id(9999).as_str()), false, 12)
            .unwrap_err();
        assert_eq!(foreign.code, ErrorCode::NotFound, "{engine:?}");
        assert_eq!(foreign.message, unknown.message, "{engine:?}");
        let nowhere = fx.mark(&ada, "#nowhere", None, false, 12);
        assert_eq!(nowhere.unwrap_err().code, ErrorCode::NotFound);
    }
}

#[test]
fn rung3_catchup_window_slices_oldest_first_and_counts_what_remains() {
    for engine in ENGINES {
        let place = Place::new();
        let mut fx = Fx::new(&place, engine);
        let (owner, ada) = (fx.owner.clone(), fx.ada.clone());

        let empty = fx.catchup(&ada, "#ops", 200).unwrap();
        assert_eq!((empty.new_total, empty.remaining_unread()), (0, 0));
        assert_eq!(empty.through_message_id(), None);

        let mut posted = Vec::new();
        for n in 0..203u32 {
            let from = if n == 5 { &ada } else { &owner };
            posted.push(fx.post(from, "#ops", &format!("post {n}"), n));
        }
        let others: Vec<&String> = posted
            .iter()
            .enumerate()
            .filter(|(n, _)| *n != 5)
            .map(|(_, mid)| mid)
            .collect();

        // At most 200 per digest, oldest first; the rest stays counted, never skipped.
        let first = fx.catchup(&ada, "#ops", 500).unwrap();
        assert_eq!(first.messages.len(), 200, "{engine:?}");
        assert_eq!(first.new_total, 202);
        assert_eq!(first.remaining_unread(), 2);
        assert_eq!(first.messages[0].id, *others[0]);
        assert_eq!(
            first.through_message_id().as_deref(),
            Some(others[199].as_str())
        );
        assert!(
            first.messages.iter().all(|m| m.id != posted[5]),
            "own post skipped"
        );

        // A digest is a pure read; marking through its last message moves on to the rest.
        assert_eq!(fx.ops(&ada).last_read_message_id, None, "{engine:?}");
        let through = first.through_message_id().unwrap();
        fx.mark(&ada, "#ops", Some(&through), false, 300).unwrap();
        let second = fx.catchup(&ada, "#ops", 3).unwrap();
        assert_eq!(second.messages.len(), 2);
        assert_eq!((second.new_total, second.remaining_unread()), (2, 0));
        assert_eq!(
            second.last_read_message_id.as_deref(),
            Some(through.as_str())
        );

        fx.mark(&ada, "#ops", None, false, 301).unwrap();
        let done = fx.catchup(&ada, "#ops", 200).unwrap();
        assert!(done.messages.is_empty());
        assert_eq!(done.through_message_id(), done.last_read_message_id);
        assert_eq!(
            done.through_message_id().as_deref(),
            Some(posted[202].as_str())
        );

        let not_channel = fx
            .store
            .run(DmOpenOp {
                here: identity(),
                owner: owner.clone(),
                caller: ada.clone(),
                params: DmOpenParams {
                    participants: vec![owner.as_str().to_owned()],
                    name: None,
                    conversation_id: None,
                },
                now_utc: at(400),
            })
            .unwrap()
            .unwrap();
        let refused = fx.catchup(&ada, &not_channel.conversation.id, 10);
        assert_eq!(refused.unwrap_err().code, ErrorCode::InvalidRequest);
    }
}

/// Commits a message file as written, so a test can hold attachments no send path makes yet.
struct InjectMessage(MessageRecord);

impl StoreOp for InjectMessage {
    type Out = Result<org_store::Reply, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let rec = self.0;
        store.submit(None, false, move |_uow| {
            Ok(Change::new(Ack::Committed).create(
                records::message_path(&rec.conversation_id, &rec.id),
                records::encode_json(&rec),
            ))
        })
    }
}

struct MessageById(String);

impl ReadOp for MessageById {
    type Out = Option<MessageRecord>;
    fn read<S: TasksRead>(self, snapshot: &S) -> Result<Self::Out, StoreError> {
        Ok(snapshot.message(&self.0)?)
    }
}

#[test]
fn rung4_thread_headers_top_level_snippets_and_attachment_counts() {
    for engine in ENGINES {
        let place = Place::new();
        let mut fx = Fx::new(&place, engine);
        let (owner, ada) = (fx.owner.clone(), fx.ada.clone());
        let root = fx.post(&owner, "#ops", "Root 🦀 question\n\twith tabs", 1);
        let r1 = fx.reply(&ada, "#ops", "first reply", Some(&root), false, 2);
        let other = fx.post(&owner, "#ops", &"long 漢字 text ".repeat(40), 3);
        fx.mark(&ada, "#ops", None, false, 4).unwrap();
        // After the watermark: a reply inside the older thread, a broadcast reply, a new post.
        let r2 = fx.reply(&owner, "#ops", "late reply", Some(&root), false, 5);
        let r3 = fx.reply(&owner, "#ops", "also here", Some(&root), true, 6);
        let fresh = fx.post(&owner, "#ops", "fresh top-level post", 7);

        let full = fx.list(&ada, ops_params()).unwrap();
        let root_view = &full.messages[0];
        assert_eq!(root_view.id, root);
        assert_eq!(root_view.reply_count, 3, "{engine:?}");
        assert_eq!(root_view.latest_reply_at.as_deref(), Some(at(6).as_str()));
        assert!(root_view.body.is_some(), "full reads carry the body");
        assert_eq!(
            (root_view.snippet.as_ref(), root_view.body_bytes),
            (None, None)
        );
        assert_eq!(full.messages[1].reply_count, 0);

        // Top-level only: roots and broadcast replies; thread replies stay in reply_count.
        let top = fx
            .list(
                &ada,
                ChatMessagesParams {
                    top_level_only: true,
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(ids(&top), vec![&root, &other, &r3, &fresh], "{engine:?}");
        // Since-read: the late reply in the older thread is new; top-level-only hides it but
        // the root still reports it through reply_count and latest_reply_at.
        let since = fx
            .list(
                &ada,
                ChatMessagesParams {
                    since_read: true,
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(ids(&since), vec![&r2, &r3, &fresh], "{engine:?}");
        let since_top = fx
            .list(
                &ada,
                ChatMessagesParams {
                    since_read: true,
                    top_level_only: true,
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(ids(&since_top), vec![&r3, &fresh], "{engine:?}");

        // Headers only: no body or meta, a one-line char-safe snippet and the body's bytes.
        let heads = fx
            .list(
                &ada,
                ChatMessagesParams {
                    headers_only: true,
                    ..ops_params()
                },
            )
            .unwrap();
        for (view, full_view) in heads.messages.iter().zip(&full.messages) {
            let body = full_view.body.as_deref().unwrap();
            assert_eq!(view.body, None);
            assert!(view.meta.is_empty());
            assert_eq!(view.body_bytes, Some(body.len() as u64));
            let snip = view.snippet.as_deref().unwrap();
            assert!(snip.chars().count() <= SNIPPET_MAX_CHARS, "{snip}");
            assert!(!snip.contains(['\n', '\t']), "{snip}");
        }
        assert_eq!(
            heads.messages[0].snippet.as_deref(),
            Some("Root 🦀 question with tabs")
        );
        assert!(heads.messages[2].snippet.as_deref().unwrap().ends_with('…'));
        assert_eq!(heads.messages[0].reply_count, 3);
        let _ = r1;

        // Attachment counts come from the message's attachment list.
        let base = fx
            .store
            .read(MessageById(fresh.clone()))
            .unwrap()
            .0
            .unwrap();
        let mut with_files = base.clone();
        with_files.id = id(7777).as_str().to_owned();
        with_files.origin_sequence += 1000;
        with_files.created_at = at(8);
        with_files.attachments = (0..2)
            .map(|n| MessageAttachmentRecord {
                id: format!("att-{n}"),
                content_ref: format!("blob-{n}"),
                media_type: "text/plain".into(),
                byte_count: 3,
                content_sha256: "0".repeat(64),
                ..Default::default()
            })
            .collect();
        with_files.content_sha256 = with_files.compute_content_sha256();
        let reply = fx.store.run(InjectMessage(with_files)).unwrap();
        assert_eq!(reply.outcome, Outcome::Completed, "{engine:?}");
        let after = fx
            .list(
                &ada,
                ChatMessagesParams {
                    after_id: Some(fresh.clone()),
                    ..ops_params()
                },
            )
            .unwrap();
        assert_eq!(after.messages.len(), 1, "{engine:?}");
        assert_eq!(after.messages[0].attachment_count, 2, "{engine:?}");
        assert_eq!(full.messages[0].attachment_count, 0);
    }
}

#[test]
fn ax23_since_read_headers_only_slashes_the_catchup_payload() {
    for engine in ENGINES {
        let place = Place::new();
        let mut fx = Fx::new(&place, engine);
        let (owner, ada) = (fx.owner.clone(), fx.ada.clone());
        let long = "Status: the migration plan is on track; details follow.\n".repeat(36);
        let mut last_read = String::new();
        for n in 0..30u32 {
            last_read = fx.post(&owner, "#ops", &format!("Update {n}\n{long}"), n);
        }
        fx.mark(&ada, "#ops", Some(&last_read), false, 40).unwrap();
        let root = fx.post(&owner, "#ops", &format!("Decision\n{long}"), 41);
        fx.reply(
            &owner,
            "#ops",
            &format!("Detail\n{long}"),
            Some(&root),
            false,
            42,
        );
        fx.post(&owner, "#ops", &format!("Blocker\n{long}"), 43);

        let bytes = |params: ChatMessagesParams| {
            serde_json::to_vec(&fx.list(&ada, params).unwrap())
                .unwrap()
                .len()
        };
        let full = bytes(ChatMessagesParams {
            limit: Some(100),
            ..ops_params()
        });
        let headers = bytes(ChatMessagesParams {
            limit: Some(100),
            headers_only: true,
            ..ops_params()
        });
        let catchup = bytes(ChatMessagesParams {
            since_read: true,
            top_level_only: true,
            headers_only: true,
            ..ops_params()
        });
        assert!(
            headers * 3 < full,
            "{engine:?}: headers {headers} vs full {full}"
        );
        assert!(
            catchup * 20 < full,
            "{engine:?}: catch-up {catchup} vs full {full}"
        );
    }
}
