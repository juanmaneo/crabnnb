use std::io;
use std::time::Duration;

use crate::hci;
use crate::mac::Mac;
use crate::sys::{RawFd, Sys};

pub struct HciTransport<'a> {
    sys: &'a dyn Sys,
    hci_fd: RawFd,
    bt_fd: RawFd,
}

impl<'a> HciTransport<'a> {
    pub fn open(sys: &'a dyn Sys) -> io::Result<Self> {
        let fd = sys.socket(
            hci::AF_BLUETOOTH,
            hci::SOCK_RAW | hci::SOCK_CLOEXEC,
            hci::BTPROTO_HCI,
        )?;
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
        let n = self.sys.ioctl(self.hci_fd, hci::HCIGETCONNINFO, &mut req)?;
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

    pub fn l2cap_connect_cid(&mut self, mac: Mac, cid: u16) -> io::Result<()> {
        if self.bt_fd < 0 {
            self.bt_fd = self.sys.socket(
                hci::AF_BLUETOOTH,
                hci::SOCK_RAW | hci::SOCK_CLOEXEC,
                hci::BTPROTO_L2CAP,
            )?;
        }
        self.sys
            .connect(self.bt_fd, &hci::sockaddr_l2cap_cid(mac, cid))
    }

    pub fn set_bt_recv_timeout(&self, d: Duration) -> io::Result<()> {
        self.sys.setsockopt(
            self.bt_fd,
            hci::SOL_SOCKET,
            hci::SO_RCVTIMEO,
            &hci::timeval_bytes(d),
        )
    }

    pub fn l2cap_send(&self, cid: u16, payload: &[u8]) -> io::Result<()> {
        let mut frame = Vec::with_capacity(4 + payload.len());
        frame.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        frame.extend_from_slice(&cid.to_le_bytes());
        frame.extend_from_slice(payload);
        let mut buf: &[u8] = &frame;
        while !buf.is_empty() {
            match self.sys.write(self.bt_fd, buf) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "l2cap write returned 0",
                    ))
                }
                Ok(n) => buf = &buf[n..],
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub fn l2cap_recv(&self) -> io::Result<Option<(u16, Vec<u8>)>> {
        let mut buf = vec![0u8; 4 + 0xffff];
        let mut filled = 0usize;
        loop {
            match self.sys.read(self.bt_fd, &mut buf[filled..]) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(e),
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "l2cap socket closed",
                    ))
                }
                Ok(n) => {
                    filled += n;
                    if filled >= 4 {
                        let len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
                        let total = 4 + len;
                        if filled >= total {
                            let cid = u16::from_le_bytes([buf[2], buf[3]]);
                            return Ok(Some((cid, buf[4..total].to_vec())));
                        }
                    }
                }
            }
        }
    }
}

impl Drop for HciTransport<'_> {
    fn drop(&mut self) {
        let _ = self.sys.close(self.hci_fd);
        if self.bt_fd >= 0 {
            let _ = self.sys.close(self.bt_fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    enum ReadStep {
        Bytes(Vec<u8>),
        Error(io::ErrorKind),
    }

    struct MockSys {
        sockets: RefCell<VecDeque<io::Result<RawFd>>>,
        reads: RefCell<VecDeque<ReadStep>>,
        writes: RefCell<VecDeque<Result<usize, io::ErrorKind>>>,
        bind_error: RefCell<Option<io::ErrorKind>>,
        connect_errors: RefCell<VecDeque<io::ErrorKind>>,
        closed: RefCell<Vec<RawFd>>,
    }

    impl Default for MockSys {
        fn default() -> Self {
            Self {
                sockets: RefCell::new(VecDeque::new()),
                reads: RefCell::new(VecDeque::new()),
                writes: RefCell::new(VecDeque::new()),
                bind_error: RefCell::new(None),
                connect_errors: RefCell::new(VecDeque::new()),
                closed: RefCell::new(Vec::new()),
            }
        }
    }

    impl Sys for MockSys {
        fn socket(&self, _: i32, _: i32, _: i32) -> io::Result<RawFd> {
            self.sockets.borrow_mut().pop_front().unwrap_or(Ok(10))
        }
        fn socketpair(&self, _: i32, _: i32, _: i32, _: &mut [RawFd; 2]) -> io::Result<()> {
            Ok(())
        }
        fn bind(&self, _: RawFd, _: &[u8]) -> io::Result<()> {
            match self.bind_error.borrow_mut().take() {
                Some(kind) => Err(io::Error::new(kind, "bind failed")),
                None => Ok(()),
            }
        }
        fn listen(&self, _: RawFd, _: i32) -> io::Result<()> {
            Ok(())
        }
        fn connect(&self, _: RawFd, _: &[u8]) -> io::Result<()> {
            match self.connect_errors.borrow_mut().pop_front() {
                Some(kind) => Err(io::Error::new(kind, "connect failed")),
                None => Ok(()),
            }
        }
        fn accept(&self, _: RawFd) -> io::Result<RawFd> {
            Ok(11)
        }
        fn read(&self, _: RawFd, buf: &mut [u8]) -> io::Result<usize> {
            match self
                .reads
                .borrow_mut()
                .pop_front()
                .unwrap_or(ReadStep::Error(io::ErrorKind::WouldBlock))
            {
                ReadStep::Bytes(bytes) => {
                    let n = bytes.len().min(buf.len());
                    buf[..n].copy_from_slice(&bytes[..n]);
                    Ok(n)
                }
                ReadStep::Error(kind) => Err(io::Error::new(kind, "read failed")),
            }
        }
        fn write(&self, _: RawFd, buf: &[u8]) -> io::Result<usize> {
            match self
                .writes
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(buf.len()))
            {
                Ok(n) => Ok(n),
                Err(kind) => Err(io::Error::new(kind, "write failed")),
            }
        }
        fn close(&self, fd: RawFd) -> io::Result<()> {
            self.closed.borrow_mut().push(fd);
            Ok(())
        }
        fn ioctl(&self, _: RawFd, _: u64, arg: &mut [u8]) -> io::Result<usize> {
            if arg.len() >= 10 {
                arg[8..10].copy_from_slice(&0x1234u16.to_le_bytes());
            }
            Ok(arg.len())
        }
        fn setsockopt(&self, _: RawFd, _: i32, _: i32, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }

    fn event_command_complete(opcode: u16, ret: &[u8]) -> Vec<u8> {
        let mut event = vec![
            hci::HCI_EVENT_PKT,
            hci::EVT_CMD_COMPLETE,
            (3 + ret.len()) as u8,
            1,
        ];
        event.extend_from_slice(&opcode.to_le_bytes());
        event.extend_from_slice(ret);
        event
    }

    #[test]
    fn mock_sys_unused_methods_and_short_ioctl_return_are_exercised() {
        let sys = MockSys::default();
        let mut fds = [-1; 2];
        sys.socketpair(1, 1, 0, &mut fds).unwrap();
        sys.listen(10, 1).unwrap();
        assert_eq!(sys.accept(10).unwrap(), 11);
        let mut short = [0u8; 9];
        sys.ioctl(10, 0, &mut short).unwrap();
        assert_eq!(short, [0; 9]);
    }

    #[test]
    fn open_closes_socket_when_bind_fails() {
        let sys = MockSys::default();
        *sys.bind_error.borrow_mut() = Some(io::ErrorKind::PermissionDenied);
        let err = HciTransport::open(&sys).err().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(&*sys.closed.borrow(), &[10]);

        sys.sockets
            .borrow_mut()
            .push_back(Err(io::Error::new(io::ErrorKind::NotFound, "socket")));
        assert_eq!(
            HciTransport::open(&sys).err().unwrap().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn l2cap_socket_creation_errors_are_propagated() {
        let sys = MockSys::default();
        let mut transport = HciTransport::open(&sys).unwrap();
        let mac = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();

        sys.sockets
            .borrow_mut()
            .push_back(Err(io::Error::new(io::ErrorKind::NotFound, "psm socket")));
        assert_eq!(
            transport.l2cap_connect(mac).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );

        sys.sockets.borrow_mut().push_back(Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cid socket",
        )));
        assert_eq!(
            transport.l2cap_connect_cid(mac, 0x0040).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn hci_send_handles_partial_interrupted_zero_and_failure_writes() {
        let sys = MockSys::default();
        let transport = HciTransport::open(&sys).unwrap();
        sys.writes
            .borrow_mut()
            .extend([Ok(1), Err(io::ErrorKind::Interrupted), Ok(2)]);
        transport.send(&[1, 2, 3]).unwrap();
        sys.writes.borrow_mut().push_back(Ok(0));
        assert_eq!(
            transport.send(&[1]).unwrap_err().kind(),
            io::ErrorKind::WriteZero
        );
        sys.writes
            .borrow_mut()
            .push_back(Err(io::ErrorKind::BrokenPipe));
        assert_eq!(
            transport.send(&[1]).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn hci_read_filters_bad_events_and_handles_all_read_outcomes() {
        let sys = MockSys::default();
        let transport = HciTransport::open(&sys).unwrap();
        sys.reads.borrow_mut().extend([
            ReadStep::Error(io::ErrorKind::Interrupted),
            ReadStep::Bytes(vec![1, 2, 3]),
            ReadStep::Bytes(event_command_complete(0x1234, &[0])),
        ]);
        assert!(matches!(
            transport.next_event().unwrap(),
            Some(hci::HciEvent::CommandComplete { opcode: 0x1234, .. })
        ));
        sys.reads
            .borrow_mut()
            .push_back(ReadStep::Error(io::ErrorKind::WouldBlock));
        assert_eq!(transport.next_event().unwrap(), None);
        sys.reads
            .borrow_mut()
            .push_back(ReadStep::Error(io::ErrorKind::Other));
        assert_eq!(
            transport.next_event().unwrap_err().kind(),
            io::ErrorKind::Other
        );
        sys.reads.borrow_mut().push_back(ReadStep::Bytes(vec![]));
        assert_eq!(
            transport.next_event().unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn l2cap_connections_timeouts_and_connection_info() {
        let sys = MockSys::default();
        let mut transport = HciTransport::open(&sys).unwrap();
        let mac = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        transport
            .set_recv_timeout(Duration::from_millis(500))
            .unwrap();
        assert_eq!(transport.conn_handle(mac).unwrap(), 0x1234);
        transport.l2cap_connect(mac).unwrap();
        transport.l2cap_connect(mac).unwrap();
        transport
            .l2cap_connect_cid(mac, hci::L2CAP_CID_ATT)
            .unwrap();
        transport
            .l2cap_connect_cid(mac, hci::L2CAP_CID_ATT)
            .unwrap();
        sys.connect_errors
            .borrow_mut()
            .push_back(io::ErrorKind::ConnectionRefused);
        assert_eq!(
            transport.l2cap_connect(mac).unwrap_err().kind(),
            io::ErrorKind::ConnectionRefused
        );
        sys.connect_errors
            .borrow_mut()
            .push_back(io::ErrorKind::TimedOut);
        assert_eq!(
            transport
                .l2cap_connect_cid(mac, hci::L2CAP_CID_ATT)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        transport
            .set_bt_recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(sys.sockets.borrow().len(), 0);
    }

    #[test]
    fn l2cap_send_and_receive_handles_partial_frames_and_errors() {
        let sys = MockSys::default();
        let mut transport = HciTransport::open(&sys).unwrap();
        transport
            .l2cap_connect(Mac::parse("AA:BB:CC:DD:EE:FF").unwrap())
            .unwrap();
        sys.writes
            .borrow_mut()
            .extend([Ok(2), Err(io::ErrorKind::Interrupted), Ok(3)]);
        transport.l2cap_send(4, &[7]).unwrap();
        sys.writes.borrow_mut().push_back(Ok(0));
        assert_eq!(
            transport.l2cap_send(4, &[7]).unwrap_err().kind(),
            io::ErrorKind::WriteZero
        );
        sys.writes
            .borrow_mut()
            .push_back(Err(io::ErrorKind::BrokenPipe));
        assert_eq!(
            transport.l2cap_send(4, &[7]).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );

        sys.reads.borrow_mut().extend([
            ReadStep::Error(io::ErrorKind::Interrupted),
            ReadStep::Bytes(vec![3, 0]),
            ReadStep::Bytes(vec![4, 0, 9]),
            ReadStep::Bytes(vec![8, 7]),
        ]);
        assert_eq!(transport.l2cap_recv().unwrap(), Some((4, vec![9, 8, 7])));
        sys.reads
            .borrow_mut()
            .push_back(ReadStep::Error(io::ErrorKind::WouldBlock));
        assert_eq!(transport.l2cap_recv().unwrap(), None);
        sys.reads
            .borrow_mut()
            .push_back(ReadStep::Error(io::ErrorKind::Other));
        assert_eq!(
            transport.l2cap_recv().unwrap_err().kind(),
            io::ErrorKind::Other
        );
        sys.reads.borrow_mut().push_back(ReadStep::Bytes(vec![]));
        assert_eq!(
            transport.l2cap_recv().unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn drop_closes_hci_and_bluetooth_fds() {
        let sys = MockSys::default();
        {
            let mut transport = HciTransport::open(&sys).unwrap();
            transport
                .l2cap_connect(Mac::parse("AA:BB:CC:DD:EE:FF").unwrap())
                .unwrap();
        }
        assert_eq!(&*sys.closed.borrow(), &[10, 10]);
    }
}
