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

fn connect_error(last_err: Option<io::Error>) -> io::Error {
    last_err.unwrap_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unresolvable address"))
}

fn frame_len(payload_len: u64) -> io::Result<u32> {
    u32::try_from(payload_len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bridge frame too large"))
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
    let stream = stream.ok_or_else(|| connect_error(last_err))?;
    finish_connected_stream(stream, timeout, set_stream_timeouts)
}

fn finish_connected_stream(
    stream: TcpStream,
    timeout: Duration,
    set_timeouts: fn(&TcpStream, Duration, Duration) -> io::Result<()>,
) -> io::Result<TcpStream> {
    set_timeouts(&stream, timeout, timeout)?;
    Ok(stream)
}

fn set_stream_timeouts(
    stream: &TcpStream,
    read_timeout: Duration,
    write_timeout: Duration,
) -> io::Result<()> {
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(write_timeout))
}

fn write_frame_header(stream: &mut dyn Write, payload_len: u64) -> io::Result<()> {
    let len = frame_len(payload_len)?;
    stream.write_all(&len.to_le_bytes())
}

fn write_frame(stream: &mut dyn Write, payload: &[u8]) -> io::Result<()> {
    write_frame_header(stream, payload.len() as u64)?;
    stream.write_all(payload)
}

fn read_frame(stream: &mut dyn Read) -> io::Result<Vec<u8>> {
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

    fn read_challenge(stream: &mut dyn Read) -> io::Result<Vec<u8>> {
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
    fn rejects_empty_resolution_and_oversized_frames() {
        assert_eq!(connect_error(None).kind(), io::ErrorKind::InvalidInput);
        assert_eq!(connect_error(None).to_string(), "unresolvable address");
        let err = connect_error(Some(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            "refused",
        )));
        assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused);

        assert_eq!(frame_len(42).unwrap(), 42);
        let err = frame_len(u64::from(u32::MAX) + 1).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(err.to_string(), "bridge frame too large");
        let mut writer = Vec::new();
        assert_eq!(
            write_frame_header(&mut writer, u64::from(u32::MAX) + 1)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn frame_readers_and_writers_report_truncated_or_failed_io() {
        assert_eq!(
            read_frame(&mut std::io::Cursor::new(Vec::<u8>::new()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        let mut partial_payload = (BRIDGE_MESSAGE_LEN as u32).to_le_bytes().to_vec();
        partial_payload.push(1);
        assert_eq!(
            read_frame(&mut std::io::Cursor::new(partial_payload))
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        assert_eq!(
            read_challenge(&mut std::io::Cursor::new(Vec::<u8>::new()))
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
        let mut partial_challenge = (CHALLENGE_LEN as u32).to_le_bytes().to_vec();
        partial_challenge.push(1);
        assert_eq!(
            read_challenge(&mut std::io::Cursor::new(partial_challenge))
                .unwrap_err()
                .kind(),
            io::ErrorKind::UnexpectedEof
        );

        struct FailAfter(usize);
        impl Write for FailAfter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.0 == 0 {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
                }
                let count = bytes.len().min(self.0);
                self.0 -= count;
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = FailAfter(0);
        writer.flush().unwrap();
        assert_eq!(
            write_frame(&mut writer, &[1]).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            write_frame(&mut FailAfter(4), &[1]).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn invalid_socket_timeouts_and_unresolvable_address_are_reported() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || listener.accept().unwrap());
        let stream = TcpStream::connect(addr).unwrap();
        let _accepted = server.join().unwrap();

        assert_eq!(
            set_stream_timeouts(&stream, Duration::ZERO, Duration::from_secs(1))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            set_stream_timeouts(&stream, Duration::from_secs(1), Duration::ZERO)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        fn reject_timeouts(_: &TcpStream, _: Duration, _: Duration) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "timeout rejected",
            ))
        }
        assert_eq!(
            finish_connected_stream(stream, Duration::from_secs(1), reject_timeouts)
                .unwrap_err()
                .to_string(),
            "timeout rejected"
        );
        assert!(BridgeDevice::connect("[::1", Duration::from_secs(1)).is_err());
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
        let reconnect_addr = device.addr.clone();
        device.addr = "[::1".to_string();
        assert!(device.secure_probe(&challenge).is_err());
        device.addr = reconnect_addr;
        device.timeout = Duration::from_secs(1);
        let response = device.secure_probe(&challenge).unwrap().unwrap();
        assert_eq!(response, vec![0x24; BRIDGE_MESSAGE_LEN]);
        server.join().unwrap();
    }

    #[test]
    fn bridge_rejects_malformed_response_and_connection_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_challenge(&mut stream).unwrap();
            stream.write_all(&1u32.to_le_bytes()).unwrap();
        });
        let mut device = BridgeDevice::connect(&addr.to_string(), Duration::from_secs(1)).unwrap();
        let err = device.secure_probe(&[0x11; CHALLENGE_LEN]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(device.stream.is_none());
        server.join().unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        assert!(BridgeDevice::connect(&addr.to_string(), Duration::from_secs(1)).is_err());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(&1u32.to_le_bytes()).unwrap();
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        assert_eq!(
            read_challenge(&mut stream).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
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
