use crate::{ADBServer, ADBTransport, Result, TCPServerTransport, models::AdbServerCommand};

impl ADBServer {
    /// Asks the ADB server to quit immediately.
    pub fn kill(&mut self) -> Result<()> {
        self.connect()?
            .proxy_connection(AdbServerCommand::Kill, false)
            .map(|_| ())
    }

    /// Returns whether an ADB server is currently listening on this server's address.
    ///
    /// Performs a plain TCP probe; never starts a server and has no side effects.
    pub fn is_running(&self) -> bool {
        let mut transport = TCPServerTransport::new_or_default(self.socket_addr);
        transport.connect().is_ok()
    }

    /// Asks a potentially running ADB server to quit immediately.
    ///
    /// Contrary to [`ADBServer::kill()`], this method does not start a new server instance
    /// if none is already running.
    pub fn kill_if_running(&mut self) -> Result<()> {
        let mut transport = TCPServerTransport::new_or_default(self.socket_addr);
        if transport.connect().is_err() {
            // No server is currently listening
            log::debug!("no ADB server listening on {}", transport.get_socketaddr());
            return Ok(());
        }
        self.transport = Some(transport);

        log::info!("killing currently running ADB server...");
        self.get_transport()?
            .proxy_connection(AdbServerCommand::Kill, false)
            .map(|_| ())
    }
}
