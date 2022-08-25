extern crate rustc_serialize;

extern crate http_muncher;
use mio::{Interest, Poll, Token};
use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{self, Sender};

use http_muncher::{Parser, ParserHandler};
use mio::net::TcpStream;

use crate::frame::{Opcode, WebSocketFrame};
use crate::http::gen_key;
use crate::server::ServerMessage;
// use crate::server::WebSocketServer;

#[derive(Debug)]
pub struct HttpParser {
    current_key: Option<String>,
    headers: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Debug)]
pub struct Connect {
    pub socket: TcpStream,
    pub parser: Parser,
    pub interest: Interest,
    pub state: ClientState,
    pub headers: Arc<Mutex<HashMap<String, String>>>,
    pub sender: mpsc::Sender<ServerMessage>,
}

impl Connect {
    pub async fn read(&mut self, poll: &mut Poll, token: &Token) -> Result<(), std::io::Error> {
        println!("in read");
        match self.state {
            ClientState::AwaitingHandshake(_) => Ok(self.read_handshake(poll, token)?),
            ClientState::Connected => Ok(self.read_frame(poll, token).await?),
            ClientState::HandshakeResponse => todo!(),
        }
    }

    pub fn write(&mut self, payload: &str) -> std::io::Result<()> {
        let frame = WebSocketFrame::from(payload);
        frame.write(&mut self.socket)?;
        Ok(())
    }

    pub async fn read_frame(
        &mut self,
        poll: &mut Poll,
        token: &Token,
    ) -> Result<(), std::io::Error> {
        let frame = WebSocketFrame::read(&mut self.socket)?;

        match frame.get_opcode() {
            Opcode::Pong => {
                let pong_frame = WebSocketFrame::pong(&frame);
                pong_frame.write(&mut self.socket)?;
                Ok(())
            }
            Opcode::TextFrame => {
                println!("recieved: {:?}", std::str::from_utf8(&frame.payload));
                let payload = frame.payload.clone();
                let message = ServerMessage::Text(payload, token.clone());
                let _x = self.sender.send(message).await;
                Ok(())
            }
            Opcode::ConnectionClose => {
                // Connection close requset
                let close_frame = WebSocketFrame::close_from(&frame);
                close_frame.write(&mut self.socket)?;
                let _close = poll.registry().deregister(&mut self.socket);
                let message = ServerMessage::Close(token.clone());
                let _ = self.sender.send(message).await;
                Ok(())
            }
            Opcode::BinaryFrame => todo!(),
            Opcode::Ping => todo!(),
        }
    }

    pub fn read_handshake(&mut self, poll: &mut Poll, token: &Token) -> std::io::Result<()> {
        loop {
            let mut buf = [0; 2048];
            match self.socket.read(&mut buf) {
                Err(e) => {
                    println!("Error while reading socket: {:?}", e);
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                }
                Ok(_len) => {
                    if _len == 0 {
                        break;
                    }

                    if let ClientState::AwaitingHandshake(ref mut parser_state) = self.state {
                        self.parser.parse(parser_state, &buf);
                    };

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
        return Ok(());
    }

    pub fn write_handshake(&mut self, poll: &mut Poll, token: &Token) -> std::io::Result<()> {
        let headers = self.headers.lock().unwrap();
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
        self.socket.write_all(response.as_bytes())?;

        // Change the state:
        self.state = ClientState::Connected;

        // And change the interest back to `readable()`:
        let _ = poll
            .registry()
            .reregister(&mut self.socket, *token, Interest::READABLE);

        Ok(())
    }

    pub fn new(socket: TcpStream, sender: mpsc::Sender<ServerMessage>) -> Connect {
        let headers = Arc::new(Mutex::new(HashMap::new()));

        Connect {
            socket,
            parser: Parser::request(),
            interest: Interest::READABLE,
            headers: headers.clone(),
            state: ClientState::AwaitingHandshake(HttpParser {
                current_key: None,
                headers: headers.clone(),
            }),
            sender,
        }
    }
}

#[derive(Debug)]
pub struct WebSocketClient {
    // pub socket: TcpStream,
    pub connection: Connect,
}

unsafe impl Send for WebSocketClient {}
unsafe impl Sync for WebSocketClient {}

impl ParserHandler for HttpParser {
    fn on_header_field(&mut self, _p: &mut Parser, s: &[u8]) -> bool {
        self.current_key = Some(std::str::from_utf8(s).unwrap().to_string());
        true
    }

    fn on_header_value(&mut self, _p: &mut Parser, s: &[u8]) -> bool {
        let mut r = self.headers.lock().unwrap();
        r.insert(
            self.current_key.clone().unwrap(),
            std::str::from_utf8(s).unwrap().to_string(),
        );
        true
    }

    fn on_headers_complete(&mut self, _p: &mut Parser) -> bool {
        false
    }
}

#[derive(Debug)]
pub enum ClientState {
    AwaitingHandshake(HttpParser),
    HandshakeResponse,
    Connected,
}

impl WebSocketClient {
    pub fn new(socket: TcpStream, sender: Sender<ServerMessage>) -> WebSocketClient {
        WebSocketClient {
            connection: Connect::new(socket, sender),
        }
    }
}
