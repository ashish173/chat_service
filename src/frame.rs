// use std::io::u16;
use std::{io::Read, iter};

use byteorder::{BigEndian, ReadBytesExt};

use mio::net::TcpStream;

enum Opcode {
    TextFrame,
    BinaryFrame,
    ConnectionClose,
    Ping,
    Pong,
}

#[derive(Debug)]
struct WebSocketFrameHeader {
    fin: bool,
    rsv1: bool,
    rsv2: bool,
    rsv3: bool,
    opcode: u8,
    // opcode: Opcode,
    masked: bool,
    payload_length: u8,
}

#[derive(Debug)]
pub struct WebSocketFrame {
    header: WebSocketFrameHeader,
    mask: Option<[u8; 4]>,
    payload: Vec<u8>,
}

impl WebSocketFrame {
    pub fn read<R: Read>(socket: &mut R) -> std::io::Result<WebSocketFrame> {
        // let mut buf = vec![0u8; 1024];
        let buf = socket.read_u16::<BigEndian>()?;
        let header = Self::get_header(buf);

        // get real payload length
        let len = Self::get_pay_length(socket, header.payload_length)?;
        // get mask_key. its 32 bit or 4 bytes
        let mask = if (header.masked) {
            // let mut mask = vec![0, 4];
            let mut buf = [0; 4];
            // let t = socket.read_u32::<BigEndian>();
            let mask = socket.read(&mut buf)?;
            Some(buf)
        } else {
            None
        };

        // get payload
        let mut payload = Self::read_payload(socket, len)?;

        // apply mask
        if let Some(mask_key) = mask {
            Self::apply_mask(mask_key, &mut payload)
        }

        Ok(WebSocketFrame {
            header: header,
            payload: payload,
            mask: mask,
        })
    }

    fn apply_mask(mask_key: [u8; 4], payload: &mut Vec<u8>) {
        for (idx, c) in payload.iter_mut().enumerate() {
            *c = *c ^ mask_key[idx % 4];
        }
    }

    fn get_pay_length<R: Read>(socket: &mut R, length: u8) -> std::io::Result<usize> {
        match length {
            127 => socket
                .read_u64::<BigEndian>()
                .map(|v| v as usize)
                .map_err(From::from),
            126 => socket
                .read_u16::<BigEndian>()
                .map(|v| v as usize)
                .map_err(From::from),
            _ => Ok(length as usize),
        }
    }

    fn read_payload<R: Read>(socket: &mut R, size: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(size);
        // TODO: get clarity
        // populating the vector with `0` so that it knows how many bytes to read
        // from the socket/reader. This is different from read_buf which doesn't
        // need this kind of allocation
        buf.extend(iter::repeat(0).take(size));

        let _ = socket.read(&mut buf)?;
        Ok(buf)
    }

    fn get_header(buf: u16) -> WebSocketFrameHeader {
        let opcode = ((buf >> 8) as u8) & 0x0F;

        WebSocketFrameHeader {
            fin: buf >> 8 & 0x80 == 0x80,
            rsv1: buf >> 8 & 0x40 == 0x40,
            rsv2: buf >> 8 & 0x20 == 0x20,
            rsv3: buf >> 8 & 0x10 == 0x10,
            opcode: opcode,
            masked: buf & 0x80 == 0x80,         // & with 10000000
            payload_length: (buf as u8) & 0x7F, // & with 01111111
        }
    }
}
