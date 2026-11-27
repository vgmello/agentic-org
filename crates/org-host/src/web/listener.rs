//! The browser listener: bound once by setup or the host and taken by the web process's
//! first start.

use std::{
    io,
    net::TcpListener,
    sync::{Arc, Mutex},
};

use org_host_control::listen::ListenConfig;

use crate::{cloexec::SPAWN_LOCK, sync::lock};

/// An instance's browser listener, bound before its host started: setup binds it when it
/// checks that the address it is about to reserve is free, and the web process's first
/// start takes it (X5 review, third round, P2). The address never changes once reserved,
/// so it is held from that check on, through the reservation, and no other program can
/// take the port before the web process serves it. It is handed over once: a clone shares
/// the one socket, and the first start that takes it leaves none.
#[derive(Clone, Debug)]
pub struct BrowserListener(Arc<Mutex<Option<TcpListener>>>);

impl BrowserListener {
    /// Binds the address of `listen`, as every start of the web process binds it.
    pub fn bind(listen: &ListenConfig) -> io::Result<Self> {
        bind(listen).map(|listener| Self(Arc::new(Mutex::new(Some(listener)))))
    }

    /// The listener, once.
    pub fn take(&self) -> Option<TcpListener> {
        lock(&self.0).take()
    }
}

/// Binds the browser listener of `listen`: its exact address and port (a port of zero is
/// refused before this, when the address is validated).
pub(super) fn bind(listen: &ListenConfig) -> io::Result<TcpListener> {
    // See `cloexec`: on macOS the socket is made close-on-exec in a second step.
    let _fence = lock(&SPAWN_LOCK);
    TcpListener::bind((listen.address(), listen.port()))
}
