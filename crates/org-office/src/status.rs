//! `status.set` and the status lines `who` shows: an agent's own one-line status, kept in
//! `office.db` so it survives a restart. The rules are `org_proto::directory::StatusSet`'s.
//! Presence is not reported yet, so a member with a line reads as offline (or asleep) beside it.

use std::collections::HashMap;

use org_proto::directory::{
    MemberPresence, StatusSetParams, StatusSetResult, WhoParams, WhoResult,
};
use org_proto::ApiError;

use crate::error::OfficeError;
use crate::office::Office;
use crate::organization::check_status as check;
use crate::organization::directory::{MemberRuntime, RuntimeView};
pub use crate::organization::presence_after_status;
use crate::store::status;
use crate::time;

/// What the cards read: each stored line as the member's runtime, nothing else known.
struct StoredLines(HashMap<String, MemberRuntime>);

impl RuntimeView for StoredLines {
    fn runtime_of(&self, member_id: &str) -> Option<MemberRuntime> {
        self.0.get(member_id).cloned()
    }
}

impl Office {
    /// Sets or clears `member_id`'s own status line (`status.set`). The member is the caller's
    /// bound one, never read from the call. `Err(ApiError)` inside the `Ok` is the caller's
    /// mistake; the outer error is storage.
    pub fn set_status(
        &self,
        member_id: &str,
        params: &StatusSetParams,
    ) -> Result<Result<StatusSetResult, ApiError>, OfficeError> {
        let line = match check(params) {
            Ok(line) => line,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let now_ms = time::now_ms();
        let now = time::format_epoch_ms(now_ms);
        let stored = line.map(|(text, ttl)| (text, time::format_epoch_ms(now_ms + ttl)));
        let (member, at) = (member_id.to_owned(), now.clone());
        let to_write = stored.clone();
        self.local_store().write(move |db| {
            status::write(
                db,
                &member,
                to_write.as_ref().map(|(t, e)| (t.as_str(), e.as_str())),
                &at,
            )?;
            Ok(((), None))
        })?;
        Ok(Ok(match stored {
            Some((text, expires_at)) => StatusSetResult {
                status: Some(text),
                expires_at: Some(expires_at),
            },
            None => StatusSetResult {
                status: None,
                expires_at: None,
            },
        }))
    }

    /// `who`, with the status lines members have set.
    pub fn who_with_status(
        &self,
        params: &WhoParams,
    ) -> Result<Result<WhoResult, ApiError>, OfficeError> {
        let lines = self.local_store().read(status::all)?;
        let runtime = StoredLines(
            lines
                .into_iter()
                .map(|line| {
                    (
                        line.member_id,
                        MemberRuntime {
                            presence: MemberPresence::Offline,
                            last_seen_at: None,
                            status: Some(line.text),
                            status_expires_at: Some(line.expires_at),
                            current_task_id: None,
                        },
                    )
                })
                .collect(),
        );
        self.who(params, &runtime)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_proto::directory::STATUS_MAX_CHARS;

    fn text(text: &str) -> StatusSetParams {
        StatusSetParams {
            text: Some(text.to_owned()),
            ttl_ms: None,
        }
    }

    use org_proto::directory::WhoParams;
    use tempfile::TempDir;

    use crate::key::Key;
    use crate::model::{NewOffice, OfficeRole};

    fn office(dir: &TempDir) -> Office {
        Office::create(
            dir.path().join("offices").join("OFFICE1"),
            &NewOffice {
                office_id: "OFFICE1".into(),
                office_name: "HQ".into(),
                role: OfficeRole::Hq,
                org_id: "ORG1".into(),
                org_name: "Acme".into(),
            },
            Key::from_bytes([5; 32]),
        )
        .expect("an office")
    }

    fn owner_id(office: &Office) -> String {
        office.org_snapshot().unwrap().members[0].id.clone()
    }

    fn card_status(office: &Office) -> Option<String> {
        let who = office
            .who_with_status(&WhoParams::default())
            .unwrap()
            .unwrap();
        who.members[0].status.clone()
    }

    #[test]
    fn a_status_line_shows_on_the_card_survives_a_restart_and_is_cleared_or_replaced() {
        let dir = TempDir::new().unwrap();
        let office = office(&dir);
        let member = owner_id(&office);
        assert_eq!(card_status(&office), None);

        let set = office
            .set_status(&member, &text(" Drafting "))
            .unwrap()
            .unwrap();
        assert_eq!(set.status.as_deref(), Some("Drafting"));
        assert!(set.expires_at.is_some());
        assert_eq!(card_status(&office).as_deref(), Some("Drafting"));

        // A restart keeps it.
        let folder = dir.path().join("offices").join("OFFICE1");
        drop(office);
        let office = Office::open(&folder, Key::from_bytes([5; 32])).unwrap();
        assert_eq!(card_status(&office).as_deref(), Some("Drafting"));

        // Replaced, then cleared.
        office
            .set_status(&member, &text("Reviewing"))
            .unwrap()
            .unwrap();
        assert_eq!(card_status(&office).as_deref(), Some("Reviewing"));
        let cleared = office
            .set_status(&member, &StatusSetParams::default())
            .unwrap()
            .unwrap();
        assert_eq!((cleared.status, cleared.expires_at), (None, None));
        assert_eq!(card_status(&office), None);
    }

    fn rows(office: &Office) -> Vec<String> {
        let mut ids: Vec<String> = office
            .local_store()
            .read(status::all)
            .unwrap()
            .into_iter()
            .map(|line| line.member_id)
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn a_write_drops_the_lines_that_lapsed_and_keeps_the_rest() {
        let dir = TempDir::new().unwrap();
        let office = office(&dir);
        let write = |member: &str, line: Option<(&str, &str)>, now: &str| {
            let (member, line, now) = (
                member.to_owned(),
                line.map(|(a, b)| (a.to_owned(), b.to_owned())),
                now.to_owned(),
            );
            office
                .local_store()
                .write(move |db| {
                    status::write(
                        db,
                        &member,
                        line.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
                        &now,
                    )?;
                    Ok(((), None))
                })
                .unwrap();
        };
        let nine = "2026-11-08T09:00:00.000Z";
        write("A", Some(("a", "2026-11-08T10:00:00.000Z")), nine);
        write("B", Some(("b", "2026-11-08T12:00:00.000Z")), nine);
        write("C", Some(("c", "2026-11-08T13:00:00.000Z")), nine);
        assert_eq!(rows(&office), ["A", "B", "C"]);
        // At exactly A's expiry it is gone (a line lasts until, not including, its instant).
        write(
            "D",
            Some(("d", "2026-11-08T14:00:00.000Z")),
            "2026-11-08T10:00:00.000Z",
        );
        assert_eq!(rows(&office), ["B", "C", "D"]);
        // One millisecond before B's expiry it is kept; clearing C removes only C.
        write("C", None, "2026-11-08T11:59:59.999Z");
        assert_eq!(rows(&office), ["B", "D"]);
    }

    #[test]
    fn an_office_db_at_schema_two_gets_the_status_table_when_it_is_opened() {
        use crate::store::{Opening, Store};
        let dir = TempDir::new().unwrap();
        let folder = dir.path().join("offices").join("OFFICE1");
        drop(office(&dir));
        // Put office.db back as version 2 made it: no status or work tables.
        let key = Key::from_bytes([5; 32]);
        {
            let store = Store::open(
                &folder.join("office.db"),
                &key,
                Opening::Existing,
                &crate::office::OFFICE_SCHEMA,
            )
            .unwrap();
            store
                .write(|db| {
                    db.execute_batch(
                        "DROP TABLE work_origin_log; DROP TABLE work_origin_counter;
                         DROP TABLE work_feed; DROP TABLE member_status; PRAGMA user_version = 2;",
                    )
                    .map_err(crate::error::OfficeError::storage)?;
                    Ok(((), None))
                })
                .unwrap();
        }
        let office = Office::open(&folder, key).unwrap();
        let member = owner_id(&office);
        office.set_status(&member, &text("back")).unwrap().unwrap();
        assert_eq!(card_status(&office).as_deref(), Some("back"));
    }

    #[test]
    fn waking_the_followers_ends_their_wait_without_a_commit() {
        let dir = TempDir::new().unwrap();
        let office = std::sync::Arc::new(office(&dir));
        let seen = office.org_change_token();
        let waiter = {
            let office = std::sync::Arc::clone(&office);
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let token = office.wait_for_org_change(seen, std::time::Duration::from_secs(20));
                (token, started.elapsed())
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        office.wake_followers();
        let (token, took) = waiter.join().unwrap();
        assert_ne!(token, seen, "the token moved");
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
    }

    #[test]
    fn a_line_that_lapsed_is_not_shown_and_a_refused_call_changes_nothing() {
        let dir = TempDir::new().unwrap();
        let office = office(&dir);
        let member = owner_id(&office);
        office
            .set_status(
                &member,
                &StatusSetParams {
                    text: Some("Short".into()),
                    ttl_ms: Some(1),
                },
            )
            .unwrap()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(card_status(&office), None, "it lapsed");

        office.set_status(&member, &text("Kept")).unwrap().unwrap();
        for bad in [text("two\nlines"), text(&"x".repeat(STATUS_MAX_CHARS + 1))] {
            assert!(office.set_status(&member, &bad).unwrap().is_err());
        }
        assert_eq!(card_status(&office).as_deref(), Some("Kept"));
    }
}
