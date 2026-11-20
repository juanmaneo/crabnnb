use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::crypto;
use crate::device::BtOps;
use crate::heartbeat::{BRIDGE_MESSAGE_LEN, CHALLENGE_LEN};
use crate::mac::Mac;

pub struct BridgeDevice {
    stream: Option<TcpStream>,
    addr: String,
    timeout: Duration,
    local: Mac,
}

fn connect_stream(addr: &str, timeout: Duration) -> io::Result<TcpStream> {
    let mut last_err: Option<io::Error> = None;
    let mut stream = None;
    for sa in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&sa, timeout) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let stream = stream.ok_or_else(|| {
        last_err
            .unwrap_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unresolvable address"))
    })?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(stream)
}

fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bridge frame too large"))?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(payload)
}

fn read_frame(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut hdr = [0u8; 4];
    stream.read_exact(&mut hdr)?;
    let len = u32::from_le_bytes(hdr) as usize;
    if len != BRIDGE_MESSAGE_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {BRIDGE_MESSAGE_LEN}-byte heartbeat frame, got {len}"),
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

impl BridgeDevice {
    pub fn connect(addr: &str, timeout: Duration) -> io::Result<Self> {
        let stream = connect_stream(addr, timeout)?;
        let mut id = Vec::with_capacity(22 + addr.len());
        id.extend_from_slice(b"crabnnb-bridge-local:");
        id.extend_from_slice(addr.as_bytes());
        let hash = crypto::sha256(&id);
        Ok(BridgeDevice {
            stream: Some(stream),
            addr: addr.to_string(),
            timeout,
            local: Mac::from_bytes([hash[0], hash[1], hash[2], hash[3], hash[4], hash[5]]),
        })
    }

    fn ensure_connected(&mut self) -> io::Result<&mut TcpStream> {
        if self.stream.is_none() {
            self.stream = Some(connect_stream(&self.addr, self.timeout)?);
        }
        Ok(self.stream.as_mut().expect("stream connected above"))
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
        if challenge.len() != CHALLENGE_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "expected {CHALLENGE_LEN}-byte challenge, got {}",
                    challenge.len()
                ),
            ));
        }
        let result = {
            let stream = self.ensure_connected()?;
            write_frame(stream, challenge).and_then(|()| read_frame(stream))
        };
        match result {
            Ok(response) => Ok(Some(response)),
            Err(e) => {
                self.stream = None;
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn read_challenge(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header)?;
        let len = u32::from_le_bytes(header) as usize;
        if len != CHALLENGE_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad challenge length",
            ));
        }
        let mut payload = vec![0; len];
        stream.read_exact(&mut payload)?;
        Ok(payload)
    }

    #[test]
    fn bridge_exchange_uses_little_endian_framing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let challenge = [0x5au8; CHALLENGE_LEN];
        let expected_challenge = challenge;
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            assert_eq!(read_challenge(&mut stream).unwrap(), expected_challenge);
            write_frame(&mut stream, &[0x42; BRIDGE_MESSAGE_LEN]).unwrap();
        });

        let mut device = BridgeDevice::connect(&addr.to_string(), Duration::from_secs(1)).unwrap();
        let response = device.secure_probe(&challenge).unwrap().unwrap();
        assert_eq!(response, vec![0x42; BRIDGE_MESSAGE_LEN]);
        server.join().unwrap();
    }

    #[test]
    fn failed_partial_frame_discards_stream_and_reconnects_next_probe() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            read_challenge(&mut first).unwrap();
            first
                .write_all(&(BRIDGE_MESSAGE_LEN as u32).to_le_bytes())
                .unwrap();
            first.write_all(&[0x01]).unwrap();
            thread::sleep(Duration::from_millis(100));
            drop(first);

            let (mut second, _) = listener.accept().unwrap();
            read_challenge(&mut second).unwrap();
            write_frame(&mut second, &[0x24; BRIDGE_MESSAGE_LEN]).unwrap();
        });

        let mut device =
            BridgeDevice::connect(&addr.to_string(), Duration::from_millis(25)).unwrap();
        let challenge = [0x11u8; CHALLENGE_LEN];
        assert!(device.secure_probe(&challenge).is_err());
        assert!(device.stream.is_none());
        device.timeout = Duration::from_secs(1);
        let response = device.secure_probe(&challenge).unwrap().unwrap();
        assert_eq!(response, vec![0x24; BRIDGE_MESSAGE_LEN]);
        server.join().unwrap();
    }

    #[test]
    fn bridge_rejects_unexpected_challenge_size() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || listener.accept().unwrap());
        let mut device = BridgeDevice::connect(&addr.to_string(), Duration::from_secs(1)).unwrap();
        let err = device.secure_probe(&[0x11]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        server.join().unwrap();
    }
}
