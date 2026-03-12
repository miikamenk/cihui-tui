use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use tokio::sync::mpsc;

pub const SINGLE_INSTANCE_PORT: u16 = 8765;
pub const SHUTDOWN_MESSAGE: &str = "SHUTDOWN";

#[derive(Debug)]
pub enum SingleInstanceError {
    AlreadyRunning,
    ToggleFailed(std::io::Error),
    ServerError(std::io::Error),
}

impl std::fmt::Display for SingleInstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SingleInstanceError::AlreadyRunning => {
                write!(f, "Another instance of cihui-tui is already running")
            }
            SingleInstanceError::ToggleFailed(e) => {
                write!(f, "Failed to send toggle signal: {}", e)
            }
            SingleInstanceError::ServerError(e) => {
                write!(f, "Server error: {}", e)
            }
        }
    }
}

impl std::error::Error for SingleInstanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SingleInstanceError::ToggleFailed(e) | SingleInstanceError::ServerError(e) => Some(e),
            _ => None,
        }
    }
}

/// Check if another instance is already running by trying to connect to the TCP port
pub fn is_another_instance_running() -> bool {
    let addr = SocketAddr::new(
        std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        SINGLE_INSTANCE_PORT,
    );
    TcpStream::connect(addr).is_ok()
}

/// Try to bind to the TCP port to claim this instance
/// Returns the TcpListener if successful, or an error if another instance is running
pub fn claim_instance() -> Result<TcpListener, SingleInstanceError> {
    let addr = SocketAddr::new(
        std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        SINGLE_INSTANCE_PORT,
    );

    match TcpListener::bind(addr) {
        Ok(listener) => {
            listener
                .set_nonblocking(true)
                .map_err(|e| SingleInstanceError::ServerError(e))?;
            Ok(listener)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            Err(SingleInstanceError::AlreadyRunning)
        }
        Err(e) => Err(SingleInstanceError::ServerError(e)),
    }
}

/// Send shutdown signal to the running instance
pub fn send_shutdown_signal() -> Result<(), SingleInstanceError> {
    let addr = SocketAddr::new(
        std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        SINGLE_INSTANCE_PORT,
    );

    match TcpStream::connect(addr) {
        Ok(mut stream) => {
            stream
                .write_all(SHUTDOWN_MESSAGE.as_bytes())
                .map_err(|e| SingleInstanceError::ToggleFailed(e))?;
            Ok(())
        }
        Err(e) => Err(SingleInstanceError::ToggleFailed(e)),
    }
}

/// Start the TCP server that listens for shutdown signals
/// Returns a receiver that will receive a message when shutdown is requested
pub fn start_shutdown_server(listener: TcpListener) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel(1);

    std::thread::spawn(move || {
        let mut buffer = [0u8; 128];

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => match stream.read(&mut buffer) {
                    Ok(n) if n > 0 => {
                        let received = String::from_utf8_lossy(&buffer[..n]);
                        if received.trim() == SHUTDOWN_MESSAGE {
                            let _ = tx.try_send(());
                            return;
                        }
                    }
                    _ => continue,
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(_) => return,
            }
        }
    });

    rx
}
