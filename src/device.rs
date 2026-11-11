use std::io;

use crate::clock::{Clock, SystemClock};
use crate::hci::{self, HciEvent};
use crate::mac::Mac;
use crate::sampler::Sampler;
use crate::sys::Sys;
use crate::transport::HciTransport;

pub trait BtOps {
    fn discover(&mut self) -> io::Result<usize>;
    fn lookup_name(&mut self, mac: &Mac) -> io::Result<Option<String>>;
    fn distance_approximation(&mut self) -> f64;
}

pub struct BleDevice<'s, S: Sys> {
    transport: HciTransport<'s, S>,
    mac: Mac,
    sampler: Sampler,
    clock: SystemClock,
    handle: Option<u16>,
    connected: bool,
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
        }
    }

    fn reset(&mut self) {
        self.connected = false;
        self.handle = None;
    }

    fn ensure_link(&mut self) -> bool {
        if !self.connected {
            if self.transport.l2cap_connect(self.mac).is_err() {
                self.connected = false;
                return false;
            }
            self.connected = true;
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
                Some(HciEvent::InquiryResult { num_responses }) => {
                    count += num_responses as usize
                }
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
}
