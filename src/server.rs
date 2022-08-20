// You can run this example from the root of the mio repo:
// cargo run --example tcp_server --features="os-poll net"
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use websocket::receiver::Receiver;
// use WebSocketClient;

use crate::client::{self, WebSocketClient};

// Setup some tokens to allow us to identify which event is for which socket.
const SERVER: Token = Token(0);

pub struct WebSocketServer {
    socket: TcpListener,
    // connections: HashMap<Token, WebSocketClient>,
    token_counter: usize,
    // receiver: mpsc::Receiver<()>,
    sender: mpsc::Sender<Vec<u8>>,
}

impl WebSocketServer {
    fn new(
        socket: TcpListener,
        // receiver: mpsc::Receiver<()>,
        sender: mpsc::Sender<Vec<u8>>,
    ) -> WebSocketServer {
        WebSocketServer {
            socket,
            // connections: HashMap::new(),
            token_counter: SERVER.0 + 1,
            // receiver,
            sender,
        }
    }

    fn next(&mut self) -> Token {
        let next = self.token_counter;
        self.token_counter += 1;
        Token(next)
    }

    pub fn broadcast(&self, payload: &Vec<u8>) {
        //
    }
}

struct Shared(Arc<Mutex<Connection>>);

impl Shared {
    fn new(poll: Poll) -> Shared {
        Shared(Arc::new(Mutex::new(Connection::new(poll))))
    }
}

struct Connection {
    clients: HashMap<Token, WebSocketClient>,
    poll: Poll,
}
impl Connection {
    fn new(poll: Poll) -> Connection {
        Connection {
            clients: HashMap::new(),
            poll: poll,
        }
    }
}

#[tokio::main]
#[cfg(not(target_os = "wasi"))]
pub async fn main() -> io::Result<()> {
    use std::io::Bytes;

    let mut poll = Poll::new()?;
    // Create storage for events.
    let mut events = Events::with_capacity(128);
    let shared = Shared::new(poll);
    let conn = shared.0.clone();

    // Setup the TCP server socket.
    let addr = "127.0.0.1:9000".parse().unwrap();
    let mut server = TcpListener::bind(addr)?;

    let conn1 = shared.0.clone();
    // Register the server with poll we can receive events for it.
    conn.lock()
        .unwrap()
        .poll
        .registry()
        .register(&mut server, SERVER, Interest::READABLE)?;

    let (tx, mut rx) = mpsc::channel::<(Vec<u8>)>(100);
    let mut server = WebSocketServer::new(server, tx);
    let rxc = server.sender.clone();

    println!("before spawn in server:main");
    tokio::spawn(async move {
        let rec = rx.recv().await.unwrap();
        // let v = Bytes::from(rec);
        println!("value received: {:?}", rec);
        let mut mutg = conn.lock().unwrap();
        for (k, v) in &mut mutg.clients {
            v.write(&mut conn.lock().unwrap().poll, &k, "sdf");
        }
    });

    loop {
        conn1.lock().unwrap().poll.poll(&mut events, None)?;
        // println!("event: {:?}", events);
        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    // Received an event for the TCP server socket, which
                    // indicates we can accept an connection.
                    let mut client = match server.socket.accept() {
                        Ok((connection, _)) => {
                            // add this to shared object

                            // let c = connection.

                            WebSocketClient::new(connection, server.sender.clone())
                        }
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
                    // let token = Token(server.token_counter);
                    conn1.lock().unwrap().poll.registry().register(
                        &mut client.socket,
                        token,
                        Interest::WRITABLE,
                    )?;
                    // println!("poll registry for token {:?}", token);
                    // let c = conn2.lock().unwrap().insert(token, v)
                    conn1.lock().unwrap().clients.insert(token, client);
                    println!("server and client generated");
                    // server.connections.insert(token, client);
                },
                token => {
                    // Maybe received an event for a TCP connection.

                    // println!("event: {:?}", event);
                    let mut mutg = conn1.lock().unwrap();
                    let done = if let Some(client) = mutg.clients.get_mut(&token) {
                        println!("client {:?}", client);
                        if event.is_readable() {
                            println!("is readable");
                            let poll = &mut conn1.lock().unwrap().poll;
                            client.ro();
                            // client.read(poll, &token);
                        } else if event.is_writable() {
                            println!("is writable");
                            client.write(&mut conn1.lock().unwrap().poll, &token, "".into());
                        }
                        false
                        // handle_connection_event(poll.registry(), &mut client.socket, event)?
                    } else {
                        // Sporadic events happen, we can safely ignore them.
                        false
                    };
                    // if done {
                    //     if let Some(mut client) = server.connections.remove(&token) {
                    //         conn1
                    //             .lock()
                    //             .unwrap()
                    //             .poll
                    //             .registry()
                    //             .deregister(&mut client.socket)?;
                    //     }
                    // }
                }
            }
        }
    }
}
