//! The property every family runs, on every engine: the projection the write path builds
//! equals the one a rebuild builds from the files it left, with and without a crash.
//!
//! ```text
//! for a generated sequence of the family's commands (valid, invalid, retried, interleaved):
//!     A = projection after Store::submit of each command, through the real writer
//!     B = rebuild from the durable directory A left behind, into a fresh engine
//!     assert dump(A) == dump(B)
//! and with a crash injected at a random point: reopen, then
//!     assert dump(reopened) == dump(rebuild into a fresh engine)
//!     assert every operation acknowledged before a process kill is present,
//!        and every durable one after a power cut
//! ```
//!
//! Every assertion prints the seed; a failure is repeated by running that seed alone.

use std::{ops::Range, path::PathBuf, sync::Arc, time::Duration};

use crate::{
    change::{Operation, Timestamp},
    custody::{OfficeIdentity, WriterAuthority},
    engine::{Engine, Projector},
    journal::AnchorFacts,
    store::{Opening, Reply, Store, StoreParts},
    testkit::{
        identity, probe::Decision, with_faults, CrashPoint, Dump, FakeCustody, Faults, FixedClock,
        Rng,
    },
};

/// One submission.
pub struct Command<E: Engine> {
    pub op: Option<Operation>,
    pub decide: Decision<E>,
}

/// A family's generator of commands. A `Spec` is plain data, so the harness can submit
/// the same command again as a retry.
pub trait FamilyScript<E: Engine>: Sync {
    type Spec: Clone + Send + Sync + std::fmt::Debug + 'static;
    fn projectors(&self, identity: &OfficeIdentity) -> Vec<Arc<dyn Projector<E>>>;
    /// The `step`th command of a sequence: valid or not, as the generator pleases.
    fn generate(&self, rng: &mut Rng, step: u64) -> Self::Spec;
    fn command(&self, spec: &Self::Spec) -> Command<E>;
}

struct Run<'a, E: Engine, S: FamilyScript<E>> {
    seed: u64,
    script: &'a S,
    root: PathBuf,
    faults: Arc<Faults>,
    custody: Arc<FakeCustody>,
    _dir: tempfile::TempDir,
    _engine: std::marker::PhantomData<E>,
}

impl<E: Engine + Dump, S: FamilyScript<E>> Run<'_, E, S> {
    fn open(&self, engine: &Arc<E>, opening: Opening) -> Result<Store<E>, crate::StoreError> {
        let mut parts = StoreParts::new(
            self.root.clone(),
            identity(),
            Arc::clone(&self.custody) as Arc<dyn WriterAuthority>,
            Arc::clone(engine),
            self.script.projectors(&identity()),
            Arc::new(FixedClock(Timestamp(1_700_000_000_000))),
        );
        parts.opening = opening;
        // Flushes come from durable requests, back-pressure and closing only.
        parts.flush_interval = Duration::from_secs(3_600);
        Store::open(with_faults(parts, &self.faults))
    }

    fn submit(
        &self,
        store: &Store<E>,
        spec: &S::Spec,
        durable: bool,
    ) -> Result<Reply, crate::StoreError> {
        let command = self.script.command(spec);
        store.submit(command.op, durable, command.decide)
    }

    fn existing(&self, engine: &Arc<E>) -> Store<E> {
        self.faults.revive();
        let opened = self.open(engine, Opening::Existing);
        opened.unwrap_or_else(|e| panic!("seed {}: reopening failed: {e}", self.seed))
    }

    fn close(&self, store: Store<E>) {
        let closed = store.close();
        closed.unwrap_or_else(|e| panic!("seed {}: closing failed: {e}", self.seed));
    }

    /// The dump of a rebuild from what is on disk, into a fresh engine.
    fn rebuilt(&self, fresh: &Arc<E>) -> Vec<String> {
        self.close(self.existing(fresh));
        fresh.dump()
    }
}

fn run<'a, E: Engine, S: FamilyScript<E>>(script: &'a S, seed: u64) -> Run<'a, E, S> {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir
        .path()
        .canonicalize()
        .expect("a real path")
        .join("durable");
    Run {
        seed,
        script,
        root,
        faults: Faults::new(),
        custody: Arc::new(FakeCustody::new()),
        _dir: dir,
        _engine: std::marker::PhantomData,
    }
}

/// Runs the property for every seed. `open_engine` returns a fresh, empty engine and
/// whatever must outlive it.
pub fn rebuild_equivalence<E, S, G>(
    open_engine: &dyn Fn() -> (Arc<E>, G),
    script: &S,
    seeds: Range<u64>,
) where
    E: Engine + Dump,
    S: FamilyScript<E>,
{
    let (mut runs, mut fired) = (0, 0);
    for seed in seeds {
        without_a_crash(open_engine, script, seed);
        fired += usize::from(with_a_crash(open_engine, script, seed));
        runs += 1;
    }
    // A point armed past the end of a run never acts; most must.
    assert!(
        fired * 2 >= runs,
        "only {fired} of {runs} armed crash points acted"
    );
}

fn without_a_crash<E, S, G>(open_engine: &dyn Fn() -> (Arc<E>, G), script: &S, seed: u64)
where
    E: Engine + Dump,
    S: FamilyScript<E>,
{
    let mut rng = Rng::new(seed);
    let run = run::<E, S>(script, seed);
    let (engine, _guard) = open_engine();
    let store = run.open(&engine, Opening::Create(AnchorFacts::default()));
    let store = store.unwrap_or_else(|e| panic!("seed {seed}: creating failed: {e}"));

    // Sequential commands, with retries of earlier ones.
    let mut done: Vec<(S::Spec, Result<Reply, crate::StoreError>)> = Vec::new();
    for step in 0..32 {
        if !done.is_empty() && rng.chance(4) {
            let (spec, first) = rng.pick(&done).clone();
            let again = run.submit(&store, &spec, rng.chance(2));
            let has_id = script.command(&spec).op.is_some();
            if let (true, Ok(first)) = (has_id, &first) {
                assert_eq!(
                    again.as_ref(),
                    Ok(first),
                    "seed {seed}: a retry of {spec:?} differs"
                );
            }
            continue;
        }
        let spec = script.generate(&mut rng, step);
        let reply = run.submit(&store, &spec, rng.chance(3));
        done.push((spec, reply));
    }
    // Interleaved commands from three threads.
    let bursts: Vec<Vec<S::Spec>> = (0..3u64)
        .map(|thread| {
            let steps = (0..6u64).map(|i| 1_000 + thread * 100 + i);
            steps.map(|step| script.generate(&mut rng, step)).collect()
        })
        .collect();
    std::thread::scope(|scope| {
        for burst in &bursts {
            let (run, store) = (&run, &store);
            scope.spawn(move || {
                for spec in burst {
                    let _ = run.submit(store, spec, false);
                }
            });
        }
    });
    run.close(store);
    let written = engine.dump();

    // A rebuild that dies part-way leaves nothing behind; the next one completes.
    let (fresh, _fresh_guard) = open_engine();
    run.faults.revive();
    run.faults
        .arm(CrashPoint::DuringRebuild, rng.between(1, 4) as usize);
    let died = run.open(&fresh, Opening::Existing);
    assert!(
        died.is_err(),
        "seed {seed}: a rebuild survived its crash point"
    );
    assert_eq!(
        run.rebuilt(&fresh),
        written,
        "seed {seed}: rebuild differs from the write path"
    );
    // Reopening the original changes nothing.
    run.close(run.existing(&engine));
    assert_eq!(
        engine.dump(),
        written,
        "seed {seed}: reopening changed the projection"
    );
}

fn with_a_crash<E, S, G>(open_engine: &dyn Fn() -> (Arc<E>, G), script: &S, seed: u64) -> bool
where
    E: Engine + Dump,
    S: FamilyScript<E>,
{
    let mut rng = Rng::new(seed ^ 0x5EED_C8A5);
    let run = run::<E, S>(script, seed);
    let (mut engine, _guard) = open_engine();
    let store = run.open(&engine, Opening::Create(AnchorFacts::default()));
    let store = store.unwrap_or_else(|e| panic!("seed {seed}: creating failed: {e}"));

    let points: Vec<CrashPoint> = CrashPoint::ALL
        .into_iter()
        .filter(|point| *point != CrashPoint::DuringRebuild)
        .collect();
    let point = match *rng.pick(&points) {
        CrashPoint::MidAppend(_) => CrashPoint::MidAppend(rng.between(1, 80) as usize),
        other => other,
    };
    let power_cut = rng.chance(2);
    let fresh_projection = rng.chance(3);
    let context =
        format!("seed {seed}: {point:?}, power cut {power_cut}, fresh {fresh_projection}");
    run.faults.arm(point, rng.between(1, 4) as usize);

    // (spec, reply, asked durable): what was acknowledged before the crash.
    let mut acknowledged: Vec<(S::Spec, Reply, bool)> = Vec::new();
    for step in 0..24 {
        if run.faults.dead() || store.status().stopped.is_some() {
            break;
        }
        let spec = script.generate(&mut rng, step);
        let durable = rng.chance(2);
        if let Ok(reply) = run.submit(&store, &spec, durable) {
            if script.command(&spec).op.is_some() {
                acknowledged.push((spec, reply, durable));
            }
        }
    }
    let fired = run.faults.fired();
    // The process dies here at the latest. Both threads are joined before the next one.
    run.faults.kill();
    drop(store);
    if power_cut {
        let cut = run.faults.power_cut();
        cut.unwrap_or_else(|e| panic!("{context}: the power cut failed: {e}"));
    }
    let mut guards = Vec::new();
    if fresh_projection {
        let (fresh, guard) = open_engine();
        engine = fresh;
        guards.push(guard);
    }

    let store = run.existing(&engine);
    for (spec, reply, durable) in &acknowledged {
        if power_cut && !durable {
            // A committed-class acknowledgement may be taken back by a power cut.
            continue;
        }
        let again = run.submit(&store, spec, false);
        assert_eq!(
            again.as_ref(),
            Ok(reply),
            "{context}: acknowledged {spec:?} is gone"
        );
    }
    run.close(store);
    let reopened = engine.dump();
    let (fresh, _fresh_guard) = open_engine();
    assert_eq!(
        run.rebuilt(&fresh),
        reopened,
        "{context}: rebuild differs after the crash"
    );
    fired
}
