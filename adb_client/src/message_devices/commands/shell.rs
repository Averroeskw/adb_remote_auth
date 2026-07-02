use std::io::{ErrorKind, Read, Write};

use crate::models::ADBLocalCommand;
use crate::{
    Result, RustADBError,
    message_devices::{
        adb_message_device::ADBMessageDevice, adb_message_transport::ADBMessageTransport,
        adb_transport_message::ADBTransportMessage, commands::utils::ShellMessageWriter,
        message_commands::MessageCommand,
    },
};

impl<T: ADBMessageTransport> ADBMessageDevice<T> {
    /// Runs 'command' in a shell on the device, and write its output and error streams into output.
    pub(crate) fn shell_command(
        &mut self,
        command: &dyn AsRef<str>,
        mut stdout: Option<&mut dyn Write>,
        _stderr: Option<&mut dyn Write>,
    ) -> Result<Option<u8>> {
        let mut session = self.open_session(&ADBLocalCommand::ShellCommand(
            command.as_ref().to_string(),
            Vec::new(),
        ))?;

        loop {
            let message = session.recv_and_reply_okay()?;
            if message.header().command() == MessageCommand::Clse {
                break;
            }
            // should this just write for ::Write messages?
            if let Some(ref mut stdout) = stdout {
                stdout.write_all(&message.into_payload())?;
            }
        }

        Ok(None)
    }

    /// Starts an interactive shell session on the device.
    /// Input data is read from [reader] and write to [writer].
    pub(crate) fn shell(
        &mut self,
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
    ) -> Result<()> {
        self.bidirectional_session(&ADBLocalCommand::Shell, reader, writer)
    }

    /// Runs `command` on the device.
    /// Input data is read from [reader] and write to [writer].
    pub(crate) fn exec(
        &mut self,
        command: &str,
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
    ) -> Result<()> {
        self.bidirectional_session(&ADBLocalCommand::Exec(command.to_string()), reader, writer)
    }

    /// Starts an bidirectional(interactive) session. This can be a shell or an exec session.
    fn bidirectional_session(
        &mut self,
        local_command: &ADBLocalCommand,
        mut reader: Box<dyn Read + Send>,
        mut writer: Box<dyn Write + Send>,
    ) -> Result<()> {
        let mut session = self.open_session(local_command)?;

        let local_id = session.local_id();
        let remote_id = session.remote_id();

        // Writing thread, reads from given reader (that could be stdin e.g), and writes content to device adbd.
        // Deliberately detached: it may stay blocked reading (e.g on stdin) after the session is over,
        // and will die with the process once this (calling) thread has returned.
        let mut write_transport = self.get_transport_mut().clone();
        std::thread::spawn(move || {
            let mut shell_writer =
                ShellMessageWriter::new(write_transport.clone(), local_id, remote_id);
            match std::io::copy(&mut reader, &mut shell_writer) {
                Ok(_) => {
                    // Reader is exhausted (e.g EOF on piped stdin), close our side of the session.
                    // Device will answer with a `CLSE` message, terminating the reading loop below.
                    if let Ok(message) =
                        ADBTransportMessage::try_new(MessageCommand::Clse, local_id, remote_id, &[])
                    {
                        let _ = write_transport.write_message(message);
                    }
                }
                Err(e) if e.kind() == ErrorKind::BrokenPipe => (),
                Err(e) => log::error!("Error while writing to device shell: {e}"),
            }
        });

        // Reading loop, reads response from adbd in the calling thread, so that returning from
        // this function happens as soon as the device closes the session (EOF).
        loop {
            let message = session.get_transport_mut().read_message()?;

            match message.header().command() {
                MessageCommand::Write => {
                    // Acknowledge for more data
                    let response = ADBTransportMessage::try_new(
                        MessageCommand::Okay,
                        local_id,
                        remote_id,
                        &[],
                    )?;
                    session.get_transport_mut().write_message(response)?;

                    writer.write_all(&message.into_payload())?;
                    writer.flush()?;
                }
                MessageCommand::Okay => {}
                MessageCommand::Clse => {
                    // Device closed the session (EOF), acknowledge and terminate gracefully
                    let response = ADBTransportMessage::try_new(
                        MessageCommand::Clse,
                        local_id,
                        remote_id,
                        &[],
                    )?;
                    let _ = session.get_transport_mut().write_message(response);
                    return Ok(());
                }
                _ => return Err(RustADBError::ADBShellNotSupported),
            }
        }
    }
}
