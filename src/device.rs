use std::io;
use std::time::Duration;

use crate::att::{self, AttPdu};
use crate::clock::{Clock, SystemClock};
use crate::hci::{self, HciEvent};
use crate::heartbeat;
use crate::mac::Mac;
use crate::sampler::Sampler;
use crate::sys::Sys;
use crate::transport::HciTransport;

pub trait BtOps {
    fn discover(&mut self) -> io::Result<usize>;
    fn lookup_name(&mut self, mac: &Mac) -> io::Result<Option<String>>;
    fn distance_approximation(&mut self) -> f64;
    fn local_mac(&mut self) -> io::Result<Mac>;
    fn secure_probe(&mut self, challenge: &[u8]) -> io::Result<Option<Vec<u8>>>;
}

pub struct BleDevice<'s, S: Sys> {
    transport: HciTransport<'s, S>,
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

impl<'s, S: Sys> BleDevice<'s, S> {
    pub fn new(transport: HciTransport<'s, S>, mac: Mac, sampler: Sampler) -> Self {
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
                            return Err(io::Error::other(format!(
                                "ATT error during MTU exchange: 0x{error_code:02x}"
                            )));
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
            .ok_or_else(|| io::Error::other("no response to HCI Read BD ADDR"))?;
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
                            return Err(io::Error::other(format!(
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

impl<'s, S: Sys> BtOps for BleDevice<'s, S> {
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
