//! Test-only loopback listener for the production UI build.

use std::convert::Infallible;
use std::io::{self, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::process::ExitCode;

use org_gateway::assets::BuildFiles;
use org_gateway::transport::{run_http, HttpLimits, ListenMode};
use org_gateway::{ConfiguredOrigin, Hsts, StaticUi};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("csp_harness: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let (ui_build, port) = arguments()?;
    let files = BuildFiles::from_directory(&ui_build).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid UI build: {error:?}"),
        )
    })?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let origin = ConfiguredOrigin::parse(&format!("http://{}", listener.local_addr()?))
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let service = StaticUi::new(files, origin.clone(), Hsts::Off);
    run_http(
        listener,
        ListenMode::HttpLoopback,
        origin,
        HttpLimits::default(),
        move || {
            println!("ready");
            let _ = io::stdout().flush();
            (
                move |request| {
                    std::future::ready(Ok::<_, Infallible>(
                        service.serve_with_unavailable_api(&request),
                    ))
                },
                std::future::pending(),
            )
        },
    )
}

fn arguments() -> io::Result<(PathBuf, u16)> {
    let usage = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: csp_harness --ui-build <dir> --port <port>",
        )
    };
    let mut args = std::env::args_os().skip(1);
    let mut build = None;
    let mut port = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(usage)?;
        match flag.to_str() {
            Some("--ui-build") if build.is_none() && !value.is_empty() => {
                build = Some(PathBuf::from(value))
            }
            Some("--port") if port.is_none() => {
                port = Some(
                    value
                        .to_str()
                        .and_then(|value| value.parse::<u16>().ok())
                        .filter(|port| *port != 0)
                        .ok_or_else(usage)?,
                );
            }
            _ => return Err(usage()),
        }
    }
    Ok((build.ok_or_else(usage)?, port.ok_or_else(usage)?))
}
