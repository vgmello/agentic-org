//! Creating and opening a journal incarnation: anchor, recovery base and retained queue.

use super::*;

impl Journal {
    /// Starts a new journal incarnation: writes its immutable anchor. Refuses when one
    /// exists.
    pub fn create(
        durable: &Path,
        identity: &OfficeIdentity,
        facts: &AnchorFacts,
    ) -> Result<Journal, JournalError> {
        Self::create_on(Arc::new(RealDisk), durable, identity, facts)
    }

    pub(crate) fn create_on(
        disk: Arc<dyn Disk>,
        durable: &Path,
        identity: &OfficeIdentity,
        facts: &AnchorFacts,
    ) -> Result<Journal, JournalError> {
        if !durable.is_absolute() {
            return Err(JournalError::Internal(
                "the durable root must be absolute".into(),
            ));
        }
        if !facts.high_waters.valid() || facts.feed.0 > MAX_POSITION {
            return Err(JournalError::Exhausted("origin position"));
        }
        let dir = journal_dir(durable, identity);
        if dir.join("anchor.json").exists() {
            return Err(JournalError::Exists);
        }
        let segments = dir.join("segments");
        let mut current = durable.to_path_buf();
        let below = segments.strip_prefix(durable).expect("built from the root");
        for component in std::iter::once(None).chain(below.components().map(Some)) {
            if let Some(component) = component {
                current.push(component);
            }
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    let not_dir = io::Error::new(io::ErrorKind::InvalidInput, "not a directory");
                    return Err(io_error(&current, "open", &not_dir));
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => disk
                    .create_dir(&current)
                    .map_err(|e| io_error(&current, "create directory", &e))?,
                Err(e) => return Err(io_error(&current, "open", &e)),
            }
            if let Some(parent) = current.parent() {
                disk.sync_dir(parent).map_err(|e| flush_error(parent, &e))?;
            }
        }
        let anchor = Anchor {
            format: FORMAT.to_owned(),
            identity: identity.clone(),
            high_waters: facts.high_waters,
            feed: facts.feed,
            predecessor: facts.predecessor.clone(),
        };
        let bytes =
            serde_json::to_vec(&anchor).map_err(|e| JournalError::Internal(e.to_string()))?;
        write_small(disk.as_ref(), &dir, "anchor.json", &bytes, true)?;
        let base = JournalPoint {
            ordinal: 0,
            sha256: Sha256::of(&bytes),
            high_waters: facts.high_waters,
            feed: facts.feed,
        };
        Ok(Journal {
            dir,
            disk,
            identity: identity.clone(),
            segment_bytes: AtomicU64::new(SEGMENT_BYTES),
            release_lock: Mutex::new(()),
            inner: Mutex::new(Inner {
                segments: VecDeque::new(),
                active: None,
                unflushed: Vec::new(),
                directory_unflushed: false,
                head: base.clone(),
                base,
                flushed: 0,
                materialized: 0,
                retained: VecDeque::new(),
                broken: None,
            }),
        })
    }

    /// Opens an existing journal: asks the custody for the confirmed head, verifies the
    /// chain from the anchor or the checkpoint, applies the torn-tail rule, and flushes
    /// what it kept. Everything it returns is on the drive.
    pub fn open(
        durable: &Path,
        identity: &OfficeIdentity,
        custody: &dyn WriterAuthority,
    ) -> Result<(Journal, Recovered), JournalError> {
        Self::open_on(Arc::new(RealDisk), durable, identity, custody, None)
    }

    pub(crate) fn open_on(
        disk: Arc<dyn Disk>,
        durable: &Path,
        identity: &OfficeIdentity,
        custody: &dyn WriterAuthority,
        min_consumer_watermark: Option<u64>,
    ) -> Result<(Journal, Recovered), JournalError> {
        if !durable.is_absolute() {
            return Err(JournalError::Internal(
                "the durable root must be absolute".into(),
            ));
        }
        let confirmed = custody
            .checkpoint(identity)
            .map_err(|e| JournalError::Custody(e.0))?;
        let dir = journal_dir(durable, identity);
        let (base, materialized) = read_recovery_base(&dir, identity)?;

        // Segments, by first ordinal. One wholly at or below the checkpoint was released
        // and only its unlink was lost.
        let segments_dir = dir.join("segments");
        let mut firsts = Vec::new();
        let listing =
            fs::read_dir(&segments_dir).map_err(|e| io_error(&segments_dir, "list", &e))?;
        for entry in listing {
            let entry = entry.map_err(|e| io_error(&segments_dir, "list", &e))?;
            let name = entry.file_name();
            let first = name
                .to_str()
                .and_then(|n| n.strip_suffix(".log"))
                .filter(|n| n.len() == 20)
                .and_then(|n| n.parse::<u64>().ok());
            if let Some(first) = first {
                firsts.push(first);
            }
        }
        firsts.sort_unstable();
        // Nothing on the drive is changed until the confirmed head has been compared:
        // an opening that refuses leaves every byte where it found it.
        let mut repairs: Vec<Repair> = Vec::new();
        while firsts.len() > 1 && firsts[1] <= base.ordinal + 1 {
            let path = segments_dir.join(segment_name(firsts.remove(0)));
            repairs.push(Repair::Unlink(path));
        }
        // The last of the released segments has no successor's name to prove it: a cut
        // that lost its unlink can also have lost the entry of the segment after it. It
        // is told by what it holds. One that ends at the checkpoint with another entry
        // than the checkpoint's is another history: refused, every byte left.
        if let Some(first) = firsts.first().copied().filter(|f| *f <= base.ordinal) {
            let path = segments_dir.join(segment_name(first));
            let bytes = fs::read(&path).map_err(|e| io_error(&path, "read", &e))?;
            if ends_at_or_below(&bytes, first, &base)? {
                firsts.remove(0);
                repairs.push(Repair::Unlink(path));
            }
        }

        let mut point = base.clone();
        let mut entries: Vec<Arc<Entry>> = Vec::new();
        let mut segments = VecDeque::new();
        let mut truncated = None;
        let mut active = None;
        let mut kept: Vec<(PathBuf, Arc<dyn DiskFile>)> = Vec::new();
        for first in firsts.iter().copied() {
            let path = segments_dir.join(segment_name(first));
            if truncated.is_some() {
                // Everything after a torn entry was never acknowledged durable.
                repairs.push(Repair::Unlink(path));
                continue;
            }
            if first != point.ordinal + 1 {
                // A gap before this segment. A power cut leaves one when it takes the
                // unflushed end of the segment before, or all of it: a segment is rolled
                // without a flush. Every entry in such a gap is above the confirmed head,
                // so this segment and the later ones are a torn tail. A gap that reaches
                // down to the confirmed head, or a segment that starts at an ordinal the
                // chain already holds, is never a cut.
                let above_confirmed = confirmed
                    .as_ref()
                    .is_none_or(|confirmed| point.ordinal >= confirmed.ordinal);
                if first <= point.ordinal || !above_confirmed {
                    return Err(JournalError::MissingPredecessor { ordinal: first });
                }
                truncated = Some(point.ordinal + 1);
                repairs.push(Repair::Unlink(path));
                continue;
            }
            let bytes = fs::read(&path).map_err(|e| io_error(&path, "read", &e))?;
            let good =
                scan(&bytes, &mut point, identity, &mut entries).or_else(|bad| match bad {
                    Bad::Chain { ordinal, reason } => {
                        Err(JournalError::Corrupt { ordinal, reason })
                    }
                    Bad::Torn {
                        offset,
                        ordinal,
                        reason,
                    } => {
                        if confirmed.as_ref().is_some_and(|c| ordinal <= c.ordinal) {
                            return Err(JournalError::Corrupt { ordinal, reason });
                        }
                        truncated = Some(ordinal);
                        Ok(offset)
                    }
                })?;
            // A rolled segment's tail may never have been flushed by the process that
            // wrote it: every kept segment is opened here and flushed below.
            let file = disk
                .open_append(&path)
                .map_err(|e| io_error(&path, "open", &e))?;
            if good < bytes.len() {
                repairs.push(Repair::Truncate {
                    path: path.clone(),
                    file: Arc::clone(&file),
                    length: good as u64,
                });
            }
            kept.push((path.clone(), Arc::clone(&file)));
            active = Some(file);
            segments.push_back(Segment {
                first,
                path,
                length: good as u64,
                end: point.clone(),
            });
        }

        if let Some(confirmed) = &confirmed {
            if confirmed.ordinal > point.ordinal {
                return Err(JournalError::Behind {
                    journal: point.ordinal,
                    custody: confirmed.ordinal,
                });
            }
            // The confirmed entry must be here to be compared: one the journal no longer
            // holds (released before a head the host has since lost) is never a pass.
            let held = if confirmed.ordinal == base.ordinal {
                base.sha256
            } else {
                let found = entries.iter().find(|e| e.ordinal() == confirmed.ordinal);
                found
                    .map(|e| e.sha256)
                    .ok_or(JournalError::MissingPredecessor {
                        ordinal: confirmed.ordinal,
                    })?
            };
            if held != confirmed.sha256 {
                return Err(JournalError::Fork {
                    ordinal: confirmed.ordinal,
                });
            }
        }

        // The journal is this writer's and holds the confirmed head: only now is the
        // torn tail cut away and what a release left behind removed.
        for repair in repairs {
            match repair {
                Repair::Unlink(path) => disk
                    .unlink(&path)
                    .map_err(|e| io_error(&path, "remove", &e))?,
                Repair::Truncate { path, file, length } => file
                    .truncate(length)
                    .map_err(|e| io_error(&path, "truncate", &e))?,
            }
        }
        // Make what was kept durable before anything is built on it.
        disk.sync_dir(&segments_dir)
            .map_err(|e| flush_error(&segments_dir, &e))?;
        for (path, file) in &kept {
            file.flush_full().map_err(|e| flush_error(path, &e))?;
        }
        let materialized = materialized.min(point.ordinal);
        let retained = build_retained_queue(
            &entries,
            base.ordinal,
            materialized,
            point.ordinal,
            min_consumer_watermark,
        )?;
        let journal = Journal {
            dir,
            disk,
            identity: identity.clone(),
            segment_bytes: AtomicU64::new(SEGMENT_BYTES),
            release_lock: Mutex::new(()),
            inner: Mutex::new(Inner {
                segments,
                active,
                unflushed: Vec::new(),
                directory_unflushed: false,
                base,
                flushed: point.ordinal,
                head: point,
                materialized,
                retained,
                broken: None,
            }),
        };
        let recovered = Recovered {
            confirmed,
            entries,
            truncated,
        };
        Ok((journal, recovered))
    }
}

pub(super) fn build_retained_queue(
    entries: &[Arc<Entry>],
    base_ordinal: u64,
    materialized: u64,
    head_ordinal: u64,
    min_consumer_watermark: Option<u64>,
) -> Result<VecDeque<Arc<Entry>>, JournalError> {
    if let Some(wm) = min_consumer_watermark {
        if wm < base_ordinal {
            return Err(JournalError::MissingPredecessor { ordinal: wm });
        }
    }
    let retain_floor = min_consumer_watermark.map_or(materialized, |wm| materialized.min(wm));
    let retained: VecDeque<Arc<Entry>> = entries
        .iter()
        .filter(|e| e.ordinal() > retain_floor)
        .cloned()
        .collect();
    if retain_floor < head_ordinal {
        let mut expected = retain_floor + 1;
        for entry in &retained {
            if entry.ordinal() != expected {
                return Err(JournalError::MissingPredecessor { ordinal: expected });
            }
            expected += 1;
        }
        if expected != head_ordinal + 1 {
            return Err(JournalError::MissingPredecessor { ordinal: expected });
        }
    }
    Ok(retained)
}

pub(super) fn read_recovery_base(
    dir: &Path,
    identity: &OfficeIdentity,
) -> Result<(JournalPoint, u64), JournalError> {
    let anchor_bytes = read_small(&dir.join("anchor.json"))?
        .ok_or(JournalError::MissingPredecessor { ordinal: 0 })?;
    let anchor: Anchor = serde_json::from_slice(&anchor_bytes)
        .map_err(|e| JournalError::UnknownFormat(format!("anchor: {e}")))?;
    if anchor.format != FORMAT {
        return Err(JournalError::UnknownFormat(anchor.format));
    }
    if anchor.identity != *identity {
        return Err(JournalError::IdentityMismatch);
    }
    let mut base = JournalPoint {
        ordinal: 0,
        sha256: Sha256::of(&anchor_bytes),
        high_waters: anchor.high_waters,
        feed: anchor.feed,
    };
    if let Some(bytes) = read_small(&dir.join("checkpoint.json"))? {
        let checkpoint: Checkpoint = serde_json::from_slice(&bytes)
            .map_err(|e| JournalError::UnknownFormat(format!("checkpoint: {e}")))?;
        if checkpoint.format != FORMAT {
            return Err(JournalError::UnknownFormat(checkpoint.format));
        }
        base = JournalPoint {
            ordinal: checkpoint.ordinal,
            sha256: checkpoint.sha256,
            high_waters: checkpoint.high_waters,
            feed: checkpoint.feed,
        };
    }
    if base.ordinal > MAX_POSITION || base.feed.0 > MAX_POSITION || !base.high_waters.valid() {
        return Err(JournalError::Corrupt {
            ordinal: base.ordinal,
            reason: "the anchor or checkpoint holds a position past the limit".into(),
        });
    }
    let mut materialized = base.ordinal;
    if let Some(bytes) = read_small(&dir.join("materialized.json"))? {
        let mark: Materialized = serde_json::from_slice(&bytes)
            .map_err(|e| JournalError::UnknownFormat(format!("materialized mark: {e}")))?;
        materialized = materialized.max(mark.ordinal);
    }
    Ok((base, materialized))
}
