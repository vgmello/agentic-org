//! Disposable real-process confinement check. Run outside a restricting parent
//! sandbox: cargo run --locked --offline -p org-socket --example gateway_confinement.
//! Never touches services, credentials or organization roots.
#[cfg(target_os = "macos")]
mod macos {
    use org_socket::{
        confinement,
        rights::{self, StreamReceiver},
    };
    use std::{
        fs,
        io::{BufRead, BufReader, Read, Write},
        net::{TcpListener, TcpStream},
        os::{
            fd::{AsFd, AsRawFd},
            unix::{
                fs::DirBuilderExt,
                net::{UnixListener, UnixStream},
            },
        },
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    const WAIT: Duration = Duration::from_secs(5);
    struct Root(PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    struct Running(Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn denied<T>(result: std::io::Result<T>) {
        let Err(error) = result else {
            panic!("ambient operation unexpectedly allowed");
        };
        assert!(
            matches!(error.raw_os_error(), Some(1 | 13)),
            "must be enforcement, not an absent fixture: {error}"
        );
    }
    fn line(reader: &mut BufReader<impl Read + AsRawFd>) -> String {
        assert!(org_socket::poll_readable(reader.get_ref().as_raw_fd(), WAIT).unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        line
    }
    fn child(args: &[String]) {
        let root = Path::new(&args[1]);
        let rendezvous = Path::new(&args[2]);
        if args[0] == "--thread" {
            std::thread::spawn(|| {}).join().unwrap();
            assert!(confinement::restrict_gateway(rendezvous).is_err());
            println!("thread-refused");
            return;
        }
        confinement::disable_core_dumps().unwrap();
        confinement::restrict_gateway(rendezvous).unwrap();
        println!("enforced");
        std::io::stdout().flush().unwrap();
        let mut approved = UnixStream::connect(rendezvous).unwrap();
        approved.set_read_timeout(Some(WAIT)).unwrap();
        denied(fs::read(root.join("fixture")));
        denied(fs::write(root.join("fixture"), b"changed"));
        denied(UnixStream::connect(root.join("unrelated")));
        denied(TcpStream::connect(&args[3]));
        denied(TcpListener::bind("127.0.0.1:0"));
        denied(Command::new("/usr/bin/true").status());
        let fixture = root.join("fixture");
        std::thread::spawn(move || denied(fs::read(fixture)))
            .join()
            .unwrap();
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
    pub(super) fn run() {
        let args: Vec<_> = std::env::args().skip(1).collect();
        if !args.is_empty() {
            child(&args);
            return;
        }
        let temp = PathBuf::from("/private/tmp").join(format!(
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
        for name in ["r", "r\"\\\né"] {
            let path = root.0.join(name);
            let listener = UnixListener::bind(&path).unwrap();
            let mut process = Running(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--child",
                        root.0.to_str().unwrap(),
                        path.to_str().unwrap(),
                        &outbound.local_addr().unwrap().to_string(),
                    ])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap(),
            );
            let mut stdout = BufReader::new(process.0.stdout.take().unwrap());
            assert_eq!(line(&mut stdout), "enforced\n");
            confinement::verify_gateway(process.0.id(), &[&root.0.join("fixture")]).unwrap();
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
            assert!(process.0.wait().unwrap().success());
        }
        let mut negative = Running(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--thread",
                    root.0.to_str().unwrap(),
                    root.0.join("r").to_str().unwrap(),
                    &outbound.local_addr().unwrap().to_string(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        assert_eq!(
            line(&mut BufReader::new(negative.0.stdout.take().unwrap())),
            "thread-refused\n"
        );
        assert!(negative.0.wait().unwrap().success());
        drop((unrelated, outbound));
        assert_eq!(fs::read(root.0.join("fixture")).unwrap(), b"retained");
        println!("PASS: normal/special pathname, main-thread enforcement, parent observation, inherited listener/rights, denied ambient I/O/exec/connect, inherited thread policy and never-threaded refusal; disposable roots only");
    }
}

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("this reviewed vector is macOS only");
}
