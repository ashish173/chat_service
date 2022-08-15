extern crate http_muncher;
use std::io::Read;

use http_muncher::{Parser, ParserHandler};
use mio::net::TcpStream;
// use mio::tcp::*;
use mio::*;

struct HttpParser;
impl ParserHandler for HttpParser {
    // fn on_header_value(&mut self) {}
}

pub struct WebSocketClient {
    pub socket: TcpStream,
    pub http_parser: Parser,
}

impl WebSocketClient {
    pub fn read(&mut self) {
        println!("in client read");
        loop {
            let mut buf = [0; 2048];
            println!("socket: {:?}", self.socket);
            match self.socket.read(&mut buf) {
                Err(e) => {
                    println!("Error while reading socket: {:?}", e);
                    return;
                }
                // Ok(_) => {}
                Ok(len) => {
                    let mut hp = HttpParser {};
                    self.http_parser.parse(&mut hp, &buf);
                    println!("len ....{:?}", &buf);
                    if self.http_parser.is_upgrade() {
                        println!("http parser upgraded {:?}", self.http_parser);
                        break;
                    }
                }
            }
        }
    }

    pub fn new(socket: TcpStream) -> WebSocketClient {
        WebSocketClient {
            socket: socket,
            http_parser: Parser::request(),
        }
    }
}

fn main() {}
