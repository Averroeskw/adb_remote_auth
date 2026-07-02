use std::{
    io::{ErrorKind, Read, Write},
    path::Path,
};

use crate::{
    ADBDeviceExt, Result, RustADBError,
    constants::BUFFER_SIZE,
    models::{AdbServerCommand, AdbStatResponse, HostFeatures},
};

use super::ADBServerDevice;

impl ADBDeviceExt for ADBServerDevice {
    fn shell_command(&mut self, command: &[&str], output: &mut dyn Write) -> Result<()> {
        let supported_features = self.host_features()?;
        if !supported_features.contains(&HostFeatures::ShellV2)
            && !supported_features.contains(&HostFeatures::Cmd)
        {
            return Err(RustADBError::ADBShellNotSupported);
        }

        self.set_serial_transport()?;

        self.transport
            .send_adb_request(AdbServerCommand::ShellCommand(command.join(" ")))?;

        loop {
            let mut buffer = [0; BUFFER_SIZE];
            match self.transport.get_raw_connection()?.read(&mut buffer) {
                Ok(size) => {
                    if size == 0 {
                        return Ok(());
                    } else {
                        output.write_all(&buffer[..size])?;
                    }
                }
                Err(e) => {
                    return Err(RustADBError::IOError(e));
                }
            }
        }
    }

    fn stat(&mut self, remote_path: &str) -> Result<AdbStatResponse> {
        self.stat(remote_path)
    }

    fn shell(
        &mut self,
        mut reader: Box<dyn Read + Send>,
        mut writer: Box<dyn Write + Send>,
    ) -> Result<()> {
        let supported_features = self.host_features()?;
        if !supported_features.contains(&HostFeatures::ShellV2)
            && !supported_features.contains(&HostFeatures::Cmd)
        {
            return Err(RustADBError::ADBShellNotSupported);
        }

        self.set_serial_transport()?;
        self.transport.send_adb_request(AdbServerCommand::Shell)?;

        let mut read_stream = self.transport.get_raw_connection()?.try_clone()?;

        let mut write_stream = read_stream.try_clone()?;

        // Writing thread, reads from given reader (that could be stdin e.g), and writes content to server socket.
        // Deliberately detached: it may stay blocked reading (e.g on stdin) after the session is over,
        // and will die with the process once this (calling) thread has returned.
        std::thread::spawn(move || {
            match std::io::copy(&mut reader, &mut write_stream) {
                Ok(_) => {
                    // Reader is exhausted (e.g EOF on stdin), close our side of the connection
                    let _ = write_stream.shutdown(std::net::Shutdown::Write);
                }
                Err(e) if e.kind() == ErrorKind::BrokenPipe => (),
                Err(e) => log::error!("Error while writing to ADB server socket: {e}"),
            }
        });

        // Reading loop, reads response from adb-server in the calling thread, so that returning
        // from this function happens as soon as the connection is closed (EOF).
        loop {
            let mut buffer = [0; BUFFER_SIZE];
            match read_stream.read(&mut buffer) {
                Ok(0) => {
                    let _ = read_stream.shutdown(std::net::Shutdown::Both);
                    return Ok(());
                }
                Ok(size) => {
                    writer.write_all(&buffer[..size])?;
                    writer.flush()?;
                }
                Err(e) => {
                    return Err(RustADBError::IOError(e));
                }
            }
        }
    }

    fn pull(&mut self, source: &dyn AsRef<str>, mut output: &mut dyn Write) -> Result<()> {
        self.pull(source, &mut output)
    }

    fn reboot(&mut self, reboot_type: crate::RebootType) -> Result<()> {
        self.reboot(reboot_type)
    }

    fn push(&mut self, stream: &mut dyn Read, path: &dyn AsRef<str>) -> Result<()> {
        self.push(stream, path)
    }

    fn install(&mut self, apk_path: &dyn AsRef<Path>) -> Result<()> {
        self.install(apk_path)
    }

    fn uninstall(&mut self, package: &str) -> Result<()> {
        self.uninstall(package)
    }

    fn framebuffer_inner(&mut self) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
        self.framebuffer_inner()
    }
}
