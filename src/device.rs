use std::io;
use std::time::Duration;

use crate::att::{self, AttPdu};
use crate::clock::{Clock, SystemClock};
use crate::hci::{self, HciEvent};
use crate::heartbeat;
use crate::mac::Mac;
use crate::sampler::Sampler;

use crate::transport::HciTransport;

pub trait BtOps {
    fn discover(&mut self) -> io::Result<usize>;
    fn lookup_name(&mut self, mac: &Mac) -> io::Result<Option<String>>;
    fn distance_approximation(&mut self) -> f64;
    fn local_mac(&mut self) -> io::Result<Mac>;
    fn secure_probe(&mut self, challenge: &[u8]) -> io::Result<Option<Vec<u8>>>;
}

pub struct BleDevice<'s> {
    transport: HciTransport<'s>,
    mac: Mac,
    sampler: Sampler,
    clock: SystemClock,
    handle: Option<u16>,
    connected: bool,
    secure: bool,
    challenge_handle: u16,
    heartbeat_handle: u16,
    challenge_timeout: Duration,
    mtu_exchanged: bool,
}

impl<'s> BleDevice<'s> {
    pub fn new(transport: HciTransport<'s>, mac: Mac, sampler: Sampler) -> Self {
        BleDevice {
            transport,
            mac,
            sampler,
            clock: SystemClock,
            handle: None,
            connected: false,
            secure: false,
            challenge_handle: 0,
            heartbeat_handle: 0,
            challenge_timeout: Duration::from_secs(2),
            mtu_exchanged: false,
        }
    }

    pub fn set_secure(
        &mut self,
        challenge_handle: u16,
        heartbeat_handle: u16,
        challenge_timeout: Duration,
    ) {
        self.secure = true;
        self.challenge_handle = challenge_handle;
        self.heartbeat_handle = heartbeat_handle;
        self.challenge_timeout = challenge_timeout;
    }

    fn reset(&mut self) {
        self.connected = false;
        self.handle = None;
        self.mtu_exchanged = false;
    }

    fn ensure_link(&mut self) -> bool {
        if !self.connected {
            let r = if self.secure {
                self.transport
                    .l2cap_connect_cid(self.mac, hci::L2CAP_CID_ATT)
            } else {
                self.transport.l2cap_connect(self.mac)
            };
            if r.is_err() {
                self.connected = false;
                return false;
            }
            self.connected = true;
            if self.secure && !self.mtu_exchanged {
                match self.att_exchange_mtu() {
                    Ok(mtu) => {
                        if (mtu as usize) < heartbeat::MESSAGE_LEN + 3 {
                            self.reset();
                            return false;
                        }
                        self.mtu_exchanged = true;
                    }
                    Err(_) => {
                        self.reset();
                        return false;
                    }
                }
            }
        }
        if self.handle.is_none() {
            match self.transport.conn_handle(self.mac) {
                Ok(h) => self.handle = Some(h),
                Err(_) => {
                    self.reset();
                    return false;
                }
            }
        }
        true
    }

    fn att_exchange_mtu(&mut self) -> io::Result<u16> {
        self.transport.l2cap_send(
            hci::L2CAP_CID_ATT,
            &att::exchange_mtu_req(att::REQUESTED_MTU),
        )?;
        loop {
            match self.transport.l2cap_recv()? {
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "ATT MTU exchange timed out",
                    ));
                }
                Some((cid, payload)) => {
                    if cid != hci::L2CAP_CID_ATT {
                        continue;
                    }
                    match att::parse_att(&payload) {
                        Some(AttPdu::ExchangeMtuRsp(mtu)) => return Ok(mtu),
                        Some(AttPdu::Error { error_code, .. }) => {
                            return Err(io::Error::new(
                                io::ErrorKind::Other,
                                format!("ATT error during MTU exchange: 0x{error_code:02x}"),
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn query(&mut self, opcode: u16, params: &[u8]) -> Option<Vec<u8>> {
        if !self.ensure_link() {
            return None;
        }
        let handle = self.handle.unwrap_or(0);
        let mut full = Vec::with_capacity(params.len() + 2);
        full.extend_from_slice(&handle.to_le_bytes());
        full.extend_from_slice(params);
        let pkt = hci::command_packet(opcode, &full);
        if self.transport.send(&pkt).is_err() {
            self.reset();
            return None;
        }
        loop {
            match self.transport.next_event() {
                Err(_) => {
                    self.reset();
                    return None;
                }
                Ok(None) => {
                    self.reset();
                    return None;
                }
                Ok(Some(HciEvent::CommandComplete { opcode: op, ret })) => {
                    if op == opcode {
                        return Some(ret);
                    }
                }
                Ok(Some(_)) => {}
            }
        }
    }

    fn host_query(&mut self, opcode: u16, params: &[u8]) -> Option<Vec<u8>> {
        let pkt = hci::command_packet(opcode, params);
        if self.transport.send(&pkt).is_err() {
            return None;
        }
        loop {
            match self.transport.next_event() {
                Err(_) | Ok(None) => return None,
                Ok(Some(HciEvent::CommandComplete { opcode: op, ret })) => {
                    if op == opcode {
                        return Some(ret);
                    }
                }
                Ok(Some(_)) => {}
            }
        }
    }

    pub fn local_mac(&mut self) -> io::Result<Mac> {
        let ret = self
            .host_query(hci::OPCODE_READ_BD_ADDR, &[])
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::Other, "no response to HCI Read BD ADDR")
            })?;
        let bytes = hci::bdaddr_from_return(&ret).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "malformed Read BD ADDR return")
        })?;
        Ok(Mac::from_bytes(bytes))
    }

    pub fn secure_probe(&mut self, challenge: &[u8]) -> io::Result<Option<Vec<u8>>> {
        if !self.ensure_link() {
            return Ok(None);
        }
        self.transport.set_bt_recv_timeout(self.challenge_timeout)?;
        self.transport.l2cap_send(
            hci::L2CAP_CID_ATT,
            &att::write_command(self.challenge_handle, challenge),
        )?;
        loop {
            match self.transport.l2cap_recv()? {
                None => return Ok(None),
                Some((cid, payload)) => {
                    if cid != hci::L2CAP_CID_ATT {
                        continue;
                    }
                    match att::parse_att(&payload) {
                        Some(AttPdu::Notification { handle, value })
                            if handle == self.heartbeat_handle =>
                        {
                            return Ok(Some(value));
                        }
                        Some(AttPdu::Error {
                            req_opcode,
                            handle,
                            error_code,
                        }) => {
                            return Err(io::Error::new(io::ErrorKind::Other, format!(
                                "ATT error (req 0x{req_opcode:02x}, handle 0x{handle:04x}): 0x{error_code:02x}"
                            )));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn rssi(&mut self) -> Option<f64> {
        let ret = self.query(hci::OPCODE_READ_RSSI, &[])?;
        match hci::rssi_from_return(&ret) {
            Some(v) => Some(v as f64),
            None => {
                self.reset();
                None
            }
        }
    }

    pub fn tx_power(&mut self) -> Option<f64> {
        let ret = self.query(hci::OPCODE_READ_TX_POWER, &[hci::TX_POWER_TYPE_CURRENT])?;
        match hci::tx_from_return(&ret) {
            Some(v) => Some(v as f64),
            None => {
                self.reset();
                None
            }
        }
    }

    pub fn discover(&mut self) -> io::Result<usize> {
        let pkt = hci::inquiry_command(hci::GIAC, 8, 0);
        self.transport.send(&pkt)?;
        let mut count = 0usize;
        loop {
            match self.transport.next_event()? {
                None => break,
                Some(HciEvent::InquiryResult { num_responses }) => count += num_responses as usize,
                Some(HciEvent::InquiryComplete) => break,
                Some(_) => {}
            }
        }
        Ok(count)
    }

    pub fn lookup_name(&mut self, mac: &Mac) -> io::Result<Option<String>> {
        let pkt = hci::remote_name_request(*mac);
        self.transport.send(&pkt)?;
        loop {
            match self.transport.next_event()? {
                None => return Ok(None),
                Some(HciEvent::RemoteNameComplete { status, name }) => {
                    return Ok(if status == 0 { Some(name) } else { None })
                }
                Some(_) => {}
            }
        }
    }

    pub fn distance_approximation(&mut self) -> f64 {
        let mut samples: Vec<(f64, f64)> = Vec::new();
        let mut attempts = 0usize;
        while samples.len() < self.sampler.num_samples && attempts < self.sampler.max_attempts {
            attempts += 1;
            let rssi = match self.rssi() {
                Some(r) if r != 0.0 => r,
                _ => continue,
            };
            let tx = self.tx_power();
            let d = self.sampler.record_sample(rssi, tx);
            samples.push((rssi, d));
            self.clock.sleep(self.sampler.sample_sleep);
        }
        self.sampler.finish(&samples)
    }
}

impl BtOps for BleDevice<'_> {
    fn discover(&mut self) -> io::Result<usize> {
        BleDevice::discover(self)
    }

    fn lookup_name(&mut self, mac: &Mac) -> io::Result<Option<String>> {
        BleDevice::lookup_name(self, mac)
    }

    fn distance_approximation(&mut self) -> f64 {
        BleDevice::distance_approximation(self)
    }

    fn local_mac(&mut self) -> io::Result<Mac> {
        BleDevice::local_mac(self)
    }

    fn secure_probe(&mut self, challenge: &[u8]) -> io::Result<Option<Vec<u8>>> {
        BleDevice::secure_probe(self, challenge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::Sys;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, VecDeque};

    enum ReadStep {
        Bytes(Vec<u8>),
        Error(io::ErrorKind),
    }

    struct MockSys {
        next_fd: Cell<i32>,
        reads: RefCell<HashMap<i32, VecDeque<ReadStep>>>,
        writes: RefCell<VecDeque<Result<usize, io::ErrorKind>>>,
        connect_errors: RefCell<VecDeque<io::ErrorKind>>,
        ioctl_error: Cell<bool>,
        setsockopt_error: Cell<bool>,
    }

    impl Default for MockSys {
        fn default() -> Self {
            Self {
                next_fd: Cell::new(10),
                reads: RefCell::new(HashMap::new()),
                writes: RefCell::new(VecDeque::new()),
                connect_errors: RefCell::new(VecDeque::new()),
                ioctl_error: Cell::new(false),
                setsockopt_error: Cell::new(false),
            }
        }
    }

    impl MockSys {
        fn read_step(&self, fd: i32, step: ReadStep) {
            self.reads
                .borrow_mut()
                .entry(fd)
                .or_default()
                .push_back(step);
        }
    }

    impl Sys for MockSys {
        fn socket(&self, _: i32, _: i32, _: i32) -> io::Result<i32> {
            let fd = self.next_fd.get();
            self.next_fd.set(fd + 1);
            Ok(fd)
        }
        fn socketpair(&self, _: i32, _: i32, _: i32, _: &mut [i32; 2]) -> io::Result<()> {
            Ok(())
        }
        fn bind(&self, _: i32, _: &[u8]) -> io::Result<()> {
            Ok(())
        }
        fn listen(&self, _: i32, _: i32) -> io::Result<()> {
            Ok(())
        }
        fn connect(&self, _: i32, _: &[u8]) -> io::Result<()> {
            match self.connect_errors.borrow_mut().pop_front() {
                Some(kind) => Err(io::Error::new(kind, "connect failed")),
                None => Ok(()),
            }
        }
        fn accept(&self, _: i32) -> io::Result<i32> {
            Ok(1)
        }
        fn read(&self, fd: i32, buf: &mut [u8]) -> io::Result<usize> {
            match self
                .reads
                .borrow_mut()
                .entry(fd)
                .or_default()
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
        fn write(&self, _: i32, buf: &[u8]) -> io::Result<usize> {
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
        fn close(&self, _: i32) -> io::Result<()> {
            Ok(())
        }
        fn ioctl(&self, _: i32, _: u64, arg: &mut [u8]) -> io::Result<usize> {
            if self.ioctl_error.get() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "connection missing",
                ));
            }
            arg[8..10].copy_from_slice(&7u16.to_le_bytes());
            Ok(arg.len())
        }
        fn setsockopt(&self, _: i32, _: i32, _: i32, _: &[u8]) -> io::Result<()> {
            if self.setsockopt_error.get() {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "timeout rejected",
                ))
            } else {
                Ok(())
            }
        }
    }

    fn make_device<'a>(sys: &'a MockSys, secure: bool) -> BleDevice<'a> {
        let transport = HciTransport::open(sys).unwrap();
        let mac = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        let mut device = BleDevice::new(transport, mac, Sampler::new(-59.0));
        if secure {
            device.set_secure(0x10, 0x11, Duration::from_secs(1));
        }
        device
    }

    fn event(code: u8, params: &[u8]) -> Vec<u8> {
        let mut bytes = vec![hci::HCI_EVENT_PKT, code, params.len() as u8];
        bytes.extend_from_slice(params);
        bytes
    }

    fn command_complete(opcode: u16, ret: &[u8]) -> Vec<u8> {
        let mut params = vec![1];
        params.extend_from_slice(&opcode.to_le_bytes());
        params.extend_from_slice(ret);
        event(hci::EVT_CMD_COMPLETE, &params)
    }

    fn l2cap_frame(cid: u16, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        frame.extend_from_slice(&cid.to_le_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    fn mtu_response(mtu: u16) -> Vec<u8> {
        vec![att::op::EXCHANGE_MTU_RSP, mtu as u8, (mtu >> 8) as u8]
    }

    fn initialize_secure_link(sys: &MockSys) {
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(hci::L2CAP_CID_ATT, &mtu_response(247))),
        );
    }

    #[test]
    fn insecure_queries_and_host_queries_cover_success_and_failures() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        let mac = device.mac;
        sys.read_step(10, ReadStep::Bytes(vec![0x04, 0x01, 0]));
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_RSSI, &[0, 0, 7, 0xd8])),
        );
        assert_eq!(device.rssi(), Some(-40.0));
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(
                hci::OPCODE_READ_TX_POWER,
                &[0, 0, 7, 0xe2],
            )),
        );
        assert_eq!(device.tx_power(), Some(-30.0));

        sys.ioctl_error.set(true);
        assert!(device.rssi().is_none());
        sys.ioctl_error.set(false);
        let err = device.local_mac().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(device.lookup_name(&mac).unwrap(), None);
    }

    #[test]
    fn host_query_decodes_local_address_and_rejects_malformed_return() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(10, ReadStep::Bytes(event(hci::EVT_INQUIRY_COMPLETE, &[])));
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_RSSI, &[0])),
        );
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(
                hci::OPCODE_READ_BD_ADDR,
                &[0, 1, 2, 3, 4, 5, 6],
            )),
        );
        assert_eq!(
            device.local_mac().unwrap(),
            Mac::from_bytes([1, 2, 3, 4, 5, 6])
        );
        sys.writes
            .borrow_mut()
            .push_back(Err(io::ErrorKind::BrokenPipe));
        assert_eq!(device.local_mac().unwrap_err().kind(), io::ErrorKind::Other);
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_BD_ADDR, &[1])),
        );
        assert_eq!(
            device.local_mac().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        sys.read_step(10, ReadStep::Error(io::ErrorKind::Other));
        assert_eq!(device.local_mac().unwrap_err().kind(), io::ErrorKind::Other);
    }

    #[test]
    fn discovers_and_resolves_names_from_hci_events() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(10, ReadStep::Bytes(event(hci::EVT_INQUIRY_RESULT, &[2])));
        sys.read_step(10, ReadStep::Bytes(event(hci::EVT_INQUIRY_RESULT, &[3])));
        sys.read_step(10, ReadStep::Bytes(vec![0x04, 0x22, 0]));
        sys.read_step(10, ReadStep::Bytes(event(hci::EVT_INQUIRY_COMPLETE, &[])));
        assert_eq!(device.discover().unwrap(), 5);

        let mut name = vec![0];
        name.extend_from_slice(&[0; 6]);
        name.extend_from_slice(b"watch\0");
        sys.read_step(10, ReadStep::Bytes(event(hci::EVT_INQUIRY_COMPLETE, &[])));
        sys.read_step(
            10,
            ReadStep::Bytes(event(hci::EVT_REMOTE_NAME_COMPLETE, &name)),
        );
        let mac = device.mac;
        assert_eq!(device.lookup_name(&mac).unwrap().as_deref(), Some("watch"));
        let mut failed_name = vec![1];
        failed_name.extend_from_slice(&[0; 6]);
        failed_name.extend_from_slice(b"ignored");
        sys.read_step(
            10,
            ReadStep::Bytes(event(hci::EVT_REMOTE_NAME_COMPLETE, &failed_name)),
        );
        let mac = device.mac;
        assert_eq!(device.lookup_name(&mac).unwrap(), None);
    }

    #[test]
    fn bt_ops_trait_methods_delegate_to_device_operations() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        device.sampler.num_samples = 0;
        let mac = device.mac;

        assert_eq!(BtOps::discover(&mut device).unwrap(), 0);
        assert_eq!(BtOps::lookup_name(&mut device, &mac).unwrap(), None);
        assert_eq!(BtOps::distance_approximation(&mut device), 1.0);
        assert!(BtOps::local_mac(&mut device).is_err());

        sys.connect_errors
            .borrow_mut()
            .push_back(io::ErrorKind::ConnectionRefused);
        assert_eq!(BtOps::secure_probe(&mut device, &[1]).unwrap(), None);
    }

    #[test]
    fn handles_link_setup_failures_and_rssi_return_validation() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.connect_errors
            .borrow_mut()
            .push_back(io::ErrorKind::ConnectionRefused);
        assert_eq!(device.rssi(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.ioctl_error.set(true);
        assert_eq!(device.rssi(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_RSSI, &[1, 0, 0, 0])),
        );
        assert_eq!(device.rssi(), None);
    }

    #[test]
    fn secure_transport_propagates_mtu_timeout_and_receive_errors() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        sys.writes
            .borrow_mut()
            .push_back(Err(io::ErrorKind::BrokenPipe));
        assert_eq!(device.secure_probe(&[1]).unwrap(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        sys.read_step(11, ReadStep::Error(io::ErrorKind::Other));
        assert_eq!(device.secure_probe(&[1]).unwrap(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        initialize_secure_link(&sys);
        sys.setsockopt_error.set(true);
        assert_eq!(
            device.secure_probe(&[1]).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );

        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        initialize_secure_link(&sys);
        sys.read_step(11, ReadStep::Error(io::ErrorKind::Other));
        assert_eq!(
            device.secure_probe(&[1]).unwrap_err().kind(),
            io::ErrorKind::Other
        );
    }

    #[test]
    fn secure_link_rejects_small_or_failed_mtu_negotiation() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(hci::L2CAP_CID_ATT, &mtu_response(20))),
        );
        assert_eq!(device.secure_probe(&[1]).unwrap(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(
                hci::L2CAP_CID_ATT,
                &[att::op::ERROR_RSP, 2, 0, 0, 5],
            )),
        );
        assert_eq!(device.secure_probe(&[1]).unwrap(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        assert_eq!(device.secure_probe(&[1]).unwrap(), None);
    }

    #[test]
    fn secure_probe_filters_att_events_and_returns_matching_heartbeat() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        initialize_secure_link(&sys);
        let _ = device.transport.conn_handle(device.mac).unwrap();
        sys.read_step(11, ReadStep::Bytes(l2cap_frame(5, &[0xff])));
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(hci::L2CAP_CID_ATT, &[0x99, 0])),
        );
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(
                hci::L2CAP_CID_ATT,
                &[att::op::HANDLE_VALUE_NOTIF, 0x12, 0, 0x99],
            )),
        );
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(
                hci::L2CAP_CID_ATT,
                &[att::op::HANDLE_VALUE_NOTIF, 0x11, 0, 0x42, 0x43],
            )),
        );
        assert_eq!(
            device.secure_probe(&[0xaa]).unwrap(),
            Some(vec![0x42, 0x43])
        );
    }

    #[test]
    fn secure_probe_reports_att_error_timeout_and_transport_error() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        initialize_secure_link(&sys);
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(
                hci::L2CAP_CID_ATT,
                &[att::op::ERROR_RSP, 0x52, 0x10, 0, 5],
            )),
        );
        let err = device.secure_probe(&[0xaa]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);

        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        initialize_secure_link(&sys);
        assert_eq!(device.secure_probe(&[0xaa]).unwrap(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        initialize_secure_link(&sys);
        sys.writes
            .borrow_mut()
            .extend([Ok(7), Err(io::ErrorKind::BrokenPipe)]);
        assert_eq!(
            device.secure_probe(&[0xaa]).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn inquiry_and_name_operations_propagate_send_errors() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.writes
            .borrow_mut()
            .push_back(Err(io::ErrorKind::BrokenPipe));
        assert_eq!(
            device.discover().unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        sys.writes
            .borrow_mut()
            .push_back(Err(io::ErrorKind::BrokenPipe));
        let mac = device.mac;
        assert_eq!(
            device.lookup_name(&mac).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn distance_sampling_stops_after_attempt_limit_and_uses_tx_power() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        device.sampler.num_samples = 2;
        device.sampler.max_attempts = 3;
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_RSSI, &[0, 0, 7, 0])),
        );
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_RSSI, &[0, 0, 7, 0xd8])),
        );
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(
                hci::OPCODE_READ_TX_POWER,
                &[0, 0, 7, 0xe2],
            )),
        );
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_RSSI, &[0, 0, 7, 0xd0])),
        );
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(
                hci::OPCODE_READ_TX_POWER,
                &[0, 0, 7, 0xe2],
            )),
        );
        assert!(device.distance_approximation().is_finite());
    }

    #[test]
    fn hci_queries_reset_on_write_read_errors_and_ignore_unrelated_events() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.writes
            .borrow_mut()
            .push_back(Err(io::ErrorKind::BrokenPipe));
        assert_eq!(device.rssi(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(10, ReadStep::Bytes(event(hci::EVT_INQUIRY_COMPLETE, &[])));
        sys.read_step(10, ReadStep::Bytes(command_complete(0x1234, &[0])));
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_RSSI, &[0, 0, 7, 0xd8])),
        );
        assert_eq!(device.rssi(), Some(-40.0));

        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(10, ReadStep::Error(io::ErrorKind::Other));
        assert_eq!(device.rssi(), None);
    }

    #[test]
    fn secure_mtu_exchange_skips_unrelated_packets_and_handles_invalid_pdu() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, true);
        sys.read_step(11, ReadStep::Bytes(l2cap_frame(5, &[0xff])));
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(hci::L2CAP_CID_ATT, &[0x99])),
        );
        sys.read_step(
            11,
            ReadStep::Bytes(l2cap_frame(hci::L2CAP_CID_ATT, &mtu_response(247))),
        );
        sys.read_step(10, ReadStep::Error(io::ErrorKind::Other));
        assert_eq!(device.rssi(), None);
    }

    #[test]
    fn tx_power_rejects_invalid_controller_return() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(
            10,
            ReadStep::Bytes(command_complete(hci::OPCODE_READ_TX_POWER, &[1, 0, 0, 0])),
        );
        assert_eq!(device.tx_power(), None);
    }

    #[test]
    fn tx_power_discovery_and_lookup_propagate_controller_read_errors() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.ioctl_error.set(true);
        assert_eq!(device.tx_power(), None);

        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(10, ReadStep::Error(io::ErrorKind::BrokenPipe));
        assert_eq!(
            device.discover().unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );

        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(10, ReadStep::Error(io::ErrorKind::BrokenPipe));
        let mac = device.mac;
        assert_eq!(
            device.lookup_name(&mac).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn inquiry_ends_when_event_stream_times_out() {
        let sys = MockSys::default();
        let mut device = make_device(&sys, false);
        sys.read_step(10, ReadStep::Bytes(event(hci::EVT_INQUIRY_RESULT, &[4])));
        assert_eq!(device.discover().unwrap(), 4);
    }

    #[test]
    fn mock_sys_unused_trait_methods_return_success() {
        let sys = MockSys::default();
        let mut fds = [0; 2];
        sys.socketpair(1, 1, 0, &mut fds).unwrap();
        sys.listen(10, 1).unwrap();
        assert_eq!(sys.accept(10).unwrap(), 1);
    }
}
