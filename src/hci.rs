use std::io;
use std::time::Duration;

use crate::mac::Mac;

pub const AF_BLUETOOTH: i32 = 31;
pub const BTPROTO_HCI: i32 = 1;
pub const BTPROTO_L2CAP: i32 = 0;
pub const SOCK_RAW: i32 = 3;
pub const SOCK_CLOEXEC: i32 = 0x80000;
pub const SOL_SOCKET: i32 = 1;
pub const SO_RCVTIMEO: i32 = 20;

pub const HCI_CHANNEL_RAW: u16 = 3;
pub const HCI_DEV_NONE: u16 = 0xffff;

pub const HCI_COMMAND_PKT: u8 = 0x01;
pub const HCI_EVENT_PKT: u8 = 0x04;

pub const EVT_INQUIRY_COMPLETE: u8 = 0x01;
pub const EVT_INQUIRY_RESULT: u8 = 0x02;
pub const EVT_REMOTE_NAME_COMPLETE: u8 = 0x07;
pub const EVT_CMD_COMPLETE: u8 = 0x0e;

pub const OGF_LINK_CTL: u16 = 0x01;
pub const OGF_HOST_CTL: u16 = 0x03;
pub const OGF_STATUS_PARAM: u16 = 0x05;

pub const OCF_INQUIRY: u16 = 0x0001;
pub const OCF_REMOTE_NAME_REQ: u16 = 0x0019;
pub const OCF_READ_RSSI: u16 = 0x0005;
pub const OCF_READ_TRANSMIT_POWER_LEVEL: u16 = 0x002d;

pub const OPCODE_INQUIRY: u16 = (OGF_LINK_CTL << 10) | OCF_INQUIRY;
pub const OPCODE_REMOTE_NAME_REQ: u16 = (OGF_LINK_CTL << 10) | OCF_REMOTE_NAME_REQ;
pub const OPCODE_READ_RSSI: u16 = (OGF_STATUS_PARAM << 10) | OCF_READ_RSSI;
pub const OPCODE_READ_TX_POWER: u16 = (OGF_HOST_CTL << 10) | OCF_READ_TRANSMIT_POWER_LEVEL;

pub const HCIGETCONNINFO: u64 = 0x801848d3;

pub const ACL_LINK: u8 = 1;
pub const L2CAP_PSM: u16 = 1;
pub const GIAC: [u8; 3] = [0x33, 0x8b, 0x9e];
pub const TX_POWER_TYPE_CURRENT: u8 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HciEvent {
    CommandComplete { opcode: u16, ret: Vec<u8> },
    InquiryResult { num_responses: u8 },
    InquiryComplete,
    RemoteNameComplete { status: u8, name: String },
    Other { code: u8 },
}

pub fn command_packet(opcode: u16, params: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(4 + params.len());
    v.push(HCI_COMMAND_PKT);
    v.extend_from_slice(&opcode.to_le_bytes());
    v.push(params.len() as u8);
    v.extend_from_slice(params);
    v
}

pub fn inquiry_command(giac: [u8; 3], inquiry_len: u8, num_responses: u8) -> Vec<u8> {
    let mut params = Vec::with_capacity(5);
    params.extend_from_slice(&giac);
    params.push(inquiry_len);
    params.push(num_responses);
    command_packet(OPCODE_INQUIRY, &params)
}

pub fn remote_name_request(mac: Mac) -> Vec<u8> {
    let mut params = Vec::with_capacity(10);
    params.extend_from_slice(&mac.bytes());
    params.push(0x02);
    params.push(0x00);
    params.extend_from_slice(&0x0000u16.to_le_bytes());
    command_packet(OPCODE_REMOTE_NAME_REQ, &params)
}

pub fn sockaddr_hci_raw() -> [u8; 8] {
    let mut a = [0u8; 8];
    a[0..2].copy_from_slice(&(AF_BLUETOOTH as u16).to_le_bytes());
    a[2..4].copy_from_slice(&HCI_DEV_NONE.to_le_bytes());
    a[4..6].copy_from_slice(&HCI_CHANNEL_RAW.to_le_bytes());
    a
}

pub fn sockaddr_l2cap(mac: Mac, psm: u16) -> [u8; 14] {
    let mut a = [0u8; 14];
    a[0..2].copy_from_slice(&(AF_BLUETOOTH as u16).to_le_bytes());
    a[2..4].copy_from_slice(&psm.to_le_bytes());
    a[4..10].copy_from_slice(&mac.bytes());
    a
}

pub fn conn_info_request(mac: Mac) -> [u8; 24] {
    let mut req = [0u8; 24];
    req[0..6].copy_from_slice(&mac.bytes());
    req[6] = ACL_LINK;
    req
}

pub fn parse_conn_handle(buf: &[u8]) -> io::Result<u16> {
    if buf.len() < 10 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "short HCIGETCONNINFO response",
        ));
    }
    Ok(u16::from_le_bytes([buf[8], buf[9]]))
}

pub fn timeval_bytes(d: Duration) -> [u8; 16] {
    let mut tv = [0u8; 16];
    let sec = d.as_secs() as i64;
    let usec = d.subsec_micros() as i64;
    tv[0..8].copy_from_slice(&sec.to_le_bytes());
    tv[8..16].copy_from_slice(&usec.to_le_bytes());
    tv
}

pub fn parse_event(buf: &[u8]) -> Option<HciEvent> {
    if buf.len() < 3 || buf[0] != HCI_EVENT_PKT {
        return None;
    }
    let code = buf[1];
    let plen = buf[2] as usize;
    if buf.len() < 3 + plen {
        return None;
    }
    let params = &buf[3..3 + plen];
    match code {
        EVT_CMD_COMPLETE if plen >= 3 => Some(HciEvent::CommandComplete {
            opcode: u16::from_le_bytes([params[1], params[2]]),
            ret: params[3..].to_vec(),
        }),
        EVT_INQUIRY_RESULT if plen >= 1 => Some(HciEvent::InquiryResult {
            num_responses: params[0],
        }),
        EVT_INQUIRY_COMPLETE => Some(HciEvent::InquiryComplete),
        EVT_REMOTE_NAME_COMPLETE if plen >= 7 => {
            let raw = &params[7..];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            Some(HciEvent::RemoteNameComplete {
                status: params[0],
                name: String::from_utf8_lossy(&raw[..end]).into_owned(),
            })
        }
        _ => Some(HciEvent::Other { code }),
    }
}

pub fn rssi_from_return(ret: &[u8]) -> Option<i8> {
    if ret.len() < 4 || ret[0] != 0 {
        return None;
    }
    Some(ret[3] as i8)
}

pub fn tx_from_return(ret: &[u8]) -> Option<i8> {
    if ret.len() < 4 || ret[0] != 0 {
        return None;
    }
    Some(ret[3] as i8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mac::Mac;

    #[test]
    fn test_command_packet() {
        let pkt = command_packet(0x1234, &[0xAA, 0xBB]);
        assert_eq!(pkt, vec![HCI_COMMAND_PKT, 0x34, 0x12, 2, 0xAA, 0xBB]);
    }

    #[test]
    fn test_command_packet_empty_params() {
        let pkt = command_packet(0x0000, &[]);
        assert_eq!(pkt, vec![HCI_COMMAND_PKT, 0x00, 0x00, 0]);
    }

    #[test]
    fn test_inquiry_command() {
        let pkt = inquiry_command(GIAC, 8, 0);
        assert_eq!(pkt[0], HCI_COMMAND_PKT);
        assert_eq!(pkt[1..3], OPCODE_INQUIRY.to_le_bytes());
        assert_eq!(pkt[3], 5); // param len
        assert_eq!(&pkt[4..7], &GIAC);
    }

    #[test]
    fn test_remote_name_request() {
        let mac = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        let pkt = remote_name_request(mac);
        assert_eq!(pkt[0], HCI_COMMAND_PKT);
        assert_eq!(pkt[1..3], OPCODE_REMOTE_NAME_REQ.to_le_bytes());
        assert_eq!(&pkt[4..10], &mac.bytes());
    }

    #[test]
    fn test_sockaddr_hci_raw() {
        let addr = sockaddr_hci_raw();
        let family = u16::from_le_bytes([addr[0], addr[1]]);
        assert_eq!(family as i32, AF_BLUETOOTH);
        let dev = u16::from_le_bytes([addr[2], addr[3]]);
        assert_eq!(dev, HCI_DEV_NONE);
        let channel = u16::from_le_bytes([addr[4], addr[5]]);
        assert_eq!(channel, HCI_CHANNEL_RAW);
    }

    #[test]
    fn test_sockaddr_l2cap() {
        let mac = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        let addr = sockaddr_l2cap(mac, 1);
        let family = u16::from_le_bytes([addr[0], addr[1]]);
        assert_eq!(family as i32, AF_BLUETOOTH);
        let psm = u16::from_le_bytes([addr[2], addr[3]]);
        assert_eq!(psm, 1);
        assert_eq!(&addr[4..10], &mac.bytes());
    }

    #[test]
    fn test_conn_info_request() {
        let mac = Mac::parse("AA:BB:CC:DD:EE:FF").unwrap();
        let req = conn_info_request(mac);
        assert_eq!(&req[0..6], &mac.bytes());
        assert_eq!(req[6], ACL_LINK);
        assert_eq!(req.len(), 24);
    }

    #[test]
    fn test_parse_conn_handle_ok() {
        let mut buf = vec![0u8; 10];
        buf[8] = 0x34;
        buf[9] = 0x12;
        assert_eq!(parse_conn_handle(&buf).unwrap(), 0x1234);
    }

    #[test]
    fn test_parse_conn_handle_short() {
        let buf = vec![0u8; 5];
        assert!(parse_conn_handle(&buf).is_err());
    }

    #[test]
    fn test_timeval_bytes() {
        let tv = timeval_bytes(Duration::new(5, 123_456_000));
        let sec = i64::from_le_bytes(tv[0..8].try_into().unwrap());
        let usec = i64::from_le_bytes(tv[8..16].try_into().unwrap());
        assert_eq!(sec, 5);
        assert_eq!(usec, 123_456);
    }

    #[test]
    fn test_parse_event_not_event_pkt() {
        assert!(parse_event(&[0x00, 0x01, 0x00]).is_none());
    }

    #[test]
    fn test_parse_event_too_short() {
        assert!(parse_event(&[HCI_EVENT_PKT, 0x01]).is_none());
    }

    #[test]
    fn test_parse_event_inquiry_complete() {
        let buf = [HCI_EVENT_PKT, EVT_INQUIRY_COMPLETE, 0];
        assert_eq!(parse_event(&buf), Some(HciEvent::InquiryComplete));
    }

    #[test]
    fn test_parse_event_inquiry_result() {
        let buf = [HCI_EVENT_PKT, EVT_INQUIRY_RESULT, 1, 3];
        assert_eq!(
            parse_event(&buf),
            Some(HciEvent::InquiryResult { num_responses: 3 })
        );
    }

    #[test]
    fn test_parse_event_cmd_complete() {
        let buf = [HCI_EVENT_PKT, EVT_CMD_COMPLETE, 4, 1, 0x34, 0x12, 0x00];
        match parse_event(&buf) {
            Some(HciEvent::CommandComplete { opcode, ret }) => {
                assert_eq!(opcode, 0x1234);
                assert_eq!(ret, vec![0x00]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_event_remote_name() {
        let name = b"MyDevice\0extra";
        let mut buf = vec![
            HCI_EVENT_PKT,
            EVT_REMOTE_NAME_COMPLETE,
            (7 + name.len()) as u8,
            0x00,
        ];
        buf.extend_from_slice(&[0u8; 6]); // bdaddr
        buf.extend_from_slice(name);
        match parse_event(&buf) {
            Some(HciEvent::RemoteNameComplete { status, name }) => {
                assert_eq!(status, 0);
                assert_eq!(name, "MyDevice");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_event_other() {
        let buf = [HCI_EVENT_PKT, 0xFF, 0];
        assert_eq!(parse_event(&buf), Some(HciEvent::Other { code: 0xFF }));
    }

    #[test]
    fn test_rssi_from_return_ok() {
        let ret = [0x00, 0x00, 0x00, 0xC5u8]; // -59 as i8
        assert_eq!(rssi_from_return(&ret), Some(-59));
    }

    #[test]
    fn test_rssi_from_return_error_status() {
        let ret = [0x01, 0x00, 0x00, 0x00];
        assert_eq!(rssi_from_return(&ret), None);
    }

    #[test]
    fn test_rssi_from_return_short() {
        assert_eq!(rssi_from_return(&[0x00, 0x00]), None);
    }

    #[test]
    fn test_tx_from_return_ok() {
        let ret = [0x00, 0x00, 0x00, 0x04u8];
        assert_eq!(tx_from_return(&ret), Some(4));
    }

    #[test]
    fn test_tx_from_return_negative() {
        let ret = [0x00, 0x00, 0x00, 0xF6u8]; // -10
        assert_eq!(tx_from_return(&ret), Some(-10));
    }
}
