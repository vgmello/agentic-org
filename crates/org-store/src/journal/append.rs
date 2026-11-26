//! Appending, flushing, materializing and releasing journal entries.

use super::*;

impl Journal {
    /// Appends a batch in one write. On an error or a short write nothing of the batch is
    /// kept: the segment is truncated back to its last good entry, and that is flushed,
    /// before this returns. If the truncation fails the journal accepts nothing more.
    pub fn append(&self, entries: &[Arc<Entry>]) -> Result<(), JournalError> {
        let mut inner = self.inner();
        if let Some(broken) = &inner.broken {
            return Err(broken.clone());
        }
        let Some(first) = entries.first() else {
            return Ok(());
        };
        let mut point = inner.head.clone();
        let mut bytes = Vec::with_capacity(entries.iter().map(|e| e.frame.len()).sum());
        for entry in entries {
            if entry.ordinal() > MAX_POSITION {
                return Err(JournalError::Exhausted("journal ordinal"));
            }
            point = point
                .after(entry, &self.identity)
                .map_err(JournalError::Internal)?;
            bytes.extend_from_slice(&entry.frame);
        }
        let limit = self.segment_bytes.load(Ordering::Relaxed);
        let roll = match inner.segments.back() {
            Some(segment) => segment.length > 0 && segment.length + bytes.len() as u64 > limit,
            None => true,
        };
        if roll || inner.active.is_none() {
            let path = match inner.segments.back() {
                Some(segment) if !roll => segment.path.clone(),
                _ => self
                    .dir
                    .join("segments")
                    .join(segment_name(first.ordinal())),
            };
            let file = self
                .disk
                .open_append(&path)
                .map_err(|e| JournalError::Append(format!("open {}: {e}", path.display())))?;
            if roll {
                if let (Some(old), Some(segment)) = (inner.active.take(), inner.segments.back()) {
                    let rolled = (segment.path.clone(), old);
                    inner.unflushed.push(rolled);
                }
                let end = inner.head.clone();
                inner.segments.push_back(Segment {
                    first: first.ordinal(),
                    path,
                    length: 0,
                    end,
                });
                inner.directory_unflushed = true;
            }
            inner.active = Some(file);
        }
        let file = Arc::clone(inner.active.as_ref().expect("opened above"));
        let segment = inner.segments.back_mut().expect("pushed above");
        let failure = match file.write(&bytes) {
            Ok(written) if written == bytes.len() => None,
            Ok(written) => Some(format!("{written} of {} bytes written", bytes.len())),
            Err(e) => Some(e.to_string()),
        };
        if let Some(failure) = failure {
            let repaired = file
                .truncate(segment.length)
                .and_then(|()| file.flush_full());
            if let Err(e) = repaired {
                let broken = JournalError::Broken {
                    path: segment.path.display().to_string(),
                    message: e.to_string(),
                };
                inner.broken = Some(broken.clone());
                return Err(broken);
            }
            return Err(JournalError::Append(failure));
        }
        segment.length += bytes.len() as u64;
        segment.end = point.clone();
        inner.head = point;
        inner.retained.extend(entries.iter().cloned());
        Ok(())
    }

    /// Fully flushes the journal through the head it had when the call began, and returns
    /// that head. Appends continue meanwhile. An error is final for this process.
    ///
    /// Only the store's flusher calls this, and the two calls after it: there is one
    /// flusher by visibility, not by convention.
    pub(crate) fn flush(&self) -> Result<JournalPoint, JournalError> {
        let (target, rolled, active, directory) = {
            let mut inner = self.inner();
            if let Some(broken) = &inner.broken {
                return Err(broken.clone());
            }
            let active = match (&inner.active, inner.segments.back()) {
                (Some(file), Some(segment)) => Some((segment.path.clone(), Arc::clone(file))),
                _ => None,
            };
            let directory = std::mem::take(&mut inner.directory_unflushed);
            let rolled = std::mem::take(&mut inner.unflushed);
            (inner.head.clone(), rolled, active, directory)
        };
        let flushed: Result<(), JournalError> = (|| {
            if directory {
                let segments = self.dir.join("segments");
                self.disk
                    .sync_dir(&segments)
                    .map_err(|e| flush_error(&segments, &e))?;
            }
            for (path, file) in rolled.iter().chain(active.iter()) {
                file.flush_full().map_err(|e| flush_error(path, &e))?;
            }
            Ok(())
        })();
        let mut inner = self.inner();
        if let Err(error) = flushed {
            // After a failed flush the kernel may have dropped the pages: a later flush
            // could succeed without the data being on the drive. Nothing more is flushed
            // or appended in this process.
            inner.broken.get_or_insert(error.clone());
            return Err(error);
        }
        inner.flushed = inner.flushed.max(target.ordinal);
        Ok(target)
    }

    /// Reconstructs in-memory `retained` entries from on-disk segments starting at `base`
    /// when a consumer registers after earlier entries were evicted from RAM (`TASK-52` M-15).
    pub(crate) fn reconstruct_retained_from_segments(&self) -> Result<(), JournalError> {
        let _release_guard = self.release_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (base, seg_paths, already_starts_at_base) = {
            let inner = self.inner();
            let base = inner.base.clone();
            let head_ord = inner.head.ordinal;
            let starts = head_ord == base.ordinal
                || inner
                    .retained
                    .front()
                    .is_some_and(|e| e.ordinal() == base.ordinal + 1);
            let paths: Vec<PathBuf> = inner.segments.iter().map(|s| s.path.clone()).collect();
            (base, paths, starts)
        };
        if already_starts_at_base || seg_paths.is_empty() {
            return Ok(());
        }
        let mut point = base;
        let mut scanned = Vec::new();
        for path in seg_paths {
            let bytes = fs::read(&path).map_err(|e| io_error(&path, "read", &e))?;
            let _ = scan(&bytes, &mut point, &self.identity, &mut scanned).map_err(
                |bad| match bad {
                    Bad::Chain { ordinal, reason }
                    | Bad::Torn {
                        ordinal, reason, ..
                    } => JournalError::Corrupt { ordinal, reason },
                },
            )?;
        }
        let mut inner = self.inner();
        let mut merged: VecDeque<Arc<Entry>> = scanned.into_iter().collect();
        let last_scanned = merged.back().map_or(inner.base.ordinal, |e| e.ordinal());
        for existing in &inner.retained {
            if existing.ordinal() > last_scanned {
                merged.push_back(Arc::clone(existing));
            }
        }
        inner.retained = merged;
        Ok(())
    }

    /// Records that every file through `ordinal` is written and flushed, and lets go of
    /// retained entries up to `ordinal`.
    pub(crate) fn set_materialized(&self, ordinal: u64) -> Result<(), JournalError> {
        self.set_materialized_with_release_cap(ordinal, ordinal)
    }

    /// Records that every file through `ordinal` is written and flushed, and lets go of
    /// retained entries up to `release_cap` (which equals `ordinal` when no external journal
    /// consumer is registered, or `ordinal.min(min_consumer_watermark)` under `ARCH-03`).
    pub(crate) fn set_materialized_with_release_cap(
        &self,
        ordinal: u64,
        release_cap: u64,
    ) -> Result<(), JournalError> {
        {
            let inner = self.inner();
            if ordinal > inner.flushed {
                return Err(JournalError::Internal(
                    "an entry is materialized only after its flush".into(),
                ));
            }
            if ordinal <= inner.materialized {
                return Ok(());
            }
        }
        let bytes = serde_json::to_vec(&Materialized { ordinal })
            .map_err(|e| JournalError::Internal(e.to_string()))?;
        write_small(
            self.disk.as_ref(),
            &self.dir,
            "materialized.json",
            &bytes,
            false,
        )?;
        let mut inner = self.inner();
        inner.materialized = inner.materialized.max(ordinal);
        let evict_through = ordinal.min(release_cap);
        while inner
            .retained
            .front()
            .is_some_and(|e| e.ordinal() <= evict_through)
        {
            inner.retained.pop_front();
        }
        Ok(())
    }

    /// Evicts in-memory retained entries through `ordinal.min(materialized)` once all
    /// registered journal consumers have advanced past them (`ARCH-03`).
    pub(crate) fn evict_retained_through(&self, ordinal: u64) {
        let mut inner = self.inner();
        let limit = ordinal.min(inner.materialized);
        while inner.retained.front().is_some_and(|e| e.ordinal() <= limit) {
            inner.retained.pop_front();
        }
    }

    /// Drops every rolled segment whose entries are all at or below `ordinal` and the
    /// materialized mark. `checkpoint.json` is written and flushed first, so the chain
    /// always has a verifiable start; and the segment directory is flushed before that,
    /// so the segment the checkpoint leads to is on the drive by name before the
    /// checkpoint names it. Returns how many segments were dropped.
    pub(crate) fn release_through(&self, ordinal: u64) -> Result<usize, JournalError> {
        let _release_guard = self.release_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (drop, base) = {
            let inner = self.inner();
            let limit = ordinal.min(inner.materialized);
            let rolled = inner.segments.len().saturating_sub(1);
            let passed = inner.segments.iter().take(rolled);
            let drop: Vec<_> = passed
                .take_while(|s| s.end.ordinal <= limit)
                .map(|s| (s.path.clone(), s.end.clone()))
                .collect();
            let Some((_, base)) = drop.last().cloned() else {
                return Ok(0);
            };
            (drop, base)
        };
        let checkpoint = Checkpoint {
            format: FORMAT.to_owned(),
            ordinal: base.ordinal,
            sha256: base.sha256,
            high_waters: base.high_waters,
            feed: base.feed,
        };
        let bytes =
            serde_json::to_vec(&checkpoint).map_err(|e| JournalError::Internal(e.to_string()))?;
        // The segment kept after the released ones may have been rolled since the last
        // flush: its directory entry must not be losable once the checkpoint is.
        let segments = self.dir.join("segments");
        self.disk
            .sync_dir(&segments)
            .map_err(|e| flush_error(&segments, &e))?;
        write_small(
            self.disk.as_ref(),
            &self.dir,
            "checkpoint.json",
            &bytes,
            false,
        )?;
        {
            let mut inner = self.inner();
            for (path, _) in &drop {
                if inner.segments.front().is_some_and(|s| &s.path == path) {
                    inner.segments.pop_front();
                }
            }
            if base.ordinal > inner.base.ordinal {
                inner.base = base;
            }
        }
        for (path, _) in &drop {
            self.disk
                .unlink(path)
                .map_err(|e| io_error(path, "remove", &e))?;
        }
        self.disk
            .sync_dir(&segments)
            .map_err(|e| flush_error(&segments, &e))?;
        Ok(drop.len())
    }
}
