//! Pure single-chain resolution (`doc-34` §3.1):
//! `organization -> accountable team ancestry (root to leaf) -> project`.
//! Contributing teams add no documents; there is one chain and no multi-chain conflicts.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{DocumentKind, DocumentStatus, ProcessKey, Scope, StructureDocInput};

/// Inputs to single-chain process and standard resolution.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SingleChainInputs {
    /// Accountable team ancestry from root team down to the accountable team (may be empty for
    /// organization-only inspection).
    pub team_chain: Vec<String>,
    /// Optional project ID at the bottom of the chain.
    pub project_id: Option<String>,
    /// All candidate structure documents in the organization.
    pub documents: Vec<StructureDocInput>,
    /// Display names for scopes (team/project names); defaults to scope ID or `"Organization"`.
    pub scope_names: BTreeMap<Scope, String>,
}

/// Output of [`resolve_single_chain`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectiveSet {
    /// Exact ordered scope chain evaluated: `[Organization, Team(..)..., Project(..)?]`.
    pub chain: Vec<Scope>,
    /// Resolved entries (one per unique key, plus one per keyless `current` document).
    pub entries: Vec<EffectiveEntry>,
    /// Every document in the chain left out due to an error, clash, or non-overridable refusal.
    pub problems: Vec<Layer>,
}

impl EffectiveSet {
    /// Looks up the entry for a given key (used by `process.why` and `process inspect --why`).
    pub fn find_by_key(&self, key: &str) -> Option<&EffectiveEntry> {
        self.entries
            .iter()
            .find(|e| e.key.as_ref().map(ProcessKey::as_str) == Some(key))
    }

    /// Looks up the winning document by key or document ID.
    pub fn winner_by_key_or_id(&self, key_or_id: &str) -> Option<&Winner> {
        self.entries.iter().find_map(|e| {
            let w = e.winner.as_ref()?;
            let matches_key = w.doc.key.as_ref().map(ProcessKey::as_str) == Some(key_or_id);
            let matches_id = w.doc.id == key_or_id;
            (matches_key || matches_id).then_some(w)
        })
    }
}

/// One resolved entry (keyed across levels, or a single keyless document).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveEntry {
    /// Stable identifier for this row: the key text when keyed, else the document ID.
    pub entry_id: String,
    /// Key when the entry is keyed across levels.
    pub key: Option<ProcessKey>,
    /// Kind of the winning (or first) document.
    pub kind: DocumentKind,
    /// Winning document, or `None` if clashed/invalid with no applying fallback.
    pub winner: Option<Winner>,
    /// Every non-draft, non-deprecated layer in chain order (organization first, project last).
    pub layers: Vec<Layer>,
}

/// The applying document for an [`EffectiveEntry`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Winner {
    /// Winning document metadata and Markdown body.
    pub doc: StructureDocInput,
    /// Whether this document was inherited from the organization, replaced one above, or added.
    pub kind: WinnerKind,
    /// The higher-level document replaced by this winner, when `kind == WinnerKind::Override`.
    pub replaces: Option<StructureDocInput>,
    /// Human-readable label for `doc.scope`.
    pub scope_name: String,
}

/// How the winning document relates to higher scopes in the chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinnerKind {
    /// From the organization scope (top of the chain).
    Inherited,
    /// Replaced an overridable document from a higher scope.
    Override,
    /// Added at a team or project scope with no document above it.
    Addition,
}

/// One document's participation and outcome in the inheritance chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// Document metadata and body.
    pub doc: StructureDocInput,
    /// Human-readable label for `doc.scope`.
    pub scope_name: String,
    /// Outcome in single-chain resolution.
    pub outcome: LayerOutcome,
}

/// What happened to one document during resolution (`doc-34` §3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerOutcome {
    /// Active winner in the effective set.
    Applies,
    /// Valid document replaced by a lower-level overridable override.
    Replaced,
    /// Refused because a higher level sealed this key (`overridable: false`).
    Refused {
        /// Scope that sealed the key.
        sealed_by_scope: Scope,
        /// Display name of the sealing scope.
        sealed_by_name: String,
        /// ID of the sealing document.
        sealed_by_id: String,
        /// Title of the sealing document.
        sealed_by_title: String,
    },
    /// Invalid front matter or path/scope mismatch.
    Invalid {
        /// Name of the invalid field.
        field: String,
    },
    /// Two or more active documents share the same key at the same scope.
    Clash {
        /// Display name of the scope where the clash occurred.
        scope_name: String,
    },
}

fn scope_label(names: &BTreeMap<Scope, String>, scope: &Scope) -> String {
    if let Some(name) = names.get(scope) {
        return name.clone();
    }
    match scope {
        Scope::Organization => "Organization".to_owned(),
        Scope::Team(id) | Scope::Project(id) => id.clone(),
    }
}

fn build_chain(inputs: &SingleChainInputs) -> Vec<Scope> {
    let mut chain = Vec::with_capacity(1 + inputs.team_chain.len() + 1);
    chain.push(Scope::Organization);
    let mut seen_teams = BTreeSet::new();
    for team_id in &inputs.team_chain {
        if seen_teams.insert(team_id.as_str()) {
            chain.push(Scope::Team(team_id.clone()));
        }
    }
    if let Some(project_id) = &inputs.project_id {
        chain.push(Scope::Project(project_id.clone()));
    }
    chain
}

#[derive(Clone)]
struct SealInfo {
    scope: Scope,
    scope_name: String,
    doc_id: String,
    doc_title: String,
}

/// Resolves organization processes and standards along one chain (`doc-34` §3.1).
pub fn resolve_single_chain(inputs: &SingleChainInputs) -> EffectiveSet {
    let chain = build_chain(inputs);
    let rank_of = |scope: &Scope| chain.iter().position(|s| s == scope);

    // Filter to documents on this chain that are not silently inactive (draft/deprecated).
    let mut active_docs: Vec<(usize, &StructureDocInput)> = inputs
        .documents
        .iter()
        .filter(|doc| !doc.is_inactive())
        .filter_map(|doc| rank_of(&doc.scope).map(|rank| (rank, doc)))
        .collect();
    active_docs.sort_by(|(ra, a), (rb, b)| (ra, &a.id, &a.path).cmp(&(rb, &b.id, &b.path)));

    let mut keyed: BTreeMap<ProcessKey, Vec<(usize, &StructureDocInput)>> = BTreeMap::new();
    let mut keyless: Vec<(usize, &StructureDocInput)> = Vec::new();
    for (rank, doc) in active_docs {
        if let Some(key) = &doc.key {
            keyed.entry(key.clone()).or_default().push((rank, doc));
        } else {
            keyless.push((rank, doc));
        }
    }

    let mut set = EffectiveSet {
        chain,
        entries: Vec::new(),
        problems: Vec::new(),
    };

    for (key, docs_for_key) in keyed {
        let entry = resolve_keyed_group(&key, &docs_for_key, &inputs.scope_names, &mut set);
        set.entries.push(entry);
    }

    for (_rank, doc) in keyless {
        let entry = resolve_keyless_doc(doc, &inputs.scope_names, &mut set);
        set.entries.push(entry);
    }

    set.entries.sort_by(|a, b| {
        let rank_a = a
            .winner
            .as_ref()
            .map(|w| w.doc.scope.level_rank())
            .unwrap_or(99);
        let rank_b = b
            .winner
            .as_ref()
            .map(|w| w.doc.scope.level_rank())
            .unwrap_or(99);
        (rank_a, a.kind, &a.entry_id).cmp(&(rank_b, b.kind, &b.entry_id))
    });

    set
}

fn resolve_keyless_doc(
    doc: &StructureDocInput,
    names: &BTreeMap<Scope, String>,
    set: &mut EffectiveSet,
) -> EffectiveEntry {
    let sname = scope_label(names, &doc.scope);
    if let Some(field) = &doc.invalid_field {
        let layer = Layer {
            doc: doc.clone(),
            scope_name: sname,
            outcome: LayerOutcome::Invalid {
                field: field.clone(),
            },
        };
        set.problems.push(layer.clone());
        return EffectiveEntry {
            entry_id: doc.id.clone(),
            key: None,
            kind: doc.kind,
            winner: None,
            layers: vec![layer],
        };
    }
    let wkind = if doc.scope == Scope::Organization {
        WinnerKind::Inherited
    } else {
        WinnerKind::Addition
    };
    let layer = Layer {
        doc: doc.clone(),
        scope_name: sname.clone(),
        outcome: LayerOutcome::Applies,
    };
    EffectiveEntry {
        entry_id: doc.id.clone(),
        key: None,
        kind: doc.kind,
        winner: Some(Winner {
            doc: doc.clone(),
            kind: wkind,
            replaces: None,
            scope_name: sname,
        }),
        layers: vec![layer],
    }
}

fn resolve_keyed_group(
    key: &ProcessKey,
    docs: &[(usize, &StructureDocInput)],
    names: &BTreeMap<Scope, String>,
    set: &mut EffectiveSet,
) -> EffectiveEntry {
    // Group by rank (scope position in the single chain) to detect same-scope clashes.
    let mut by_rank: BTreeMap<usize, Vec<&StructureDocInput>> = BTreeMap::new();
    for &(rank, doc) in docs {
        by_rank.entry(rank).or_default().push(doc);
    }

    let mut seal: Option<SealInfo> = None;
    let mut current_winner_idx: Option<usize> = None;
    let mut prev_winner_doc: Option<StructureDocInput> = None;
    let mut layers: Vec<Layer> = Vec::new();

    for group in by_rank.values() {
        let is_clash = group.len() > 1;
        for &doc in group {
            let sname = scope_label(names, &doc.scope);
            let outcome = evaluate_layer_doc(doc, &sname, is_clash, &seal);
            let applied = matches!(outcome, LayerOutcome::Applies);

            if doc.seals_key() && seal.is_none() {
                seal = Some(SealInfo {
                    scope: doc.scope.clone(),
                    scope_name: sname.clone(),
                    doc_id: doc.id.clone(),
                    doc_title: doc.title.clone(),
                });
            }

            if applied {
                if let Some(prev_idx) = current_winner_idx {
                    prev_winner_doc = Some(layers[prev_idx].doc.clone());
                    layers[prev_idx].outcome = LayerOutcome::Replaced;
                }
                current_winner_idx = Some(layers.len());
            }

            let layer = Layer {
                doc: doc.clone(),
                scope_name: sname,
                outcome,
            };
            if !matches!(
                layer.outcome,
                LayerOutcome::Applies | LayerOutcome::Replaced
            ) {
                set.problems.push(layer.clone());
            }
            layers.push(layer);
        }
    }

    let kind = current_winner_idx
        .map(|i| layers[i].doc.kind)
        .or_else(|| layers.first().map(|l| l.doc.kind))
        .unwrap_or(DocumentKind::Process);

    let winner = current_winner_idx.map(|idx| {
        let w = &layers[idx];
        let wkind = if prev_winner_doc.is_some() {
            WinnerKind::Override
        } else if w.doc.scope == Scope::Organization {
            WinnerKind::Inherited
        } else {
            WinnerKind::Addition
        };
        Winner {
            doc: w.doc.clone(),
            kind: wkind,
            replaces: prev_winner_doc,
            scope_name: w.scope_name.clone(),
        }
    });

    EffectiveEntry {
        entry_id: key.as_str().to_owned(),
        key: Some(key.clone()),
        kind,
        winner,
        layers,
    }
}

fn evaluate_layer_doc(
    doc: &StructureDocInput,
    scope_name: &str,
    is_clash: bool,
    seal: &Option<SealInfo>,
) -> LayerOutcome {
    if let Some(field) = &doc.invalid_field {
        return LayerOutcome::Invalid {
            field: field.clone(),
        };
    }
    if is_clash {
        return LayerOutcome::Clash {
            scope_name: scope_name.to_owned(),
        };
    }
    if let Some(sealed) = seal {
        return LayerOutcome::Refused {
            sealed_by_scope: sealed.scope.clone(),
            sealed_by_name: sealed.scope_name.clone(),
            sealed_by_id: sealed.doc_id.clone(),
            sealed_by_title: sealed.doc_title.clone(),
        };
    }
    debug_assert_eq!(doc.status, DocumentStatus::Current);
    LayerOutcome::Applies
}
