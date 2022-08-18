extern crate rustc_serialize;
extern crate sha1;
use rustc_serialize::base64::{ToBase64, STANDARD};
extern crate http_muncher;
use mio::{Interest, Poll, Token};
use sha1::Digest;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::rc::Rc;

use http_muncher::{Parser, ParserHandler};
use mio::net::TcpStream;

#[derive(Debug)]
pub struct HttpParser {
    current_key: Option<String>,
    headers: Rc<RefCell<HashMap<String, String>>>,
}

#[derive(Debug)]
pub struct WebSocketClient {
    pub socket: TcpStream,
    pub parser: Parser,
    pub interest: Interest,
    pub state: ClientState,
    pub http_parser: HttpParser,
}

impl ParserHandler for HttpParser {
    fn on_header_field(&mut self, _p: &mut Parser, s: &[u8]) -> bool {
        self.current_key = Some(std::str::from_utf8(s).unwrap().to_string());
        true
    }

    fn on_header_value(&mut self, _p: &mut Parser, s: &[u8]) -> bool {
        self.headers.borrow_mut().insert(
            self.current_key.clone().unwrap(),
            std::str::from_utf8(s).unwrap().to_string(),
        );
        true
    }

    fn on_headers_complete(&mut self, _p: &mut Parser) -> bool {
        false
    }
}

#[derive(PartialEq, Debug)]
pub enum ClientState {
    AwaitingHandshake,
    HandshakeResponse,
    Connected,
}

impl WebSocketClient {
    pub fn read(&mut self, poll: &mut Poll, token: &Token) {
        match self.state {
            ClientState::AwaitingHandshake => {
                self.read_handshake(poll, token);
            }
            ClientState::HandshakeResponse => todo!(),
            ClientState::Connected => todo!(),
        }
    }

    pub fn read_handshake(&mut self, poll: &mut Poll, token: &Token) {
        loop {
            let mut buf = [0; 2048];
            // println!("socket: {:?}", self.socket);
            match self.socket.read(&mut buf) {
                Err(e) => {
                    println!("Error while reading socket: {:?}", e);
                    return;
                }
                // Ok(None) => break,
                Ok(_len) => {
                    if _len == 0 {
                        break;
                    }
                    let hp = &mut self.http_parser;

                    self.parser.parse(hp, &buf);
                    if self.parser.is_upgrade() {
                        self.state = ClientState::HandshakeResponse;

                        let _res = poll.registry().reregister(
                            &mut self.socket,
                            *token,
                            Interest::WRITABLE,
                        );
                        break;
                    }
                }
            }
        }
    }

    pub fn write(&mut self, poll: &mut Poll, token: &Token) {
        // Get the headers HashMap from the Rc<RefCell<...>> wrapper:
        let headers = self.http_parser.headers.borrow();

        // Find the header that interests us, and generate the key from its value:
        let response_key = gen_key(&headers.get("Sec-WebSocket-Key").unwrap());

        let response = fmt::format(format_args!(
            "HTTP/1.1 101 Switching Protocols\r\n\
                                                 Connection: Upgrade\r\n\
                                                 Sec-WebSocket-Accept: {}\r\n\
                                                 Upgrade: websocket\r\n\r\n",
            response_key
        ));

        // Write the response to the socket:
        let _res = self.socket.write_all(response.as_bytes());

        // Change the state:
        self.state = ClientState::Connected;

        // And change the interest back to `readable()`:
        let _ = poll
            .registry()
            .reregister(&mut self.socket, *token, Interest::READABLE);
    }

    pub fn new(socket: TcpStream) -> WebSocketClient {
        WebSocketClient {
            socket: socket,
            parser: Parser::request(),
            interest: Interest::READABLE,
            state: ClientState::AwaitingHandshake,
            http_parser: HttpParser {
                current_key: None,
                headers: Rc::new(RefCell::new(HashMap::new())),
            },
        }
    }
}

fn gen_key(key: &String) -> String {
    let mut m = sha1::Sha1::new();
    let mut _buf = [0u8; 20];

    let data = [
        key.as_bytes(),
        "258EAFA5-E914-47DA-95CA-C5AB0DC85B11".as_bytes(), // appending a key
    ]
    .concat();
    m.update(data);

    let _len = m.finalize();

    return _len.to_base64(STANDARD);
}
