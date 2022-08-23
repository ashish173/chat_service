use crate::client::WebSocketClient;
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

// Setup some tokens to allow us to identify which event is for which socket.
const SERVER: Token = Token(0);

pub struct WebSocketServer {
    socket: TcpListener,
    token_counter: usize,
    sender: mpsc::Sender<(Vec<u8>, Token)>,
}

impl WebSocketServer {
    fn new(socket: TcpListener, sender: mpsc::Sender<(Vec<u8>, Token)>) -> WebSocketServer {
        WebSocketServer {
            socket,
            token_counter: SERVER.0 + 1,
            sender,
        }
    }

    fn next(&mut self) -> Token {
        let next = self.token_counter;
        self.token_counter += 1;
        Token(next)
    }

    pub fn broadcast(&self, _payload: &Vec<u8>) {
        //
    }
}

struct Shared {
    connection: Arc<Mutex<Connection>>,
    poll: Arc<Mutex<Poll>>,
}

impl Shared {
    fn new(poll: Poll) -> Shared {
        Shared {
            connection: Arc::new(Mutex::new(Connection::new())),
            poll: Arc::new(Mutex::new(poll)),
        }
    }
}

struct Connection {
    clients: HashMap<Token, WebSocketClient>,
    // poll: Poll,
}
impl Connection {
    fn new() -> Connection {
        Connection {
            clients: HashMap::new(),
            // poll: poll,
        }
    }
}

#[tokio::main]
#[cfg(not(target_os = "wasi"))]
pub async fn main() -> io::Result<()> {
    use tracing::metadata::Kind;

    use crate::client::ClientState;
    use std::borrow::BorrowMut;

    let poll = Poll::new()?;
    // Create storage for events.
    let mut events = Events::with_capacity(128);
    let shared = Shared::new(poll);
    // let conn = shared.connection.clone();
    let recv_poll = shared.poll.clone();
    let send_poll = shared.poll.clone();

    // Setup the TCP server socket.
    let addr = "127.0.0.1:9000".parse().unwrap();
    let mut server = TcpListener::bind(addr)?;

    let recv_conn = shared.connection.clone();
    let send_conn = shared.connection.clone();
    recv_poll
        .lock()
        .unwrap()
        .registry()
        .register(&mut server, SERVER, Interest::READABLE)?;

    let (tx, mut rx) = mpsc::channel::<(Vec<u8>, Token)>(1);
    let mut server = WebSocketServer::new(server, tx);
    let _rxc = server.sender.clone();

    tokio::spawn(async move {
        loop {
            let rec = rx.recv().await.unwrap();
            println!("value received: {:?}", rec);

            let payload = std::str::from_utf8(&rec.0).unwrap();
            let self_token = rec.1;
            let mut conn = recv_conn.lock().unwrap();
            for (k, v) in &mut conn.clients.borrow_mut().into_iter() {
                if k.0 != self_token.0 {
                    v.write(payload);
                } else {
                    println!("excluding {:?} value {:?}", k, payload);
                }
            }
        }
    });

    loop {
        send_poll
            .lock()
            .unwrap()
            .poll(&mut events, Some(std::time::Duration::from_millis(1000)))?;
        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    // Received an event for the TCP server socket, which
                    // indicates we can accept an connection.
                    let mut client = match server.socket.accept() {
                        Ok((connection, _)) => {
                            // add this to shared object
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
                    let token = server.next();
                    send_poll.lock().unwrap().registry().register(
                        &mut client.socket,
                        token,
                        Interest::READABLE,
                    )?;
                    send_conn.lock().unwrap().clients.insert(token, client);
                },
                token => {
                    // Maybe received an event for a TCP connection.
                    let mut mutg = send_conn.lock().unwrap();
                    let _done = if let Some(client) = mutg.clients.get_mut(&token) {
                        if event.is_readable() {
                            let mut new_poll = send_poll.lock().unwrap();
                            match client.read(&mut new_poll, &token).await {
                                Ok(res) => {}
                                Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                                    if let Some(mut client) = mutg.clients.remove(&token) {
                                        new_poll.registry().deregister(&mut client.socket)?;
                                    };
                                }
                                Err(_) => {}
                            }
                        } else if event.is_writable() {
                            let a = match client.state {
                                ClientState::HandshakeResponse => {
                                    client.write_handshake(&mut send_poll.lock().unwrap(), &token)
                                }
                                _ => Ok(()),
                            };
                            match a {
                                Ok(()) => {}
                                Err(e) => {
                                    println!("Server::err -> {:?}", e);
                                }
                            }
                        }
                        false
                        // handle_connection_event(poll.registry(), &mut client.socket, event)?
                    } else {
                        // Sporadic events happen, we can safely ignore them.
                        false
                    };
                }
            }
        }
    }
}
