extern crate rustc_serialize;

extern crate http_muncher;
use mio::{Interest, Poll, Token};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::rc::Rc;

use http_muncher::{Parser, ParserHandler};
use mio::net::TcpStream;

use crate::frame::WebSocketFrame;
use crate::http::gen_key;

#[derive(Debug, PartialEq)]
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
    pub headers: Rc<RefCell<HashMap<String, String>>>,
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
    AwaitingHandshake(HttpParser),
    HandshakeResponse,
    Connected,
}

impl WebSocketClient {
    pub fn read(&mut self, poll: &mut Poll, token: &Token) {
        match self.state {
            ClientState::AwaitingHandshake(_) => self.read_handshake(poll, token),
            ClientState::HandshakeResponse => {}
            ClientState::Connected => self.read_frame(poll, token),
        }
    }

    pub fn write(&mut self, poll: &mut Poll, token: &Token) {
        match self.state {
            ClientState::HandshakeResponse => self.write_handshake(poll, token),
            ClientState::Connected => {
                println!("socket is writable");
                std::thread::sleep(std::time::Duration::from_millis(5000));
                // prepare websocketframe without mask
                let frame = WebSocketFrame::from("hey there!");
                // socket write for this frame
                // 1. write header
                // 2. write payload
                frame.write(&mut self.socket);
                // change interest to readable
                let _ = poll
                    .registry()
                    .reregister(&mut self.socket, *token, Interest::READABLE);
            }
            _ => {}
        }
    }

    pub fn read_frame(&mut self, poll: &mut Poll, token: &Token) {
        // read websocket frame
        let frame = WebSocketFrame::read(&mut self.socket);

        match frame {
            Ok(wb) => {
                println!("suucess, {:?}", wb.payload);
                let pay = String::from_utf8(wb.payload).unwrap();
                println!("data = {}", pay);

                // send response
                // reregister into write
                let _ = poll
                    .registry()
                    .reregister(&mut self.socket, *token, Interest::WRITABLE);
            }
            Err(err) => println!("error occired"),
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

                    if let ClientState::AwaitingHandshake(ref mut parser_state) = self.state {
                        // let http_parser = parser_state.get_mut();
                        self.parser.parse(parser_state, &buf);
                    };

                    // self.parser.parse(hp, &buf);
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

    pub fn write_handshake(&mut self, poll: &mut Poll, token: &Token) {
        // Get the headers HashMap from the Rc<RefCell<...>> wrapper:
        let headers = self.headers.borrow();

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
        let headers = Rc::new(RefCell::new(HashMap::new()));

        WebSocketClient {
            socket: socket,
            parser: Parser::request(),
            interest: Interest::READABLE,
            headers: headers.clone(),
            state: ClientState::AwaitingHandshake(HttpParser {
                current_key: None,
                headers: headers.clone(),
            }),
        }
    }
}
