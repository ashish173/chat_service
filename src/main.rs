// You can run this example from the root of the mio repo:
// cargo run --example tcp_server --features="os-poll net"
mod client;
use client::WebSocketClient;
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Registry, Token};
use std::collections::HashMap;
use std::io::{self, Read};
use std::str::from_utf8;

// Setup some tokens to allow us to identify which event is for which socket.
const SERVER: Token = Token(0);

struct WebSocketServer {
    socket: TcpListener,
    connections: HashMap<Token, WebSocketClient>,
    token_counter: usize,
}

impl WebSocketServer {
    fn new(socket: TcpListener) -> WebSocketServer {
        WebSocketServer {
            socket,
            connections: HashMap::new(),
            token_counter: SERVER.0 + 1,
        }
    }

    fn next(&mut self) -> Token {
        let next = self.token_counter;
        self.token_counter += 1;
        Token(next)
    }
}

#[cfg(not(target_os = "wasi"))]
fn main() -> io::Result<()> {
    let mut poll = Poll::new()?;
    // Create storage for events.
    let mut events = Events::with_capacity(128);

    // Setup the TCP server socket.
    let addr = "127.0.0.1:9000".parse().unwrap();
    let mut server = TcpListener::bind(addr)?;

    // Register the server with poll we can receive events for it.
    poll.registry()
        .register(&mut server, SERVER, Interest::READABLE)?;

    let mut server = WebSocketServer::new(server);

    loop {
        poll.poll(&mut events, None)?;
        // println!("event: {:?}", events);
        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    // Received an event for the TCP server socket, which
                    // indicates we can accept an connection.
                    let mut client = match server.socket.accept() {
                        Ok((connection, _)) => WebSocketClient::new(connection),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            // If we get a `WouldBlock` error we know our
                            // listener has no more incoming connections queued,
                            // so we can return to polling and wait for some
                            // more.
                            break;
                        }
                        Err(e) => {
                            // If it was any other kind of error, something went
                            // wrong and we terminate with an error.
                            return Err(e);
                        }
                    };

                    // println!("Accepted connection from: {}", address);

                    let token = server.next();
                    poll.registry()
                        .register(&mut client.socket, token, Interest::READABLE)?;
                    // println!("poll registry for token {:?}", token);

                    server.connections.insert(token, client);
                },
                token => {
                    // Maybe received an event for a TCP connection.

                    // println!("event: {:?}", event);
                    let done = if let Some(client) = server.connections.get_mut(&token) {
                        if event.is_readable() {
                            client.read(&mut poll, &token);
                        } else if event.is_writable() {
                            client.write(&mut poll, &token);
                        }
                        false
                        // handle_connection_event(poll.registry(), &mut client.socket, event)?
                    } else {
                        // Sporadic events happen, we can safely ignore them.
                        false
                    };
                    if done {
                        if let Some(mut client) = server.connections.remove(&token) {
                            poll.registry().deregister(&mut client.socket)?;
                        }
                    }
                }
            }
        }
    }
}
