use crate::client::WebSocketClient;
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::collections::HashMap;
use std::io::{self, Error};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

// Setup some tokens to allow us to identify which event is for which socket.
const SERVER: Token = Token(0);

#[derive(Debug)]
pub enum ServerMessage {
    Text(Vec<u8>, Token),
    Close(Token),
}

pub struct WebSocketServer {
    socket: TcpListener,
    token_counter: usize,
    sender: mpsc::Sender<ServerMessage>,
}

impl WebSocketServer {
    fn new(socket: TcpListener, sender: mpsc::Sender<ServerMessage>) -> WebSocketServer {
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
unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}

impl Connection {
    fn new() -> Connection {
        Connection {
            clients: HashMap::new(),
            // poll: poll,
        }
    }
}

// #[tokio::main]
#[cfg(not(target_os = "wasi"))]
pub async fn run() -> io::Result<()> {
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
        .await
        // .unwrap()
        .registry()
        .register(&mut server, SERVER, Interest::READABLE)?;

    let (tx, mut rx) = mpsc::channel::<ServerMessage>(1);
    let mut server = WebSocketServer::new(server, tx);
    let _rxc = server.sender.clone();

    // broadcast replies to clients
    tokio::spawn(async move {
        loop {
            let rec = rx.recv().await.unwrap();

            println!("testing .... {:?}", rec);
            match rec {
                ServerMessage::Text(data, self_token) => {
                    let payload = std::str::from_utf8(&data).unwrap();
                    let mut conn = recv_conn.lock().await;

                    for (k, v) in &mut conn.clients.borrow_mut().into_iter() {
                        if k.0 != self_token.0 {
                            println!("token: {:?}", k);
                            let _ = v.connection.write(payload);
                        } else {
                            println!("excluding {:?} value {:?}", k, payload);
                        }
                    }
                }
                ServerMessage::Close(token) => {
                    // client closed, remove from webserver object
                    println!("client is getting closed");
                    recv_conn.lock().await.clients.remove(&token);
                    // recv_poll.lock().await.registry().deregister(source)
                }
            }
        }
    });

    loop {
        send_poll
            .lock()
            .await
            .poll(&mut events, Some(std::time::Duration::from_millis(100)))?;
        for event in events.iter() {
            println!(
                "new event recieved token: {:?} eventreda: {:?}",
                event.token(),
                event.is_readable()
            );
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
                    send_poll.lock().await.registry().register(
                        &mut client.connection.socket,
                        token,
                        Interest::READABLE,
                    )?;
                    send_conn.lock().await.clients.insert(token, client);
                },
                token => {
                    // Maybe received an event for a TCP connection.
                    let mut mutg = send_conn.lock().await;
                    let _done = if let Some(client) = mutg.clients.get_mut(&token) {
                        if event.is_readable() {
                            let send_poll = send_poll.clone();
                            let send_conn = send_conn.clone();
                            let token = token.clone();

                            tokio::spawn(async move {
                                let _x = process_method(send_poll, send_conn, token).await;
                            });
                        } else if event.is_writable() {
                            let a = match client.connection.state {
                                ClientState::HandshakeResponse => {
                                    let poll = &mut send_poll.lock().await;
                                    client.connection.write_handshake(poll, &token)
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

async fn process_method(
    send_poll: Arc<Mutex<Poll>>,
    send_conn: Arc<Mutex<Connection>>,
    token: Token,
) -> std::io::Result<()> {
    println!("trying to aquire  {:?}", token);
    let mut new_poll = send_poll.lock().await;
    println!("already aquire {:?}", token);

    let mut send_conn = send_conn.lock().await;

    if let Some(client) = send_conn.clients.get_mut(&token) {
        tokio::select! {
            res = client.connection.read(&mut new_poll, &token) => {
                // Ok(_res) => Ok(()), // do nothing if read is successful
                // Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                //     println!("inside some client {:?}", token);

                //     Ok(if let Some(mut client) = send_conn.clients.remove(&token) {
                //         let _ = new_poll.registry().deregister(&mut client.socket)?;
                //     })
                // }

                // _ => Err(Error::new(std::io::ErrorKind::Other, "")),
                println!("client going out of scope.");
            }
        }
    };

    Ok(())
}
