//! Strictly read-only, pure-Rust SQLite 3 B-tree and WAL reader for `agentic-org doctor`
//! projection parity verification.
//!
//! Opens `office.db` (and an optional `office.db-wal` sidecar) read-only with `O_NOFOLLOW`, never
//! writes or checkpoints, never creates `-wal`/`-shm` files, and requires zero SQLite C bindings.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::paths::normalize_lstat_path;
use super::sqlite_wal::{load_wal_index, read_be_u16_at, read_be_u32_at};

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const MAX_PAGES_VISITED: usize = 16_384;
const MAX_OVERFLOW_HOPS: usize = 256;
const MAX_ROWS_DECODED: usize = 100_000;
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;

/// One task row extracted read-only from `office.db`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedTask {
    pub(crate) id: Option<String>,
    pub(crate) key: Option<String>,
}

/// Reads all rows of the `tasks` table from `db_path` without mutating the file or sidecars.
pub(crate) fn read_projected_tasks(db_path: &Path) -> Result<Vec<ProjectedTask>, String> {
    let mut reader = SqliteReader::open(db_path)?;
    let schema_rows = reader.read_table(1)?;
    let mut tasks_root: Option<u32> = None;
    let mut tasks_ddl: Option<String> = None;
    for row in schema_rows {
        let kind = row.first().and_then(SqlVal::as_str);
        let name = row.get(1).and_then(SqlVal::as_str);
        if kind == Some("table") && name == Some("tasks") {
            tasks_root = row.get(3).and_then(SqlVal::as_u32);
            tasks_ddl = row.get(4).and_then(SqlVal::as_str).map(str::to_owned);
            break;
        }
    }
    let Some(root_page) = tasks_root else {
        return Err("SQLite database is missing required `tasks` table".to_owned());
    };
    let cols = parse_task_columns(tasks_ddl.as_deref().unwrap_or(""));
    let rows = reader.read_table(root_page)?;
    let mut tasks = Vec::with_capacity(rows.len());
    for row in rows {
        let id = cols
            .id
            .and_then(|i| row.get(i))
            .and_then(SqlVal::as_str)
            .map(str::to_owned);
        let key = cols
            .key
            .and_then(|i| row.get(i))
            .and_then(SqlVal::as_str)
            .map(str::to_owned)
            .or_else(|| {
                let code = cols
                    .office_code
                    .and_then(|i| row.get(i))
                    .and_then(SqlVal::as_str)?;
                let num = cols
                    .number
                    .and_then(|i| row.get(i))
                    .and_then(SqlVal::as_i64)?;
                Some(format!("T-{code}{num}"))
            });
        tasks.push(ProjectedTask { id, key });
    }
    Ok(tasks)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TaskCols {
    pub(super) id: Option<usize>,
    pub(super) key: Option<usize>,
    pub(super) office_code: Option<usize>,
    pub(super) number: Option<usize>,
}

pub(super) fn parse_task_columns(ddl: &str) -> TaskCols {
    let mut cols = TaskCols {
        id: None,
        key: None,
        office_code: None,
        number: None,
    };
    let Some(open) = ddl.find('(') else {
        return cols;
    };
    let Some(close) = ddl.rfind(')') else {
        return cols;
    };
    if close <= open {
        return cols;
    }
    let mut physical_idx = 0usize;
    for raw_def in split_top_level_commas(&ddl[open + 1..close]) {
        let def = strip_leading_sql_comments(raw_def).trim();
        if def.is_empty() || is_table_constraint(def) {
            continue;
        }
        if is_virtual_generated(def) {
            continue;
        }
        let col_name = extract_column_name(def);
        match col_name.to_ascii_lowercase().as_str() {
            "id" => cols.id = Some(physical_idx),
            "key" => cols.key = Some(physical_idx),
            "office_code" => cols.office_code = Some(physical_idx),
            "number" => cols.number = Some(physical_idx),
            _ => {}
        }
        physical_idx += 1;
    }
    cols
}

fn strip_leading_sql_comments(mut input: &str) -> &str {
    loop {
        let trimmed = input.trim_start();
        if let Some(after) = trimmed.strip_prefix("--") {
            match after.find('\n') {
                Some(nl) => input = &after[nl + 1..],
                None => return "",
            }
        } else {
            return trimmed;
        }
    }
}

pub(super) fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if !in_single && !in_double && bytes[i] == b'-' && bytes.get(i + 1) == Some(&b'-') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        match bytes[i] {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'(' if !in_single && !in_double => depth += 1,
            b')' if !in_single && !in_double => depth = (depth - 1).max(0),
            b',' if !in_single && !in_double && depth == 0 => {
                out.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&input[start..]);
    out
}

fn is_table_constraint(def: &str) -> bool {
    let first = def
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(
        first.as_str(),
        "PRIMARY" | "UNIQUE" | "FOREIGN" | "CHECK" | "CONSTRAINT"
    )
}

fn is_virtual_generated(def: &str) -> bool {
    let upper = def.to_ascii_uppercase();
    upper.contains("GENERATED ALWAYS AS") && !upper.contains("STORED")
}

fn extract_column_name(def: &str) -> &str {
    let trimmed = def.trim_start();
    if let Some(rest) = trimmed.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else if let Some(rest) = trimmed.strip_prefix('`') {
        rest.split('`').next().unwrap_or("")
    } else {
        trimmed.split_whitespace().next().unwrap_or("")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SqlVal {
    Null,
    Int(i64),
    Text(String),
    Other,
}

impl SqlVal {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    fn as_u32(&self) -> Option<u32> {
        self.as_i64().and_then(|v| u32::try_from(v).ok())
    }
}

struct SqliteReader {
    db: File,
    wal: Option<File>,
    wal_pages: BTreeMap<u32, u64>,
    page_size: usize,
    usable_size: usize,
    total_payload_bytes: usize,
    max_rows: usize,
    max_payload_bytes: usize,
    max_total_payload_bytes: usize,
}

impl SqliteReader {
    fn open(path: &Path) -> Result<Self, String> {
        let mut db = org_platform::fs::open_nofollow_ro(&normalize_lstat_path(path))
            .map_err(|e| format!("opening SQLite projection `{}`: {e}", path.display()))?;
        let meta = db
            .metadata()
            .map_err(|e| format!("reading metadata for `{}`: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("`{}` is not a regular file", path.display()));
        }
        if meta.len() < 100 {
            return Err(format!("`{}` is truncated (< 100 bytes)", path.display()));
        }
        let mut hdr = [0u8; 100];
        db.read_exact(&mut hdr)
            .map_err(|e| format!("reading SQLite header: {e}"))?;
        if &hdr[..16] != SQLITE_MAGIC {
            return Err("file is not a valid SQLite 3 database".to_owned());
        }
        let raw_ps = read_be_u16_at(&hdr, 16)?;
        let page_size = if raw_ps == 1 {
            65_536
        } else {
            usize::from(raw_ps)
        };
        if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err(format!("invalid SQLite page size {page_size}"));
        }
        let reserved = usize::from(hdr[20]);
        let usable_size = page_size
            .checked_sub(reserved)
            .filter(|&u| u >= 480)
            .ok_or_else(|| "invalid SQLite usable page size".to_owned())?;
        let mut wal_path = path.as_os_str().to_os_string();
        wal_path.push("-wal");
        let (wal, wal_pages) = load_wal_index(Path::new(&wal_path), page_size)?;
        Ok(Self {
            db,
            wal,
            wal_pages,
            page_size,
            usable_size,
            total_payload_bytes: 0,
            max_rows: MAX_ROWS_DECODED,
            max_payload_bytes: MAX_PAYLOAD_BYTES,
            max_total_payload_bytes: MAX_TOTAL_PAYLOAD_BYTES,
        })
    }

    fn read_page(&mut self, page_no: u32) -> Result<Vec<u8>, String> {
        if page_no == 0 {
            return Err("invalid SQLite page number 0".to_owned());
        }
        let mut buf = vec![0u8; self.page_size];
        if let (Some(wal), Some(&offset)) = (&mut self.wal, self.wal_pages.get(&page_no)) {
            wal.seek(SeekFrom::Start(offset))
                .and_then(|_| wal.read_exact(&mut buf))
                .map_err(|e| format!("reading WAL page {page_no}: {e}"))?;
            return Ok(buf);
        }
        let page_size_u64 =
            u64::try_from(self.page_size).map_err(|_| "SQLite page size overflow".to_owned())?;
        let offset = u64::from(page_no - 1)
            .checked_mul(page_size_u64)
            .ok_or_else(|| "SQLite page offset overflow".to_owned())?;
        self.db
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.db.read_exact(&mut buf))
            .map_err(|e| format!("reading SQLite page {page_no}: {e}"))?;
        Ok(buf)
    }

    fn read_table(&mut self, root_page: u32) -> Result<Vec<Vec<SqlVal>>, String> {
        let mut rows = Vec::new();
        let mut visited = BTreeSet::new();
        let mut stack = vec![root_page];
        while let Some(page_no) = stack.pop() {
            if !visited.insert(page_no) || visited.len() > MAX_PAGES_VISITED {
                return Err("SQLite B-tree cycle or page limit exceeded".to_owned());
            }
            let page = self.read_page(page_no)?;
            let hdr_off = if page_no == 1 { 100 } else { 0 };
            let page_type = *page
                .get(hdr_off)
                .ok_or_else(|| "truncated B-tree page".to_owned())?;
            let cell_count = usize::from(read_be_u16_at(&page, hdr_off + 3)?);
            match page_type {
                0x0d => self.read_leaf_cells(&page, hdr_off, cell_count, &mut rows)?,
                0x05 => push_interior_children(&page, hdr_off, cell_count, &mut stack)?,
                other => return Err(format!("unexpected B-tree page type {other:#x}")),
            }
        }
        Ok(rows)
    }

    fn read_leaf_cells(
        &mut self,
        page: &[u8],
        hdr_off: usize,
        cell_count: usize,
        rows: &mut Vec<Vec<SqlVal>>,
    ) -> Result<(), String> {
        let ptr_base = hdr_off
            .checked_add(8)
            .ok_or_else(|| "SQLite leaf header offset overflow".to_owned())?;
        for idx in 0..cell_count {
            if rows.len() >= self.max_rows {
                return Err(format!(
                    "SQLite decoded row limit ({}) exceeded",
                    self.max_rows
                ));
            }
            let off = read_cell_ptr(page, ptr_base, idx)?;
            let cell = page
                .get(off..)
                .ok_or_else(|| "SQLite leaf cell offset out of bounds".to_owned())?;
            let (payload_len_u64, n1) = read_varint(cell)?;
            let rowid_off = off
                .checked_add(n1)
                .ok_or_else(|| "SQLite leaf cell offset overflow".to_owned())?;
            let after_len = page
                .get(rowid_off..)
                .ok_or_else(|| "SQLite leaf cell rowid out of bounds".to_owned())?;
            let (_, n2) = read_varint(after_len)?;
            let payload_start = rowid_off
                .checked_add(n2)
                .filter(|&s| s <= page.len())
                .ok_or_else(|| "SQLite leaf cell payload offset out of bounds".to_owned())?;
            let payload_len = self.reserve_payload_budget(payload_len_u64)?;
            let payload = self.assemble_payload(page, payload_start, payload_len)?;
            rows.push(decode_record(&payload)?);
        }
        Ok(())
    }

    fn reserve_payload_budget(&mut self, payload_len_u64: u64) -> Result<usize, String> {
        let payload_len = usize::try_from(payload_len_u64)
            .map_err(|_| "SQLite cell payload length exceeds platform usize".to_owned())?;
        if payload_len > self.max_payload_bytes {
            return Err(format!(
                "SQLite cell payload length {payload_len} exceeds per-row limit {}",
                self.max_payload_bytes
            ));
        }
        let next_total = self
            .total_payload_bytes
            .checked_add(payload_len)
            .filter(|&n| n <= self.max_total_payload_bytes)
            .ok_or_else(|| {
                format!(
                    "SQLite cumulative payload budget ({} bytes) exceeded",
                    self.max_total_payload_bytes
                )
            })?;
        self.total_payload_bytes = next_total;
        Ok(payload_len)
    }

    fn assemble_payload(
        &mut self,
        page: &[u8],
        start: usize,
        total_len: usize,
    ) -> Result<Vec<u8>, String> {
        let max_local = self.usable_size - 35;
        if total_len <= max_local {
            let end = start
                .checked_add(total_len)
                .filter(|&e| e <= page.len())
                .ok_or_else(|| "SQLite cell payload out of bounds".to_owned())?;
            let slice = page
                .get(start..end)
                .ok_or_else(|| "SQLite cell payload out of bounds".to_owned())?;
            return Ok(slice.to_vec());
        }
        let min_local = ((self.usable_size - 12) * 32 / 255) - 23;
        let k = min_local + ((total_len - min_local) % (self.usable_size - 4));
        let local = if k <= max_local { k } else { min_local };
        let ovf_ptr_off = start
            .checked_add(local)
            .ok_or_else(|| "SQLite overflow pointer offset overflow".to_owned())?;
        let local_slice = page
            .get(start..ovf_ptr_off)
            .ok_or_else(|| "SQLite local payload out of bounds".to_owned())?;
        let mut next = read_be_u32_at(page, ovf_ptr_off)?;
        let mut out = Vec::with_capacity(total_len);
        out.extend_from_slice(local_slice);
        let mut hops = 0usize;
        while out.len() < total_len && next != 0 {
            hops += 1;
            if hops > MAX_OVERFLOW_HOPS {
                return Err("SQLite overflow chain limit exceeded".to_owned());
            }
            let ovf = self.read_page(next)?;
            next = read_be_u32_at(&ovf, 0)?;
            let take = (total_len - out.len()).min(self.usable_size - 4);
            let chunk_end = 4usize
                .checked_add(take)
                .ok_or_else(|| "SQLite overflow chunk offset overflow".to_owned())?;
            let chunk = ovf
                .get(4..chunk_end)
                .ok_or_else(|| "SQLite overflow page truncated".to_owned())?;
            out.extend_from_slice(chunk);
        }
        if out.len() != total_len {
            return Err("SQLite overflow payload truncated before expected length".to_owned());
        }
        Ok(out)
    }
}

fn push_interior_children(
    page: &[u8],
    hdr_off: usize,
    cell_count: usize,
    stack: &mut Vec<u32>,
) -> Result<(), String> {
    let right_off = hdr_off
        .checked_add(8)
        .ok_or_else(|| "SQLite interior header offset overflow".to_owned())?;
    let right_child = read_be_u32_at(page, right_off)?;
    stack.push(right_child);
    let ptr_base = hdr_off
        .checked_add(12)
        .ok_or_else(|| "SQLite interior pointer base overflow".to_owned())?;
    for idx in (0..cell_count).rev() {
        let off = read_cell_ptr(page, ptr_base, idx)?;
        let left = read_be_u32_at(page, off)?;
        stack.push(left);
    }
    Ok(())
}

fn read_cell_ptr(page: &[u8], ptr_base: usize, idx: usize) -> Result<usize, String> {
    let delta = idx
        .checked_mul(2)
        .ok_or_else(|| "SQLite cell pointer index overflow".to_owned())?;
    let at = ptr_base
        .checked_add(delta)
        .ok_or_else(|| "SQLite cell pointer offset overflow".to_owned())?;
    Ok(usize::from(read_be_u16_at(page, at)?))
}

fn read_varint(bytes: &[u8]) -> Result<(u64, usize), String> {
    let mut val = 0u64;
    for (idx, &b) in bytes.iter().take(9).enumerate() {
        if idx == 8 {
            return Ok(((val << 8) | u64::from(b), 9));
        }
        val = (val << 7) | u64::from(b & 0x7f);
        if b & 0x80 == 0 {
            return Ok((val, idx + 1));
        }
    }
    Err("truncated SQLite varint".to_owned())
}

pub(super) fn decode_record(payload: &[u8]) -> Result<Vec<SqlVal>, String> {
    let (hdr_len, mut pos) = read_varint(payload)?;
    let hdr_end = usize::try_from(hdr_len)
        .ok()
        .filter(|&e| e >= pos && e <= payload.len())
        .ok_or_else(|| "SQLite record header out of bounds".to_owned())?;
    let mut serials = Vec::new();
    while pos < hdr_end {
        let slice = payload
            .get(pos..hdr_end)
            .ok_or_else(|| "SQLite record header out of bounds".to_owned())?;
        let (st, n) = read_varint(slice)?;
        serials.push(st);
        pos = pos
            .checked_add(n)
            .ok_or_else(|| "SQLite record header offset overflow".to_owned())?;
    }
    let mut data_pos = hdr_end;
    let mut cols = Vec::with_capacity(serials.len());
    for st in serials {
        let rest = payload
            .get(data_pos..)
            .ok_or_else(|| "SQLite record column out of bounds".to_owned())?;
        let (val, used) = decode_column(st, rest)?;
        data_pos = data_pos
            .checked_add(used)
            .ok_or_else(|| "SQLite record data offset overflow".to_owned())?;
        cols.push(val);
    }
    Ok(cols)
}

fn decode_column(serial: u64, rest: &[u8]) -> Result<(SqlVal, usize), String> {
    match serial {
        0 => Ok((SqlVal::Null, 0)),
        1..=6 => decode_signed_int(serial, rest),
        7 => {
            if rest.len() < 8 {
                return Err("SQLite float column truncated".to_owned());
            }
            Ok((SqlVal::Other, 8))
        }
        8 => Ok((SqlVal::Int(0), 0)),
        9 => Ok((SqlVal::Int(1), 0)),
        n if n >= 12 => {
            let len = usize::try_from((n - 12) / 2)
                .map_err(|_| "SQLite column length exceeds platform usize".to_owned())?;
            let slice = rest
                .get(..len)
                .ok_or_else(|| "SQLite column payload truncated".to_owned())?;
            if n % 2 == 1 {
                let text = String::from_utf8_lossy(slice).into_owned();
                Ok((SqlVal::Text(text), len))
            } else {
                Ok((SqlVal::Other, len))
            }
        }
        _ => Ok((SqlVal::Other, 0)),
    }
}

fn decode_signed_int(serial: u64, rest: &[u8]) -> Result<(SqlVal, usize), String> {
    let len = match serial {
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 6,
        6 => 8,
        _ => return Err(format!("invalid SQLite integer serial type {serial}")),
    };
    let slice = rest
        .get(..len)
        .ok_or_else(|| "SQLite integer truncated".to_owned())?;
    let first = slice
        .first()
        .copied()
        .ok_or_else(|| "SQLite integer truncated".to_owned())?;
    let mut v = if first & 0x80 != 0 { -1i64 } else { 0i64 };
    for &b in slice {
        v = (v << 8) | i64::from(b);
    }
    Ok((SqlVal::Int(v), len))
}
