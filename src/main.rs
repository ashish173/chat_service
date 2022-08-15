// You can run this example from the root of the mio repo:
// cargo run --example tcp_server --features="os-poll net"
mod client;
use client::WebSocketClient;
use mio::event::Event;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Registry, Token};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::str::from_utf8;

// Setup some tokens to allow us to identify which event is for which socket.
const SERVER: Token = Token(0);

// Some data we'll send over the connection.
const DATA: &[u8] = b"Hello world!\n";

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
    // env_logger::init();

    // Create a poll instance.
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

    println!("You can connect to the server using `nc`:");
    println!(" $ nc 127.0.0.1 9000");
    println!("You'll see our welcome message and anything you type will be printed here.");

    loop {
        println!("in for loop");
        poll.poll(&mut events, None)?;
        println!("event: {:?}", events);
        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    println!("in server match");
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
                    println!("poll registry for token {:?}", token);

                    server.connections.insert(token, client);
                },
                token => {
                    println!("token fired for {:?}", token);
                    // Maybe received an event for a TCP connection.
                    let done = if let Some(client) = server.connections.get_mut(&token) {
                        client.read();
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

/// Returns `true` if the connection is done.
fn handle_connection_event(
    registry: &Registry,
    connection: &mut TcpStream,
    event: &Event,
) -> io::Result<bool> {
    // if event.is_writable() {
    //     println!("event: {:?}", event);
    //     // We can (maybe) write to the connection.
    //     match connection.write(DATA) {
    //         // We want to write the entire `DATA` buffer in a single go. If we
    //         // write less we'll return a short write error (same as
    //         // `io::Write::write_all` does).
    //         Ok(n) if n < DATA.len() => return Err(io::ErrorKind::WriteZero.into()),
    //         Ok(_) => {
    //             // After we've written something we'll reregister the connection
    //             // to only respond to readable events.
    //             println!("token {:?}", event.token());
    //             registry.reregister(connection, event.token(), Interest::READABLE)?;
    //             // connection.write(DATA);
    //         }
    //         // Would block "errors" are the OS's way of saying that the
    //         // connection is not actually ready to perform this I/O operation.
    //         Err(ref err) if would_block(err) => {}
    //         // Got interrupted (how rude!), we'll try again.
    //         Err(ref err) if interrupted(err) => {
    //             return handle_connection_event(registry, connection, event)
    //         }
    //         // Other errors we'll consider fatal.
    //         Err(err) => return Err(err),
    //     }
    // }

    if event.is_readable() {
        println!("connection is readable {:?}", event);
        let mut connection_closed = false;
        let mut received_data = vec![0; 4096];
        let mut bytes_read = 0;
        // We can (maybe) read from the connection.
        loop {
            println!("read loop run");
            let data_read = connection.read(&mut received_data[bytes_read..]);
            println!("data read {:?}", data_read);
            match data_read {
                Ok(0) => {
                    // Reading 0 bytes means the other side has closed the
                    // connection or is done writing, then so are we.
                    println!("connection closed");
                    connection_closed = true;
                    break;
                }
                Ok(n) => {
                    bytes_read += n;
                    if bytes_read == received_data.len() {
                        received_data.resize(received_data.len() + 1024, 0);
                    }
                }
                // Would block "errors" are the OS's way of saying that the
                // connection is not actually ready to perform this I/O operation.
                Err(ref err) if would_block(err) => break,
                Err(ref err) if interrupted(err) => continue,
                // Other errors we'll consider fatal.
                Err(err) => return Err(err),
            }
        }

        if bytes_read != 0 {
            let received_data = &received_data[..bytes_read];
            if let Ok(str_buf) = from_utf8(received_data) {
                println!("Received data: {}", str_buf.trim_end());
            } else {
                println!("Received (none UTF-8) data: {:?}", received_data);
            }
        }

        if connection_closed {
            println!("Connection closed");
            return Ok(true);
        }
    }

    Ok(false)
}

fn would_block(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::WouldBlock
}

fn interrupted(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::Interrupted
}

#[cfg(target_os = "wasi")]
fn main() {
    panic!("can't bind to an address with wasi")
}
