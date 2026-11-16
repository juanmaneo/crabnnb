pub const ATT_CID: u16 = 0x0004;
pub const REQUESTED_MTU: u16 = 247;

pub mod op {
    pub const ERROR_RSP: u8 = 0x01;
    pub const EXCHANGE_MTU_REQ: u8 = 0x02;
    pub const EXCHANGE_MTU_RSP: u8 = 0x03;
    pub const READ_REQ: u8 = 0x0a;
    pub const READ_RSP: u8 = 0x0b;
    pub const WRITE_REQ: u8 = 0x0d;
    pub const WRITE_RSP: u8 = 0x13;
    pub const HANDLE_VALUE_NOTIF: u8 = 0x1b;
    pub const WRITE_CMD: u8 = 0x52;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttPdu {
    Error {
        req_opcode: u8,
        handle: u16,
        error_code: u8,
    },
    ExchangeMtuRsp(u16),
    WriteRsp,
    ReadRsp(Vec<u8>),
    Notification { handle: u16, value: Vec<u8> },
    Other(u8),
}

pub fn exchange_mtu_req(mtu: u16) -> Vec<u8> {
    vec![op::EXCHANGE_MTU_REQ, (mtu & 0xff) as u8, (mtu >> 8) as u8]
}

pub fn write_command(handle: u16, value: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(3 + value.len());
    v.push(op::WRITE_CMD);
    v.extend_from_slice(&handle.to_le_bytes());
    v.extend_from_slice(value);
    v
}

pub fn read_request(handle: u16) -> Vec<u8> {
    let mut v = Vec::with_capacity(3);
    v.push(op::READ_REQ);
    v.extend_from_slice(&handle.to_le_bytes());
    v
}

pub fn parse_att(buf: &[u8]) -> Option<AttPdu> {
    let (&opcode, rest) = buf.split_first()?;
    match opcode {
        op::ERROR_RSP if rest.len() >= 4 => Some(AttPdu::Error {
            req_opcode: rest[0],
            handle: u16::from_le_bytes([rest[1], rest[2]]),
            error_code: rest[3],
        }),
        op::EXCHANGE_MTU_RSP if rest.len() >= 2 => {
            Some(AttPdu::ExchangeMtuRsp(u16::from_le_bytes([rest[0], rest[1]])))
        }
        op::WRITE_RSP => Some(AttPdu::WriteRsp),
        op::READ_RSP => Some(AttPdu::ReadRsp(rest.to_vec())),
        op::HANDLE_VALUE_NOTIF if rest.len() >= 2 => Some(AttPdu::Notification {
            handle: u16::from_le_bytes([rest[0], rest[1]]),
            value: rest[2..].to_vec(),
        }),
        other => Some(AttPdu::Other(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_mtu_req() {
        assert_eq!(exchange_mtu_req(247), vec![0x02, 0xf7, 0x00]);
        assert_eq!(exchange_mtu_req(23), vec![0x02, 0x17, 0x00]);
    }

    #[test]
    fn test_write_command() {
        let v = write_command(0x0031, &[0xaa, 0xbb]);
        assert_eq!(v, vec![0x52, 0x31, 0x00, 0xaa, 0xbb]);
    }

    #[test]
    fn test_read_request() {
        assert_eq!(read_request(0x0034), vec![0x0a, 0x34, 0x00]);
    }

    #[test]
    fn test_parse_error() {
        let buf = [0x01, 0x52, 0x31, 0x00, 0x0a];
        assert_eq!(
            parse_att(&buf),
            Some(AttPdu::Error {
                req_opcode: 0x52,
                handle: 0x0031,
                error_code: 0x0a
            })
        );
    }

    #[test]
    fn test_parse_exchange_mtu_rsp() {
        let buf = [0x03, 0xf7, 0x00];
        assert_eq!(parse_att(&buf), Some(AttPdu::ExchangeMtuRsp(247)));
    }

    #[test]
    fn test_parse_notification() {
        let buf = [0x1b, 0x34, 0x00, 0xde, 0xad];
        assert_eq!(
            parse_att(&buf),
            Some(AttPdu::Notification {
                handle: 0x0034,
                value: vec![0xde, 0xad]
            })
        );
    }

    #[test]
    fn test_parse_write_rsp_and_read_rsp() {
        assert_eq!(parse_att(&[0x13]), Some(AttPdu::WriteRsp));
        assert_eq!(
            parse_att(&[0x0b, 0x01, 0x02]),
            Some(AttPdu::ReadRsp(vec![0x01, 0x02]))
        );
    }

    #[test]
    fn test_parse_empty_and_other() {
        assert_eq!(parse_att(&[]), None);
        assert_eq!(parse_att(&[0xff, 0x00]), Some(AttPdu::Other(0xff)));
    }
}
