//! The "Your processes" section of an agent's instructions (processes.md §3.4, PROC-7, PROC-12):
//! the set, the processes changed since the agent last read them, and the moment prompts, within
//! a 24 KiB cap. Pure: the office says what the agent has read and gives the texts.

use std::collections::{BTreeMap, BTreeSet};

use crate::file::{Moments, Status};
use crate::place::{Place, ProcessKey};
use crate::resolve::{place_name, Conflict, EffectiveSet, Winner};

type Names = BTreeMap<Place, String>;

/// The cap on the whole section, in bytes of UTF-8, headings and prompts included (§3.4).
const CAP: usize = 24 * 1024;

/// A guidance moment (decisions-and-tech-debt-ux.md §6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Moment {
    /// Starting a task.
    StartingTask,
    /// Waking up.
    WakingUp,
    /// Finishing a task.
    FinishingTask,
}

impl Moment {
    /// Its name in the app.
    pub fn title(self) -> &'static str {
        match self {
            Moment::StartingTask => "Starting a task",
            Moment::WakingUp => "Waking up",
            Moment::FinishingTask => "Finishing a task",
        }
    }

    fn prompt(self, moments: &Moments) -> Option<&String> {
        match self {
            Moment::StartingTask => moments.starting_task.as_ref(),
            Moment::WakingUp => moments.waking_up.as_ref(),
            Moment::FinishingTask => moments.finishing_task.as_ref(),
        }
    }
}

/// A winning file as the office read it: its git blob and its full text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileText {
    /// The blob id.
    pub blob: String,
    /// The whole file.
    pub text: String,
}

/// What the office knows for one delivery (a start, a wake or `task start`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderInput {
    /// Per key, the blobs the agent has read. For a fresh context (processes.md §3.5) the
    /// caller passes it empty, so every applying text is offered again.
    pub read: BTreeMap<ProcessKey, BTreeSet<String>>,
    /// Per key, the winning file's blob and full text. The caller guarantees they are the
    /// winning file's: render can't check it, since a [`crate::Winner`] carries no blob.
    pub texts: BTreeMap<ProcessKey, FileText>,
    /// The moments On for this delivery, each with its guidance text; a moment that is Off is
    /// absent.
    pub moments: BTreeMap<Moment, String>,
    /// The names the app shows for places, for place labels and conflict lines, as
    /// [`crate::Inputs::names`].
    pub names: BTreeMap<Place, String>,
}

/// The rendered section and what it delivered. Built only by [`render`], so `delivered` names
/// exactly the full texts in `text`:
///
/// ```compile_fail,E0451
/// use org_process::Rendered;
/// let made = Rendered { text: String::new(), delivered: vec![] };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    text: String,
    delivered: Vec<(ProcessKey, String)>,
}

impl Rendered {
    /// The section; empty when it has nothing to say.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The keys and blobs whose full text is in [`Rendered::text`], in the text's order: what
    /// counts as read once the runtime accepts the start or wake. A text named instead, or left
    /// out, is never here.
    pub fn delivered(&self) -> &[(ProcessKey, String)] {
        &self.delivered
    }
}

/// The "Your processes" section for `set`, at most 24 KiB of UTF-8 (prompts included). In order,
/// each part getting the room the parts before it leave, less the room kept for each later part
/// that has something (changed processes: the changed heading and its count line; On moments:
/// their count line), so a later part is never dropped without a word:
///
/// 1. The set: one line per applying (`active`) winner, with its name, place ("org", a team's
///    or project's name from `input.names`, else its team id or project key, or "private"),
///    version and summary, by level (organization, team, project, agent), then key; then each
///    turned-off key and each conflict, in the set's order: the longest run, from the first,
///    that fits with "and N more: run `org-agent process list`" after it (none when all fit).
///    One long early line that doesn't fit leaves out every later one, though they would fit.
/// 2. "Changed since you last read": in the same order, each applying winner whose blob in
///    `input.texts` isn't among `input.read`'s blobs for its key, or that has no text there.
///    The longest run of them, from the first, that fits in full (trailing whitespace trimmed)
///    with all the rest named after it ("Run `org-agent process show <key>`"); one with no
///    text is always named. If not even every name fits, the longest run of names that fits
///    with "and N more changed: run `org-agent process list`" after it.
/// 3. Each On moment, in [`Moment`] order: its guidance text (trailing whitespace trimmed),
///    then each applying winner's prompt for it, in the same order. These are reminders, not
///    rules (processes-ux.md decision 9), so they come last. A moment is whole or left out:
///    the longest run that fits with "and N more moments: process context limit reached
///    (`org-agent process list`)" after it.
///
/// Only whole entries are emitted, never a cut one; an entry or count line that doesn't fit is
/// left out, so the cap holds whatever the input (with a cap too small for the kept room, the
/// kept lines may not fit either). With nothing to show, the text is empty.
pub fn render(set: &EffectiveSet, input: &RenderInput) -> Rendered {
    render_within(set, input, CAP)
}

/// [`render`] with the cap `cap`.
fn render_within(set: &EffectiveSet, input: &RenderInput, cap: usize) -> Rendered {
    let mut winners: Vec<&Winner> = set
        .entries
        .iter()
        .filter_map(|entry| entry.winner.as_ref())
        .filter(|w| w.file.status == Status::Active)
        .collect();
    winners.sort_by(|a, b| {
        (a.file.place.level(), &a.file.key).cmp(&(b.file.place.level(), &b.file.key))
    });
    let mut out = Out {
        cap,
        ..Out::default()
    };
    let changes = changes(&winners, input);
    let blocks = moments(&winners, input);
    // Each later part keeps room to say it has something: the changed heading and count line,
    // and the moments' count line, whatever the parts before it fill (unless the cap can't hold
    // the earlier count lines too: see `Out::push`).
    let keep = |n: usize, len: usize| if n > 0 { len } else { 0 };
    let for_moments = keep(blocks.len(), more_moments(blocks.len()).len());
    let n = changes.1.len();
    let for_changed = keep(n, CHANGED.len() + more_changed(n).len());
    out.section("# Your processes\n\n", |out| {
        let lines = summaries(set, &winners, &input.names);
        out.reserved = for_changed + for_moments;
        out.fill(&lines, |n| {
            format!("and {n} more: run `org-agent process list`\n")
        });
        out.reserved = for_moments;
        out.section(CHANGED, |out| changed(out, &changes));
        out.reserved = 0;
        out.fill(&blocks, more_moments);
    });
    Rendered {
        text: out.text,
        delivered: out.delivered,
    }
}

const CHANGED: &str = "\n## Changed since you last read\n";

fn more_changed(n: usize) -> String {
    format!("\nand {n} more changed: run `org-agent process list`\n")
}

fn more_moments(n: usize) -> String {
    format!("\nand {n} more moments: process context limit reached (`org-agent process list`)\n")
}

/// The text so far, which never passes `cap`, the full texts in it, and the bytes kept for
/// the parts still to come.
#[derive(Default)]
struct Out {
    text: String,
    cap: usize,
    reserved: usize,
    delivered: Vec<(ProcessKey, String)>,
}

impl Out {
    /// The room for choosing what to show: what the cap leaves, less the room kept.
    fn room(&self) -> usize {
        self.cap.saturating_sub(self.text.len() + self.reserved)
    }

    /// Appends `s` if it fits whole within the cap; says whether it did. It may use the room
    /// kept for later parts: a count line or heading that only fits there goes in, so an earlier
    /// part's word wins over a later one's when the cap can't hold both.
    fn push(&mut self, s: &str) -> bool {
        let fits = s.len() <= self.cap - self.text.len();
        if fits {
            self.text.push_str(s);
        }
        fits
    }

    /// `heading`, then what `body` appends; neither if `body` appends nothing.
    fn section(&mut self, heading: &str, body: impl FnOnce(&mut Out)) {
        let mark = self.text.len();
        if !self.push(heading) {
            return;
        }
        let after = self.text.len();
        body(self);
        if self.text.len() == after {
            self.text.truncate(mark);
        }
    }

    /// The longest run of `items` from the first that fits in [`Out::room`] together with `more`
    /// of the number left out (nothing when none is), then that count line if it fits the cap.
    fn fill(&mut self, items: &[String], more: impl Fn(usize) -> String) {
        let n = items.len();
        let size = |k: usize| {
            let tail = if k < n { more(n - k).len() } else { 0 };
            items[..k].iter().map(String::len).sum::<usize>() + tail
        };
        let k = (0..=n).rev().find(|&k| size(k) <= self.room()).unwrap_or(0);
        for item in &items[..k] {
            self.push(item);
        }
        if k < n {
            self.push(&more(n - k));
        }
    }
}

/// Part 1's lines.
fn summaries(set: &EffectiveSet, winners: &[&Winner], names: &Names) -> Vec<String> {
    let applying = winners.iter().map(|w| {
        let f = &w.file;
        let place = label(&f.place, names);
        format!("- {} ({place} v{}): {}\n", f.name, f.version, f.summary)
    });
    let off = set
        .turned_off
        .iter()
        .map(|t| format!("- {}: {}\n", t.key.as_str(), t.message()));
    let conflicts = set
        .conflicts
        .iter()
        .map(|c| format!("- {}\n", conflict(set, c, names)));
    applying.chain(off).chain(conflicts).collect()
}

/// A conflict in the spec's wording (processes-ux.md §2, PROC-3, PROC-6): "<process>: <A> and
/// <B> disagree.", then what applies until the project decides, any other team sealing the key,
/// and each "can't tell" (processes.md §3.2 rule 2). The process is named by the version that
/// applies, else by its key.
fn conflict(set: &EffectiveSet, c: &Conflict, names: &Names) -> String {
    let name = |place: &Place| place_name(names, place);
    let process = set.why(&c.key).and_then(|e| e.winner.as_ref());
    let process = process.map_or(c.key.as_str(), |w| w.file.name.as_str());
    let teams: Vec<String> = c.teams.iter().map(name).collect();
    let mut line = format!("{process}: {} disagree.", and_list(&teams));
    line += &match &c.applies {
        None => " No version applies until the project decides.".to_owned(),
        Some(Place::Organization) => {
            " The organization's version applies until the project decides.".to_owned()
        }
        Some(p) if c.sealed_by.contains(p) => {
            format!(" Sealed by {}; applies until the project decides.", name(p))
        }
        Some(p) => format!(" {}'s version applies until the project decides.", name(p)),
    };
    let others = c
        .sealed_by
        .iter()
        .filter(|p| c.applies.as_ref() != Some(*p));
    for p in others {
        line += &format!(" Sealed by {}.", name(p));
    }
    for path in &c.unknown {
        line += &format!(" Can't tell: {path} can't be read.");
    }
    line
}

/// Where a process comes from, as processes-ux.md §3 labels it ("Review and merge (org v3), Code
/// review (Checkout v2), Release checklist (chk v1)"): "org", a team's or project's name (else
/// its team id or project key), or "private" for an agent's own, which never shows its id.
fn label(place: &Place, names: &Names) -> String {
    match place {
        Place::Organization => "org".into(),
        Place::Agent(_) => "private".into(),
        _ => place_name(names, place),
    }
}

/// "A", "A and B", "A, B and C".
fn and_list(items: &[String]) -> String {
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

/// Part 2's entries, for each changed process: its full text with its key and blob, if the
/// office gave one, and its naming line.
type Changes<'a> = (
    Vec<Option<(String, &'a ProcessKey, &'a String)>>,
    Vec<String>,
);

fn changes<'a>(winners: &[&'a Winner], input: &'a RenderInput) -> Changes<'a> {
    let mut full = Vec::new();
    let mut named = Vec::new();
    for w in winners {
        let key = &w.file.key;
        let text = input.texts.get(key);
        let read = input.read.get(key);
        if text.is_some_and(|t| read.is_some_and(|read| read.contains(&t.blob))) {
            continue;
        }
        let place = label(&w.file.place, &input.names);
        let head = format!("\n### {} ({place} v{})\n\n", w.file.name, w.file.version);
        full.push(text.map(|t| (format!("{head}{}\n", t.text.trim_end()), key, &t.blob)));
        let show = format!(
            "{head}Run `org-agent process show {}` to read it.\n",
            key.as_str()
        );
        named.push(show);
    }
    (full, named)
}

/// Part 2: every changed process in full or named; see [`render`].
fn changed(out: &mut Out, (full, named): &Changes) {
    let n = named.len();
    let entry = |i: usize, k: usize| match &full[i] {
        Some((text, ..)) if i < k => text,
        _ => &named[i],
    };
    let size = |k: usize| (0..n).map(|i| entry(i, k).len()).sum::<usize>();
    let Some(k) = (0..=n).rev().find(|&k| size(k) <= out.room()) else {
        out.fill(named, more_changed);
        return;
    };
    // `k` was chosen so that every entry fits: each full text pushed here is in the text.
    for (i, text) in full.iter().enumerate() {
        out.push(entry(i, k));
        if let (true, Some((_, key, blob))) = (i < k, text) {
            out.delivered.push(((*key).clone(), (*blob).clone()));
        }
    }
}

/// Part 3's blocks, one per On moment.
fn moments(winners: &[&Winner], input: &RenderInput) -> Vec<String> {
    let block = |(moment, guidance): (&Moment, &String)| {
        let mut block = format!("\n## {}\n\n{}\n", moment.title(), guidance.trim_end());
        let mut prompts = String::new();
        for w in winners {
            if let Some(prompt) = moment.prompt(&w.file.moments) {
                prompts += &format!("- {}: {prompt}\n", w.file.name);
            }
        }
        if !prompts.is_empty() {
            block += &format!("\n{prompts}");
        }
        block
    };
    input.moments.iter().map(block).collect()
}

#[cfg(test)]
mod tests;
