use std::{
    io::{self, ErrorKind, Read, Write},
    iter,
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use mio::net::TcpStream;

#[derive(Debug, Clone, Copy)]
pub enum Opcode {
    TextFrame = 1,       // 1
    BinaryFrame = 2,     // 2
    ConnectionClose = 8, // 8
    Ping = 9,            // 9
    Pong = 0xA,          // 0xA
}

impl Opcode {
    fn from(code: u8) -> Option<Opcode> {
        match code {
            1 => Some(Opcode::TextFrame),
            2 => Some(Opcode::BinaryFrame),
            8 => Some(Opcode::ConnectionClose),
            9 => Some(Opcode::Ping),
            0xA => Some(Opcode::Pong),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct WebSocketFrameHeader {
    fin: bool,
    rsv1: bool,
    rsv2: bool,
    rsv3: bool,
    opcode: Opcode,
    masked: bool,
    payload_length: u8,
}

#[derive(Debug)]
pub struct WebSocketFrame {
    header: WebSocketFrameHeader,
    pub mask: Option<[u8; 4]>,
    pub payload: Vec<u8>,
}

impl WebSocketFrame {
    pub fn close_from(frame: &WebSocketFrame) -> WebSocketFrame {
        let payload = &frame.payload;

        let body = if payload.len() > 0 {
            let mut t = Vec::with_capacity(2);
            let buf = &payload[0..2];
            let _ = t.write(&buf);
            t
        } else {
            Vec::new()
        };
        let close_opcode = Opcode::ConnectionClose;
        let payload_for_header = std::str::from_utf8(&body).unwrap();

        WebSocketFrame {
            header: Self::prepare_headers(payload_for_header, close_opcode),
            mask: None,
            payload: body,
        }
    }

    pub fn pong(&self) -> Self {
        let pong_opcode = Opcode::Pong;
        let bytes = std::str::from_utf8(&self.payload).unwrap();
        let header = Self::prepare_headers(bytes, pong_opcode);

        WebSocketFrame {
            header,
            mask: None,
            payload: Vec::from(bytes),
        }
    }

    pub fn get_opcode(&self) -> Opcode {
        self.header.opcode
    }

    pub fn write(&self, socket: &mut TcpStream) -> Result<(), std::io::Error> {
        self.write_header(socket);
        socket.write(&self.payload)?;
        Ok(())
    }

    pub fn write_header(&self, socket: &mut TcpStream) {
        let b1 = (self.header.fin as u8) << 7
            | (self.header.rsv1 as u8) << 6
            | (self.header.rsv2 as u8) << 5
            | (self.header.rsv3 as u8) << 4
            | (self.header.opcode as u8) & 0x0F; // & with 00001111

        let b2 = (self.header.payload_length as u8) & 0x7F;

        let buf = ((b1 as u16) << 8) | (b2 as u16);
        let _ = socket.write_u16::<BigEndian>(buf);
    }

    pub fn from(payload: &str) -> WebSocketFrame {
        let opcode = Opcode::TextFrame;
        let header = Self::prepare_headers(payload, opcode);

        WebSocketFrame {
            header,
            mask: None,
            payload: Vec::from(payload),
        }
    }

    pub fn prepare_headers(payload: &str, opcode: Opcode) -> WebSocketFrameHeader {
        // opcode depends on
        WebSocketFrameHeader {
            fin: true,
            rsv1: false,
            rsv2: false,
            rsv3: false,
            opcode,
            masked: false,
            payload_length: payload.len() as u8,
        }
    }

    pub fn read<R: Read>(socket: &mut R) -> std::io::Result<WebSocketFrame> {
        let buf = socket.read_u16::<BigEndian>()?;
        let header = Self::get_header(buf).map_err(|e| io::Error::new(ErrorKind::Other, e))?;
        let len = Self::get_pay_length(socket, header.payload_length)?;
        // get mask_key. its 32 bit or 4 bytes
        let mask = if header.masked {
            let mut buf = [0; 4];
            let _mask = socket.read(&mut buf)?;
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
            header,
            payload,
            mask,
        })
    }

    fn apply_mask(mask_key: [u8; 4], payload: &mut Vec<u8>) {
        for (idx, c) in payload.iter_mut().enumerate() {
            *c = *c ^ mask_key[idx % 4];
        }
    }

    fn get_pay_length<R: Read>(socket: &mut R, length: u8) -> std::io::Result<usize> {
        match length {
            // when the length is 127 read next 8 bytes
            127 => socket
                .read_u64::<BigEndian>()
                .map(|v| v as usize)
                .map_err(From::from),
            // when the length is 126 read next 2 bytes
            126 => socket
                .read_u16::<BigEndian>()
                .map(|v| v as usize)
                .map_err(From::from),
            // anything less than 126 is the actual length
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

    fn get_header(buf: u16) -> Result<WebSocketFrameHeader, String> {
        let opcode = ((buf >> 8) as u8) & 0x0F;
        let opcode_res = Opcode::from(opcode);

        if let Some(op) = opcode_res {
            Ok(WebSocketFrameHeader {
                fin: buf >> 8 & 0x80 == 0x80,
                rsv1: buf >> 8 & 0x40 == 0x40,
                rsv2: buf >> 8 & 0x20 == 0x20,
                rsv3: buf >> 8 & 0x10 == 0x10,
                opcode: op,
                masked: buf & 0x80 == 0x80,         // & with 10000000
                payload_length: (buf as u8) & 0x7F, // & with 01111111
            })
        } else {
            Err("sdf".to_owned())
        }
    }
}
