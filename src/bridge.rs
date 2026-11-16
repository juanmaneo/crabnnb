use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::crypto;
use crate::device::BtOps;
use crate::mac::Mac;

pub struct BridgeDevice {
    stream: TcpStream,
    local: Mac,
}

impl BridgeDevice {
    pub fn connect(addr: &str, timeout: Duration) -> io::Result<Self> {
        let mut last_err: Option<io::Error> = None;
        let mut stream = None;
        for sa in addr.to_socket_addrs()? {
            match TcpStream::connect(sa) {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let mut stream = stream.ok_or_else(|| {
            last_err.unwrap_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unresolvable address"))
        })?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let mut id = Vec::with_capacity(22 + addr.len());
        id.extend_from_slice(b"crabnnb-bridge-local:");
        id.extend_from_slice(addr.as_bytes());
        let hash = crypto::sha256(&id);
        Ok(BridgeDevice {
            stream,
            local: Mac::from_bytes([hash[0], hash[1], hash[2], hash[3], hash[4], hash[5]]),
        })
    }

    fn write_frame(&mut self, payload: &[u8]) -> io::Result<()> {
        self.stream.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.stream.write_all(payload)
    }

    fn read_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut hdr = [0u8; 4];
        match self.stream.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                return Ok(None)
            }
            Err(e) => return Err(e),
        }
        let len = u32::from_le_bytes(hdr) as usize;
        if len > 4096 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bridge frame too large"));
        }
        let mut buf = vec![0u8; len];
        match self.stream.read_exact(&mut buf) {
            Ok(()) => Ok(Some(buf)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

impl BtOps for BridgeDevice {
    fn discover(&mut self) -> io::Result<usize> {
        Ok(1)
    }

    fn lookup_name(&mut self, _mac: &Mac) -> io::Result<Option<String>> {
        Ok(Some("phone-bridge".to_string()))
    }

    fn distance_approximation(&mut self) -> f64 {
        0.0
    }

    fn local_mac(&mut self) -> io::Result<Mac> {
        Ok(self.local)
    }

    fn secure_probe(&mut self, challenge: &[u8]) -> io::Result<Option<Vec<u8>>> {
        self.write_frame(challenge)?;
        self.read_frame()
    }
}
