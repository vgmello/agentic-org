//! Disposable real-process confinement check. Run outside a restricting parent
//! sandbox: cargo run --locked --offline -p org-socket --example gateway_confinement.
//! Never touches services, credentials or organization roots.
//!
//! One vector for every system with a web confinement adapter (macOS: ADR-0008;
//! GNU/Linux: Linux host design, section 3.3); where the web process has none it
//! refuses to run. What a system expects comes from the contract, never from a
//! platform conditional: the child's `enforced` line carries what `confine_web`
//! reported, as the web process's ready line does, and a report with a seccomp filter
//! means a forbidden operation kills the process instead of returning an error. There
//! each denial is proved by its own child, which must die of `SIGSYS`.
//!
//! Whoever starts a process that will confine itself must give it exactly its three
//! standard descriptors: every adapter refuses a process that holds another. The product's
//! host does that through its admission. This vector does it through the platform's
//! `spawn_piped`, which starts a child with three pipes and nothing else of this process,
//! on every system, so the vector passes whatever descriptors the process that runs it was
//! left with. `--start <role> ...` is that start alone, for a test that must run one role
//! under a profile of its own: it starts the role so, relays its standard output and exits
//! with its status.
//!
//! The layers that were enforced are the contract's too (`confinement::layers`), and
//! the `PASS` line names them. Where the adapter has a layer it applies only when
//! the system offers it (Landlock on GNU/Linux), a child that went without it prints
//! why, the vector holds with the remaining layer alone, and the `PASS` line says so.
mod vector {
    use org_platform::confinement::{self, Confined, Enforcement};
    use org_socket::rights::{self, StreamReceiver};
    use std::{
        ffi::CString,
        fs,
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        os::{
            fd::{AsFd, AsRawFd},
            unix::{
                ffi::{OsStrExt, OsStringExt},
                fs::DirBuilderExt,
                net::{UnixListener, UnixStream},
                process::ExitStatusExt,
            },
        },
        path::{Path, PathBuf},
        process::{Command, ExitStatus},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    const WAIT: Duration = Duration::from_secs(5);
    /// The contract's fixed refusal text; an adapter that can say more appends a
    /// colon and the cause.
    const REFUSAL: &str = "gateway confinement unavailable";
    /// The operations a confined process must not complete, one probe child each
    /// where a forbidden operation kills.
    const PROBES: [&str; 11] = [
        "read",
        "write",
        "create",
        "rename",
        "remove",
        "environment",
        "unix-connect",
        "tcp-connect",
        "tcp-bind",
        "exec",
        "thread-read",
    ];
    struct Root(PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    /// A child of this vector: started with exactly its three standard descriptors,
    /// killed and reaped when dropped.
    struct Running {
        pid: libc::pid_t,
        stdout: Option<fs::File>,
        _stdin: fs::File,
        reaped: bool,
    }
    impl Running {
        fn id(&self) -> u32 {
            self.pid as u32
        }
        fn stdout(&mut self) -> BufReader<fs::File> {
            BufReader::new(self.stdout.take().expect("the child's output, taken once"))
        }
        /// Waits for the child and returns how it ended.
        fn wait(&mut self) -> ExitStatus {
            let mut status = 0;
            // SAFETY: waits for this process's own child, which nothing else reaps.
            assert_eq!(unsafe { libc::waitpid(self.pid, &mut status, 0) }, self.pid);
            self.reaped = true;
            ExitStatus::from_raw(status)
        }
    }
    impl Drop for Running {
        fn drop(&mut self) {
            if !self.reaped {
                // SAFETY: this process's own unreaped child: its id names nothing else.
                unsafe { libc::kill(self.pid, libc::SIGKILL) };
                self.wait();
            }
        }
    }
    /// `operation`, run by a confined process, was refused by the confinement itself.
    fn denied<T>(operation: &str, result: std::io::Result<T>) {
        refused_with(operation, result, &[]);
    }
    /// [`denied`], where the system may also report the refusal as one of `also`.
    fn refused_with<T>(operation: &str, result: std::io::Result<T>, also: &[i32]) {
        let Err(error) = result else {
            panic!("{operation}: ambient operation unexpectedly allowed");
        };
        assert!(
            matches!(error.raw_os_error(), Some(1 | 13))
                || error
                    .raw_os_error()
                    .is_some_and(|errno| also.contains(&errno)),
            "{operation}: must be enforcement, not an absent fixture: {error} (errno {:?})",
            error.raw_os_error()
        );
    }
    fn line(reader: &mut BufReader<impl Read + AsRawFd>) -> String {
        // A line already read from the pipe with the one before it is not waited for.
        if !reader.buffer().contains(&b'\n') {
            assert!(org_socket::poll_readable(reader.get_ref().as_raw_fd(), WAIT).unwrap());
        }
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        line
    }
    /// What `confine_web` reported, as one line for the parent: the example's ready
    /// line.
    fn enforced(report: &Enforcement) -> String {
        let landlock = report
            .landlock_abi
            .map_or("none".to_owned(), |abi| abi.to_string());
        format!("enforced landlock={landlock} seccomp={}\n", report.seccomp)
    }
    /// What a child that went without Landlock prints after its `enforced` line.
    const NOT_APPLIED: &str = "landlock not applied: ";
    /// A child's report as the parent takes it: its `enforced` line and, where the
    /// contract says a Landlock layer is missing from it, the line that says why.
    /// Returns the report and the layers in words.
    fn report(stdout: &mut BufReader<impl Read + AsRawFd>) -> (Enforcement, String) {
        let report = reported(&line(stdout));
        let layers = confinement::layers(&report).unwrap();
        if !layers.landlock_missing {
            return (report, layers.description);
        }
        let why = line(stdout);
        let reason = why
            .strip_prefix(NOT_APPLIED)
            .and_then(|rest| rest.strip_suffix('\n'))
            .filter(|reason| !reason.is_empty())
            .unwrap_or_else(|| panic!("the child did not say why it has no Landlock: {why:?}"));
        (
            report,
            format!(
                "{} (Landlock was not applied: {reason})",
                layers.description
            ),
        )
    }
    /// The report of a child's `enforced` line.
    fn reported(line: &str) -> Enforcement {
        let fields = line
            .strip_prefix("enforced landlock=")
            .and_then(|rest| rest.strip_suffix('\n'))
            .and_then(|rest| rest.split_once(" seccomp="));
        let Some((landlock, seccomp)) = fields else {
            panic!("the child did not report enforcement: {line:?}");
        };
        Enforcement {
            landlock_abi: (landlock != "none").then(|| landlock.parse().unwrap()),
            seccomp: seccomp.parse().unwrap(),
        }
    }
    /// A process that does not meet the entry preconditions: the refusal must be the
    /// entry check's, not a failed connect to the rendezvous. It has no OS error
    /// number, it has the adapter's refusal text, and nothing was confined.
    fn refused_at_entry(root: &Path, rendezvous: &Path) {
        let error = confinement::confine_web(rendezvous).unwrap_err();
        entry_refusal(root, &error);
    }
    /// Checks that `error` is the entry check's refusal and that nothing was confined.
    fn entry_refusal(root: &Path, error: &std::io::Error) {
        assert!(
            error.raw_os_error().is_none(),
            "refused at entry, not at connect: {error}"
        );
        let text = error.to_string();
        assert!(
            text == REFUSAL || text.starts_with(&format!("{REFUSAL}: ")),
            "the adapter's own refusal: {text}"
        );
        assert_eq!(fs::read(root.join("fixture")).unwrap(), b"retained");
    }
    /// The role that does nothing: what the `exec` attempts would start.
    const NOOP: &str = "--noop";
    /// This program, as the parent named it when it started this child: the first
    /// argument, which needs no system call to read.
    fn own_program() -> PathBuf {
        PathBuf::from(std::env::args_os().next().expect("a first argument"))
    }
    /// One forbidden operation, by its probe name.
    fn attempt(probe: &str, root: &Path, outbound: &str) -> std::io::Result<()> {
        let fixture = root.join("fixture");
        match probe {
            "read" => fs::read(fixture).map(drop),
            "write" => fs::write(fixture, b"changed"),
            "create" => fs::write(root.join("created"), b"created"),
            "rename" => fs::rename(fixture, root.join("renamed")),
            "remove" => fs::remove_file(fixture),
            "environment" => fs::read("/proc/self/environ").map(drop),
            "unix-connect" => UnixStream::connect(root.join("unrelated")).map(drop),
            "tcp-connect" => TcpStream::connect(outbound).map(drop),
            "tcp-bind" => TcpListener::bind("127.0.0.1:0").map(drop),
            "exec" => Command::new(own_program()).arg(NOOP).status().map(drop),
            "thread-read" => std::thread::spawn(move || fs::read(fixture).map(drop))
                .join()
                .unwrap(),
            other => panic!("unknown probe {other}"),
        }
    }
    /// Confines this process, or prints the refusal and exits with status 2: a
    /// process that cannot confine itself reports why and serves nothing.
    fn confine(rendezvous: &Path) -> (UnixStream, Enforcement) {
        confinement::disable_core_dumps().unwrap();
        // Connected to the approved rendezvous and confined, inside the adapter.
        let Confined {
            stream: approved,
            enforcement: report,
            landlock_not_applied,
        } = match confinement::confine_web(rendezvous) {
            Ok(confined) => confined,
            Err(error) => {
                println!("refused: {error}");
                std::process::exit(2);
            }
        };
        confinement::verify_ready_report(&report).unwrap();
        print!("{}", enforced(&report));
        if let Some(reason) = landlock_not_applied {
            println!("{NOT_APPLIED}{reason}");
        }
        std::io::stdout().flush().unwrap();
        (approved, report)
    }
    fn child(args: &[String]) {
        if args[0] == NOOP {
            return;
        }
        if args[0] == "--start" {
            // The start alone: the role named after it, with exactly its three standard
            // descriptors, its output relayed and its status passed on.
            let role: Vec<&str> = args[1..].iter().map(String::as_str).collect();
            let mut started = start(&role);
            let mut output = Vec::new();
            started.stdout().read_to_end(&mut output).unwrap();
            std::io::stdout().write_all(&output).unwrap();
            std::io::stdout().flush().unwrap();
            let status = started.wait();
            std::process::exit(status.code().unwrap_or(128 + status.signal().unwrap_or(0)));
        }
        let root = Path::new(&args[1]);
        let rendezvous = Path::new(&args[2]);
        if args[0] == "--thread" {
            // A second thread that lives through the call: every adapter refuses a
            // process that has another thread.
            let (release, parked) = std::sync::mpsc::channel::<()>();
            let second = std::thread::spawn(move || parked.recv());
            refused_at_entry(root, rendezvous);
            drop(release);
            let _ = second.join().unwrap();
            println!("thread-refused");
            return;
        }
        if args[0] == "--joined-thread" {
            // A thread that has ended before the call, and no other. An adapter that
            // asks whether the process ever had a thread refuses; one that counts
            // the threads it has now may find one and confine it. The parent knows
            // from the first child's report which it must be.
            std::thread::spawn(|| {}).join().unwrap();
            match confinement::confine_web(rendezvous) {
                Err(error) => {
                    entry_refusal(root, &error);
                    println!("joined-thread-refused");
                }
                Ok(confined) => {
                    confinement::verify_ready_report(&confined.enforcement).unwrap();
                    println!("joined-thread-confined");
                }
            }
            return;
        }
        if args[0] == "--descriptor" {
            let held = fs::File::open(root.join("fixture")).unwrap();
            refused_at_entry(root, rendezvous);
            drop(held);
            println!("descriptor-refused");
            return;
        }
        if args[0] == "--probe" {
            let (_approved, report) = confine(rendezvous);
            assert!(report.seccomp, "a probe child is for a filter that kills");
            // Not reached where the filter works: the operation kills this process.
            let outcome = attempt(&args[4], root, &args[3]);
            println!("survived {}: {outcome:?}", args[4]);
            return;
        }
        let (mut approved, report) = confine(rendezvous);
        approved.set_read_timeout(Some(WAIT)).unwrap();
        if report.seccomp {
            // A forbidden operation would kill this process, so each one has its own
            // probe child. This process shows what must keep working: a thread,
            // memory, the clock and randomness, then the approved descriptors below.
            // Threads that allocate and free in their own allocator arenas, as the
            // web process's workers do under load. glibc must not open a file for it.
            let workers: Vec<_> = (0..4)
                .map(|_| {
                    std::thread::spawn(|| {
                        for _ in 0..4 {
                            let blocks: Vec<Vec<u8>> =
                                (0..256).map(|_| vec![1u8; 48 << 10]).collect();
                            assert_eq!(blocks.len(), 256);
                        }
                    })
                })
                .collect();
            for worker in workers {
                worker.join().unwrap();
            }
            // Buffers that grow by appending, here and on another thread (the C
            // library moves a large block with mremap), and a contended channel
            // (a waiting side yields with sched_yield).
            let grow = || {
                let mut bytes = Vec::new();
                (0..(1u32 << 20)).for_each(|value| bytes.push(value as u8));
                let mut text = String::new();
                while text.len() < 1 << 20 {
                    text.push_str("grown by appending ");
                }
                bytes.len() + text.len()
            };
            assert!(grow() > 2 << 20);
            assert!(std::thread::spawn(grow).join().unwrap() > 2 << 20);
            let (send, receive) = std::sync::mpsc::sync_channel::<u32>(0);
            let senders: Vec<_> = (0..4)
                .map(|_| {
                    let send = send.clone();
                    std::thread::spawn(move || (0..5_000).for_each(|n| send.send(n).unwrap()))
                })
                .collect();
            drop(send);
            assert_eq!(receive.iter().count(), 20_000);
            senders
                .into_iter()
                .for_each(|sender| sender.join().unwrap());
            let started = std::time::Instant::now();
            let sum = std::thread::spawn(|| {
                let block = vec![1u8; 8 << 20];
                block.iter().map(|byte| u64::from(*byte)).sum::<u64>()
            })
            .join()
            .unwrap();
            assert_eq!(sum, 8 << 20);
            assert!(started.elapsed() < WAIT);
            assert!(SystemTime::now().duration_since(UNIX_EPOCH).is_ok());
        } else {
            denied("read a file", fs::read(root.join("fixture")));
            denied("write a file", fs::write(root.join("fixture"), b"changed"));
            denied(
                "connect to another Unix socket",
                UnixStream::connect(root.join("unrelated")),
            );
            denied("connect over TCP", TcpStream::connect(&args[3]));
            denied("bind a TCP listener", TcpListener::bind("127.0.0.1:0"));
            // The program is this process's own executable, which exists: macOS 15 reports
            // the sandbox's refusal to start it as ENOENT (the profile denies even the
            // lookup), later systems as EPERM. Either is the refusal; it never ran.
            refused_with(
                "start a program",
                Command::new(own_program()).arg(NOOP).status(),
                &[libc::ENOENT],
            );
            let fixture = root.join("fixture");
            std::thread::spawn(move || denied("read a file on another thread", fs::read(fixture)))
                .join()
                .unwrap();
        }
        let mut random = [0; 32];
        confinement::fill_entropy(&mut random).unwrap();
        assert_ne!(random, [0; 32]);
        // Actual approved FD traffic after confinement, using the maintained rights
        // receiver and typed listener checks, not raw fake descriptor numbers.
        let mut receiver = StreamReceiver::new(approved.try_clone().unwrap());
        let until = std::time::Instant::now() + WAIT;
        let fds = loop {
            match receiver.receive_marker(1, 7, &std::sync::Mutex::new(())) {
                Ok(fds) => break fds,
                Err(rights::ReceiveError::WouldBlock) => {
                    assert!(org_socket::poll_readable(
                        approved.as_raw_fd(),
                        until.saturating_duration_since(std::time::Instant::now())
                    )
                    .unwrap());
                }
                Err(rights::ReceiveError::Fatal(_)) => std::process::exit(3),
            }
        };
        let listener = rights::tcp_listener(fds.into_iter().next().unwrap()).unwrap();
        let (mut client, _) = listener.accept().unwrap();
        client.write_all(b"accepted").unwrap();
        approved.write_all(b"complete").unwrap();
    }
    fn spawn(role: &[&str], root: &Path, rendezvous: &Path, outbound: &str) -> Running {
        let mut arguments = vec![
            role[0],
            root.to_str().unwrap(),
            rendezvous.to_str().unwrap(),
            outbound,
        ];
        arguments.extend(&role[1..]);
        start(&arguments)
    }
    /// Starts this program again with `arguments`, as the host starts the web process:
    /// three pipes for its standard descriptors and no other descriptor of this process,
    /// whatever this process holds, and no `RUST_BACKTRACE` (a backtrace opens files,
    /// which a confined process cannot do). Its standard error is copied to this one's.
    fn start(arguments: &[&str]) -> Running {
        let text = |bytes: &[u8]| CString::new(bytes).expect("no NUL in an argument");
        let program = std::env::current_exe().unwrap();
        let program = text(program.as_os_str().as_bytes());
        let mut argv = vec![program.clone()];
        argv.extend(arguments.iter().map(|argument| text(argument.as_bytes())));
        let environment: Vec<CString> = std::env::vars_os()
            .filter(|(name, _)| name != "RUST_BACKTRACE")
            .map(|(name, value)| {
                let mut pair = name.into_vec();
                pair.push(b'=');
                pair.extend(value.into_vec());
                text(&pair)
            })
            .collect();
        let (pid, stdin, stdout, mut stderr) =
            org_platform::process::spawn_piped(&program, &argv, &environment, c"/").unwrap();
        // A system that starts the child suspended lets it run here.
        org_platform::process::release_spawned(pid, || {
            Ok(org_platform::process::try_exit(pid)?.is_some())
        })
        .unwrap();
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut stderr, &mut std::io::stderr());
        });
        Running {
            pid,
            stdout: Some(stdout),
            _stdin: stdin,
            reaped: false,
        }
    }
    pub(super) fn run() {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if !args.is_empty() {
            // A child does what the web process does: it confines itself, or says
            // why it cannot. Only the parent asks whether the system has an adapter.
            child(&args);
            return;
        }
        // No adapter, no run: the vector never reports a pass without enforcement.
        if let Err(refused) = confinement::web_confinement_supported() {
            panic!("this reviewed vector needs a web confinement adapter: {refused}");
        }
        // `/private/tmp` on macOS, `/tmp` on Linux: short enough for a socket path.
        let temp = fs::canonicalize("/tmp").unwrap().join(format!(
            "agw-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::DirBuilder::new().mode(0o700).create(&temp).unwrap();
        let root = Root(temp);
        fs::write(root.0.join("fixture"), b"retained").unwrap();
        let unrelated = UnixListener::bind(root.0.join("unrelated")).unwrap();
        let outbound = TcpListener::bind("127.0.0.1:0").unwrap();
        let outbound_address = outbound.local_addr().unwrap().to_string();
        let mut kills = false;
        let mut confined_by = None;
        for name in ["r", "r\"\\\né"] {
            let path = root.0.join(name);
            let listener = UnixListener::bind(&path).unwrap();
            let mut process = spawn(&["--child"], &root.0, &path, &outbound_address);
            let mut stdout = process.stdout();
            // The report is checked as the host checks the ready line's, then the
            // process itself.
            let (report, layers) = report(&mut stdout);
            confinement::verify_ready_report(&report).unwrap();
            confinement::verify_web(process.id(), &report, &[&root.0.join("fixture")]).unwrap();
            kills = report.seccomp;
            // Every child is confined by the same layers.
            assert_eq!(*confined_by.get_or_insert_with(|| layers.clone()), layers);
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(WAIT)).unwrap();
            let inherited = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut browser = TcpStream::connect(inherited.local_addr().unwrap()).unwrap();
            browser.set_read_timeout(Some(WAIT)).unwrap();
            rights::send(
                stream.as_fd(),
                &[7],
                &[inherited.as_fd()],
                std::time::Instant::now() + WAIT,
            )
            .unwrap();
            let mut got = [0; 8];
            browser.read_exact(&mut got).unwrap();
            assert_eq!(&got, b"accepted");
            stream.read_exact(&mut got).unwrap();
            assert_eq!(&got, b"complete");
            assert!(process.wait().success());
        }
        if kills {
            // The filter kills on a forbidden call: no operation returns, so each is
            // attempted by its own confined child, which must die of SIGSYS having
            // printed nothing after its report.
            let path = root.0.join("probe");
            let listener = UnixListener::bind(&path).unwrap();
            for probe in PROBES {
                let mut process = spawn(&["--probe", probe], &root.0, &path, &outbound_address);
                let mut stdout = process.stdout();
                let (report, layers) = report(&mut stdout);
                assert!(report.seccomp, "{probe}");
                assert_eq!(Some(&layers), confined_by.as_ref(), "{probe}");
                let (held, _) = listener.accept().unwrap();
                let mut rest = String::new();
                stdout.read_to_string(&mut rest).unwrap();
                assert_eq!(rest, "", "{probe}: the operation returned");
                let status = process.wait();
                assert_eq!(
                    status.signal(),
                    Some(libc::SIGSYS),
                    "{probe}: the forbidden call must kill the process ({status})"
                );
                drop(held);
            }
            assert!(!root.0.join("created").exists());
            assert!(!root.0.join("renamed").exists());
            println!(
                "{} forbidden operations each killed their confined process with SIGSYS",
                PROBES.len()
            );
        }
        for (role, expected) in [
            ("--thread", "thread-refused\n"),
            ("--descriptor", "descriptor-refused\n"),
        ] {
            let mut negative = spawn(&[role], &root.0, &root.0.join("r"), &outbound_address);
            assert_eq!(line(&mut negative.stdout()), expected);
            assert!(negative.wait().success());
        }
        // A process whose only other thread has ended. Without a filter in the
        // report (the adapter that asks whether the process ever had a thread) it
        // must be refused. With one, the adapter counts the threads the process has
        // now: it is refused or confined, and either is right. Its rendezvous
        // listens, so a refusal is the entry check's and not a failed connection.
        let joined_rendezvous = root.0.join("j");
        let joined_listener = UnixListener::bind(&joined_rendezvous).unwrap();
        let mut joined = spawn(
            &["--joined-thread"],
            &root.0,
            &joined_rendezvous,
            &outbound_address,
        );
        let outcome = line(&mut joined.stdout());
        assert!(joined.wait().success());
        let joined_thread = match outcome.as_str() {
            "joined-thread-refused\n" => "refusal of a process whose thread had ended",
            "joined-thread-confined\n" if kills => {
                "confinement of a process whose thread had ended (one thread at entry)"
            }
            other => panic!("a process that had a thread: {other:?}"),
        };
        drop((unrelated, outbound, joined_listener));
        assert_eq!(fs::read(root.0.join("fixture")).unwrap(), b"retained");
        let confined_by = confined_by.expect("two children were confined");
        println!(
            "PASS: confined by {confined_by}; normal/special pathname, main-thread \
             enforcement, parent observation, inherited listener/rights, denied ambient \
             I/O/exec/connect, inherited thread policy, refusal of a process with a second \
             thread, {joined_thread}, refusal of a process with a fourth descriptor; \
             disposable roots only"
        );
    }
}

fn main() {
    vector::run();
}
