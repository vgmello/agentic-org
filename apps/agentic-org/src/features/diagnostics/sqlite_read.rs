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

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const WAL_MAGIC_LE: u32 = 0x377f_0682;
const WAL_MAGIC_BE: u32 = 0x377f_0683;
const MAX_PAGES_VISITED: usize = 16_384;
const MAX_OVERFLOW_HOPS: usize = 256;
const MAX_WAL_FRAMES: usize = 65_536;

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
        return Ok(Vec::new());
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

struct TaskCols {
    id: Option<usize>,
    key: Option<usize>,
    office_code: Option<usize>,
    number: Option<usize>,
}

fn parse_task_columns(ddl: &str) -> TaskCols {
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
        let def = raw_def.trim();
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

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_single = false;
    let bytes = input.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' => in_single = !in_single,
            b'(' if !in_single => depth += 1,
            b')' if !in_single => depth = (depth - 1).max(0),
            b',' if !in_single && depth == 0 => {
                out.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
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

#[derive(Debug)]
enum SqlVal {
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
        let raw_ps = u16::from_be_bytes([hdr[16], hdr[17]]);
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
        let (wal, wal_pages) = load_wal_index(Path::new(&wal_path), page_size);
        Ok(Self {
            db,
            wal,
            wal_pages,
            page_size,
            usable_size,
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
        let offset = u64::from(page_no - 1) * (self.page_size as u64);
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
            let cell_count =
                usize::from(u16::from_be_bytes([page[hdr_off + 3], page[hdr_off + 4]]));
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
        let ptr_base = hdr_off + 8;
        for idx in 0..cell_count {
            let off = read_cell_ptr(page, ptr_base, idx)?;
            let (payload_len, n1) = read_varint(&page[off..])?;
            let (_, n2) = read_varint(&page[off + n1..])?;
            let payload_start = off + n1 + n2;
            let payload = self.assemble_payload(page, payload_start, payload_len as usize)?;
            rows.push(decode_record(&payload)?);
        }
        Ok(())
    }

    fn assemble_payload(
        &mut self,
        page: &[u8],
        start: usize,
        total_len: usize,
    ) -> Result<Vec<u8>, String> {
        let max_local = self.usable_size - 35;
        if total_len <= max_local {
            let end = start.checked_add(total_len).filter(|&e| e <= page.len());
            return end
                .map(|e| page[start..e].to_vec())
                .ok_or_else(|| "SQLite cell payload out of bounds".to_owned());
        }
        let min_local = ((self.usable_size - 12) * 32 / 255) - 23;
        let k = min_local + ((total_len - min_local) % (self.usable_size - 4));
        let local = if k <= max_local { k } else { min_local };
        let ovf_ptr_off = start + local;
        if ovf_ptr_off + 4 > page.len() {
            return Err("SQLite overflow pointer out of bounds".to_owned());
        }
        let mut out = Vec::with_capacity(total_len.min(1024 * 1024));
        out.extend_from_slice(&page[start..ovf_ptr_off]);
        let mut next = u32::from_be_bytes(
            page[ovf_ptr_off..ovf_ptr_off + 4]
                .try_into()
                .expect("4 bytes"),
        );
        let mut hops = 0usize;
        while out.len() < total_len && next != 0 {
            hops += 1;
            if hops > MAX_OVERFLOW_HOPS {
                return Err("SQLite overflow chain limit exceeded".to_owned());
            }
            let ovf = self.read_page(next)?;
            next = u32::from_be_bytes(ovf[0..4].try_into().expect("4 bytes"));
            let take = (total_len - out.len()).min(self.usable_size - 4);
            out.extend_from_slice(&ovf[4..4 + take]);
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
    let right_child = u32::from_be_bytes(
        page[hdr_off + 8..hdr_off + 12]
            .try_into()
            .map_err(|_| "truncated interior page")?,
    );
    stack.push(right_child);
    let ptr_base = hdr_off + 12;
    for idx in (0..cell_count).rev() {
        let off = read_cell_ptr(page, ptr_base, idx)?;
        let left = u32::from_be_bytes(
            page.get(off..off + 4)
                .ok_or_else(|| "interior cell out of bounds".to_owned())?
                .try_into()
                .expect("4 bytes"),
        );
        stack.push(left);
    }
    Ok(())
}

fn read_cell_ptr(page: &[u8], ptr_base: usize, idx: usize) -> Result<usize, String> {
    let at = ptr_base + idx * 2;
    let slice = page
        .get(at..at + 2)
        .ok_or_else(|| "cell pointer array out of bounds".to_owned())?;
    Ok(usize::from(u16::from_be_bytes([slice[0], slice[1]])))
}

fn load_wal_index(
    wal_path: &Path,
    expected_page_size: usize,
) -> (Option<File>, BTreeMap<u32, u64>) {
    let Ok(mut wal) = org_platform::fs::open_nofollow_ro(&normalize_lstat_path(wal_path)) else {
        return (None, BTreeMap::new());
    };
    let Ok(meta) = wal.metadata() else {
        return (None, BTreeMap::new());
    };
    if !meta.is_file() || meta.len() < 32 {
        return (None, BTreeMap::new());
    }
    let mut hdr = [0u8; 32];
    if wal.read_exact(&mut hdr).is_err() {
        return (None, BTreeMap::new());
    }
    let magic = u32::from_be_bytes(hdr[0..4].try_into().expect("4 bytes"));
    let big_endian = match magic {
        WAL_MAGIC_LE => false,
        WAL_MAGIC_BE => true,
        _ => return (None, BTreeMap::new()),
    };
    let wal_ps = u32::from_be_bytes(hdr[8..12].try_into().expect("4 bytes")) as usize;
    if wal_ps != expected_page_size {
        return (None, BTreeMap::new());
    }
    let (mut s0, mut s1) = wal_checksum(&hdr[..24], 0, 0, big_endian);
    let hdr_c0 = u32::from_be_bytes(hdr[24..28].try_into().expect("4 bytes"));
    let hdr_c1 = u32::from_be_bytes(hdr[28..32].try_into().expect("4 bytes"));
    if (s0, s1) != (hdr_c0, hdr_c1) {
        return (None, BTreeMap::new());
    }
    let salt = &hdr[16..24];
    let committed = scan_wal_frames(&mut wal, wal_ps, salt, &mut s0, &mut s1, big_endian);
    (Some(wal), committed)
}

fn scan_wal_frames(
    wal: &mut File,
    page_size: usize,
    salt: &[u8],
    s0: &mut u32,
    s1: &mut u32,
    big_endian: bool,
) -> BTreeMap<u32, u64> {
    let mut committed = BTreeMap::new();
    let mut pending = Vec::new();
    let mut frame_hdr = [0u8; 24];
    let mut page_buf = vec![0u8; page_size];
    let mut offset = 32u64;
    for _ in 0..MAX_WAL_FRAMES {
        if wal.read_exact(&mut frame_hdr).is_err() || wal.read_exact(&mut page_buf).is_err() {
            break;
        }
        if &frame_hdr[8..16] != salt {
            break;
        }
        let (n0, n1) = wal_checksum(&frame_hdr[..8], *s0, *s1, big_endian);
        let (n0, n1) = wal_checksum(&page_buf, n0, n1, big_endian);
        let exp0 = u32::from_be_bytes(frame_hdr[16..20].try_into().expect("4 bytes"));
        let exp1 = u32::from_be_bytes(frame_hdr[20..24].try_into().expect("4 bytes"));
        if (n0, n1) != (exp0, exp1) {
            break;
        }
        *s0 = n0;
        *s1 = n1;
        let page_no = u32::from_be_bytes(frame_hdr[0..4].try_into().expect("4 bytes"));
        let db_pages = u32::from_be_bytes(frame_hdr[4..8].try_into().expect("4 bytes"));
        pending.push((page_no, offset + 24));
        if db_pages > 0 {
            for (p, off) in pending.drain(..) {
                committed.insert(p, off);
            }
        }
        offset += 24 + page_size as u64;
    }
    committed
}

fn wal_checksum(data: &[u8], mut s0: u32, mut s1: u32, big_endian: bool) -> (u32, u32) {
    let (chunks, _) = data.as_chunks::<8>();
    for &[a0, a1, a2, a3, b0, b1, b2, b3] in chunks {
        let x0 = if big_endian {
            u32::from_be_bytes([a0, a1, a2, a3])
        } else {
            u32::from_le_bytes([a0, a1, a2, a3])
        };
        let x1 = if big_endian {
            u32::from_be_bytes([b0, b1, b2, b3])
        } else {
            u32::from_le_bytes([b0, b1, b2, b3])
        };
        s0 = s0.wrapping_add(x0).wrapping_add(s1);
        s1 = s1.wrapping_add(x1).wrapping_add(s0);
    }
    (s0, s1)
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

fn decode_record(payload: &[u8]) -> Result<Vec<SqlVal>, String> {
    let (hdr_len, mut pos) = read_varint(payload)?;
    let hdr_end = usize::try_from(hdr_len)
        .ok()
        .filter(|&e| e <= payload.len())
        .ok_or_else(|| "SQLite record header out of bounds".to_owned())?;
    let mut serials = Vec::new();
    while pos < hdr_end {
        let (st, n) = read_varint(&payload[pos..hdr_end])?;
        serials.push(st);
        pos += n;
    }
    let mut data_pos = hdr_end;
    let mut cols = Vec::with_capacity(serials.len());
    for st in serials {
        let (val, used) = decode_column(st, &payload[data_pos..])?;
        data_pos += used;
        cols.push(val);
    }
    Ok(cols)
}

fn decode_column(serial: u64, rest: &[u8]) -> Result<(SqlVal, usize), String> {
    match serial {
        0 => Ok((SqlVal::Null, 0)),
        1..=6 => decode_signed_int(serial as usize, rest),
        7 => Ok((SqlVal::Other, 8.min(rest.len()))),
        8 => Ok((SqlVal::Int(0), 0)),
        9 => Ok((SqlVal::Int(1), 0)),
        n if n >= 12 => {
            let len = ((n - 12) / 2) as usize;
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

fn decode_signed_int(serial: usize, rest: &[u8]) -> Result<(SqlVal, usize), String> {
    let len = match serial {
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 6,
        6 => 8,
        _ => 0,
    };
    let slice = rest
        .get(..len)
        .ok_or_else(|| "SQLite integer truncated".to_owned())?;
    let mut v = if slice[0] & 0x80 != 0 { -1i64 } else { 0i64 };
    for &b in slice {
        v = (v << 8) | i64::from(b);
    }
    Ok((SqlVal::Int(v), len))
}
