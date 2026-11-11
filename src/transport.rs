use std::io;
use std::time::Duration;

use crate::hci;
use crate::mac::Mac;
use crate::sys::{RawFd, Sys};

pub struct HciTransport<'a, S: Sys> {
    sys: &'a S,
    hci_fd: RawFd,
    bt_fd: RawFd,
}

impl<'a, S: Sys> HciTransport<'a, S> {
    pub fn open(sys: &'a S) -> io::Result<Self> {
        let fd = sys.socket(hci::AF_BLUETOOTH, hci::SOCK_RAW | hci::SOCK_CLOEXEC, hci::BTPROTO_HCI)?;
        if let Err(e) = sys.bind(fd, &hci::sockaddr_hci_raw()) {
            let _ = sys.close(fd);
            return Err(e);
        }
        Ok(HciTransport {
            sys,
            hci_fd: fd,
            bt_fd: -1,
        })
    }

    pub fn set_recv_timeout(&self, d: Duration) -> io::Result<()> {
        self.sys.setsockopt(
            self.hci_fd,
            hci::SOL_SOCKET,
            hci::SO_RCVTIMEO,
            &hci::timeval_bytes(d),
        )
    }

    pub fn send(&self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            match self.sys.write(self.hci_fd, buf) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "hci write returned 0",
                    ))
                }
                Ok(n) => buf = &buf[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub fn next_event(&self) -> io::Result<Option<hci::HciEvent>> {
        let mut buf = [0u8; 512];
        loop {
            match self.sys.read(self.hci_fd, &mut buf) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(e),
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "hci socket closed",
                    ))
                }
                Ok(n) => {
                    if let Some(ev) = hci::parse_event(&buf[..n]) {
                        return Ok(Some(ev));
                    }
                }
            }
        }
    }

    pub fn conn_handle(&mut self, mac: Mac) -> io::Result<u16> {
        let mut req = hci::conn_info_request(mac);
        let n = self
            .sys
            .ioctl(self.hci_fd, hci::HCIGETCONNINFO, &mut req)?;
        hci::parse_conn_handle(&req[..n])
    }

    pub fn l2cap_connect(&mut self, mac: Mac) -> io::Result<()> {
        if self.bt_fd < 0 {
            self.bt_fd = self.sys.socket(
                hci::AF_BLUETOOTH,
                hci::SOCK_RAW | hci::SOCK_CLOEXEC,
                hci::BTPROTO_L2CAP,
            )?;
        }
        self.sys
            .connect(self.bt_fd, &hci::sockaddr_l2cap(mac, hci::L2CAP_PSM))
    }
}

impl<'a, S: Sys> Drop for HciTransport<'a, S> {
    fn drop(&mut self) {
        let _ = self.sys.close(self.hci_fd);
        if self.bt_fd >= 0 {
            let _ = self.sys.close(self.bt_fd);
        }
    }
}
