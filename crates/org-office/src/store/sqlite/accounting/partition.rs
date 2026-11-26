//! Verifies durable stream prefixes and partitions usage records into projected and staged sets.

use std::collections::{BTreeMap, BTreeSet};

use org_proto::costs::UsageRecordView;

use crate::accounting::UsageStreamCheckpoint;

use super::{PartitionedStreams, StagedStreamSet, VerifiedStreamHead, VerifiedStreamMap};

struct VerifiedPrefixOutcome {
    head: VerifiedStreamHead,
    prefix_chains: Vec<(String, String)>,
    tail_ids: Vec<String>,
}

fn verify_single_stream_prefix(
    cp: &UsageStreamCheckpoint,
    stream_recs: &[&UsageRecordView],
) -> Option<VerifiedPrefixOutcome> {
    if (stream_recs.len() as u64) < cp.high_water_sequence {
        return None;
    }
    let mut expected_seq = 0u64;
    let mut last_payload_sha: Option<&str> = None;
    let mut running_chain: Option<String> = None;
    let mut prefix_chains: Vec<(String, String)> =
        Vec::with_capacity(cp.high_water_sequence as usize);
    for rec in stream_recs.iter().take(cp.high_water_sequence as usize) {
        expected_seq += 1;
        if rec.source_sequence != expected_seq {
            return None;
        }
        last_payload_sha = Some(rec.payload_sha256.as_str());
        let next_c = crate::accounting::chain_usage_stream_head(running_chain.as_deref(), rec);
        prefix_chains.push((rec.id.clone(), next_c.clone()));
        running_chain = Some(next_c);
    }
    if last_payload_sha != cp.head_sha256.as_deref()
        || cp
            .chain_sha256
            .as_deref()
            .is_some_and(|c| running_chain.as_deref() != Some(c))
    {
        return None;
    }
    let tail_ids = stream_recs
        .iter()
        .skip(cp.high_water_sequence as usize)
        .map(|extra| extra.id.clone())
        .collect();
    Some(VerifiedPrefixOutcome {
        head: VerifiedStreamHead {
            high_water_sequence: cp.high_water_sequence,
            head_sha256: cp.head_sha256.clone(),
            chain_sha256: running_chain,
        },
        prefix_chains,
        tail_ids,
    })
}

fn partition_topological_records(
    candidates: Vec<UsageRecordView>,
    chain_by_id: &mut BTreeMap<String, String>,
) -> (Vec<(UsageRecordView, Option<String>)>, Vec<UsageRecordView>) {
    let mut valid_ids: BTreeSet<String> = BTreeSet::new();
    let mut remaining = candidates;
    let mut projected = Vec::with_capacity(remaining.len());
    loop {
        let mut progressed = false;
        let mut next_remaining = Vec::new();
        for rec in remaining {
            let ready = rec
                .supersedes_id
                .as_deref()
                .is_none_or(|pid| valid_ids.contains(pid));
            if ready {
                let chain = chain_by_id.remove(&rec.id);
                valid_ids.insert(rec.id.clone());
                projected.push((rec, chain));
                progressed = true;
            } else {
                next_remaining.push(rec);
            }
        }
        remaining = next_remaining;
        if !progressed {
            break;
        }
    }
    (projected, remaining)
}

/// 1. Verifies each stream's complete durable sequence (`1..=hw` + `head_sha256` + `chain_sha256`)
///    against its durable checkpoint BEFORE checking `supersedes_id` parents (`Pass 6 #2`).
///    Unanchored or mismatched streams move entirely into `staged_records`.
/// 2. Topologically partitions verified records so valid corrections stay projected while any
///    correction whose parent is missing (or was in a quarantined stream) is staged individually,
///    removing its stream from `verified_streams` and recording it in `staged_streams`.
pub(super) fn verify_and_partition_streams(
    org_id: &str,
    checkpoints: &[UsageStreamCheckpoint],
    all_records: Vec<UsageRecordView>,
) -> PartitionedStreams {
    let cp_by_stream: BTreeMap<(&str, &str), &UsageStreamCheckpoint> = checkpoints
        .iter()
        .filter(|c| c.organization_id == org_id)
        .map(|c| ((c.origin_office_id.as_str(), c.producer_id.as_str()), c))
        .collect();
    let mut by_stream: BTreeMap<(String, String), Vec<&UsageRecordView>> = BTreeMap::new();
    for rec in &all_records {
        by_stream
            .entry((rec.origin_office_id.clone(), rec.producer_id.clone()))
            .or_default()
            .push(rec);
    }
    let mut bad_streams: BTreeSet<(String, String)> = BTreeSet::new();
    let mut tail_staged_ids: BTreeSet<String> = BTreeSet::new();
    let mut chain_by_id: BTreeMap<String, String> = BTreeMap::new();
    let mut verified_streams: VerifiedStreamMap = BTreeMap::new();
    let mut verified_prefixes: VerifiedStreamMap = BTreeMap::new();
    for (&(origin, producer), cp) in &cp_by_stream {
        if cp.high_water_sequence == 0
            && cp.head_sha256.is_none()
            && !by_stream.contains_key(&(origin.to_owned(), producer.to_owned()))
        {
            let head = VerifiedStreamHead {
                high_water_sequence: 0,
                head_sha256: None,
                chain_sha256: None,
            };
            let skey = (org_id.to_owned(), origin.to_owned(), producer.to_owned());
            verified_streams.insert(skey.clone(), head.clone());
            verified_prefixes.insert(skey, head);
        }
    }
    for (key, mut stream_recs) in by_stream {
        stream_recs.sort_by_key(|r| r.source_sequence);
        let outcome = cp_by_stream
            .get(&(key.0.as_str(), key.1.as_str()))
            .copied()
            .and_then(|cp| verify_single_stream_prefix(cp, &stream_recs));
        let Some(outcome) = outcome else {
            bad_streams.insert(key);
            continue;
        };
        chain_by_id.extend(outcome.prefix_chains);
        let skey = (org_id.to_owned(), key.0, key.1);
        verified_prefixes.insert(skey.clone(), outcome.head.clone());
        if outcome.tail_ids.is_empty() {
            verified_streams.insert(skey, outcome.head);
        } else {
            tail_staged_ids.extend(outcome.tail_ids);
        }
    }
    let (mut staged_records, candidates): (Vec<_>, Vec<_>) =
        all_records.into_iter().partition(|rec| {
            bad_streams.contains(&(rec.origin_office_id.clone(), rec.producer_id.clone()))
                || tail_staged_ids.contains(&rec.id)
        });
    let (projected, orphans) = partition_topological_records(candidates, &mut chain_by_id);
    for orphan in orphans {
        verified_streams.remove(&(
            org_id.to_owned(),
            orphan.origin_office_id.clone(),
            orphan.producer_id.clone(),
        ));
        staged_records.push(orphan);
    }
    let staged_streams: StagedStreamSet = staged_records
        .iter()
        .map(|r| {
            (
                org_id.to_owned(),
                r.origin_office_id.clone(),
                r.producer_id.clone(),
            )
        })
        .collect();
    PartitionedStreams {
        projected,
        staged: staged_records,
        verified: verified_streams,
        verified_prefixes,
        staged_streams,
    }
}
