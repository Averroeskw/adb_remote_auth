use std::io::{ErrorKind, Read, Write};

use crate::Result;
use crate::device::ShellMessageWriter;
use crate::{
    ADBMessageTransport, RustADBError,
    device::{ADBMessageDevice, ADBTransportMessage, MessageCommand},
};

impl<T: ADBMessageTransport> ADBMessageDevice<T> {
    /// Runs 'command' in a shell on the device, and write its output and error streams into output.
    pub(crate) fn shell_command(&mut self, command: &[&str], output: &mut dyn Write) -> Result<()> {
        let response = self.open_session(format!("shell:{}\0", command.join(" "),).as_bytes())?;

        if response.header().command() != MessageCommand::Okay {
            return Err(RustADBError::ADBRequestFailed(format!(
                "wrong command {}",
                response.header().command()
            )));
        }

        loop {
            let response = self.get_transport_mut().read_message()?;
            if response.header().command() != MessageCommand::Write {
                break;
            }

            output.write_all(&response.into_payload())?;
        }

        Ok(())
    }

    /// Starts an interactive shell session on the device.
    /// Input data is read from [reader] and write to [writer].
    pub(crate) fn shell(
        &mut self,
        mut reader: Box<dyn Read + Send>,
        mut writer: Box<dyn Write + Send>,
    ) -> Result<()> {
        self.open_session(b"shell:\0")?;

        let mut transport = self.get_transport().clone();

        let local_id = self.get_local_id()?;
        let remote_id = self.get_remote_id()?;

        // Writing thread, reads from given reader (that could be stdin e.g), and writes content to device adbd.
        // Deliberately detached: it may stay blocked reading (e.g on stdin) after the session is over,
        // and will die with the process once this (calling) thread has returned.
        let mut write_transport = self.get_transport().clone();
        std::thread::spawn(move || {
            let mut shell_writer =
                ShellMessageWriter::new(write_transport.clone(), local_id, remote_id);
            match std::io::copy(&mut reader, &mut shell_writer) {
                Ok(_) => {
                    // Reader is exhausted (e.g EOF on stdin), close our side of the session.
                    // Device will answer with a `CLSE` message, terminating the reading loop below.
                    let message =
                        ADBTransportMessage::new(MessageCommand::Clse, local_id, remote_id, &[]);
                    let _ = write_transport.write_message(message);
                }
                Err(e) if e.kind() == ErrorKind::BrokenPipe => (),
                Err(e) => log::error!("Error while writing to device shell: {e}"),
            }
        });

        // Reading loop, reads response from adbd in the calling thread, so that returning from
        // this function happens as soon as the device closes the session (EOF).
        loop {
            let message = transport.read_message()?;

            match message.header().command() {
                MessageCommand::Write => {
                    // Acknowledge for more data
                    let response =
                        ADBTransportMessage::new(MessageCommand::Okay, local_id, remote_id, &[]);
                    transport.write_message(response)?;

                    writer.write_all(&message.into_payload())?;
                    writer.flush()?;
                }
                MessageCommand::Okay => continue,
                MessageCommand::Clse => {
                    // Device closed the session (EOF), acknowledge and terminate gracefully
                    let response =
                        ADBTransportMessage::new(MessageCommand::Clse, local_id, remote_id, &[]);
                    let _ = transport.write_message(response);
                    return Ok(());
                }
                _ => return Err(RustADBError::ADBShellNotSupported),
            }
        }
    }
}
