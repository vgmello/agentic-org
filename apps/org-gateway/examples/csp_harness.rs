//! Test-only loopback listener for the production UI build.

use std::io::{self, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::process::ExitCode;

use org_gateway::assets::BuildFiles;
use org_gateway::service::{run_gateway, ApiLimits, GatewayConfig, UnavailableApi};
use org_gateway::transport::{HttpLimits, ListenMode};
use org_gateway::Hsts;
use org_host_control::listen::ListenOrigin;

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
    let origin = ListenOrigin::parse(&format!("http://{}", listener.local_addr()?))
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    run_gateway(
        listener,
        GatewayConfig {
            mode: ListenMode::HttpLoopback,
            origin,
            hsts: Hsts::Off,
            http: HttpLimits::default(),
            api: ApiLimits::default(),
        },
        files,
        move || {
            Ok((UnavailableApi, async {
                println!("ready");
                let _ = io::stdout().flush();
                std::future::pending::<()>().await;
            }))
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
