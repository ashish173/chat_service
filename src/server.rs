use crate::client::{ClientState, WebSocketClient};
use crate::poll::{self, Polling};
use mio::event::Event;
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::sync::broadcast::Sender;
use tokio::sync::{mpsc, Mutex};

// Setup some tokens to allow us to identify which event is for which socket.
const SERVER: Token = Token(0);

// Message from client to server to broadcast messages
#[derive(Debug)]
pub enum ServerMessage {
    Text(Vec<u8>, Token),
    Close(Token),
    None,
}

struct Shared {
    connection: Arc<Mutex<Connection>>,
    poll: Arc<Mutex<Poll>>,
}

pub struct Connection {
    clients: HashMap<Token, WebSocketClient>,
}

pub struct WebSocketServer {
    socket: TcpListener,
    token_counter: usize,
    sender: mpsc::Sender<ServerMessage>,
}

impl Shared {
    fn new(poll: Poll) -> Shared {
        Shared {
            connection: Arc::new(Mutex::new(Connection::new())),
            poll: Arc::new(Mutex::new(poll)),
        }
    }
}

impl Connection {
    fn new() -> Connection {
        Connection {
            clients: HashMap::new(),
        }
    }
}

impl WebSocketServer {
    fn new(socket: TcpListener, recv_conn: Arc<Mutex<Connection>>) -> WebSocketServer {
        let (tx, rx) = mpsc::channel::<ServerMessage>(1);

        listen_incoming_message(recv_conn, rx);

        WebSocketServer {
            socket,
            token_counter: SERVER.0 + 1,
            sender: tx,
        }
    }

    fn next(&mut self) -> Token {
        let next = self.token_counter;
        self.token_counter += 1;
        Token(next)
    }

    // polls to check for new events appeared on sources(TcpStreams)
    // listens for ctrl+c signal and drops all clients(mpsc::Sender)
    // thus paving the way for graceful shutdown of server
    async fn get_events(
        send_poll: Arc<Mutex<Poll>>,
        send_conn: Arc<Mutex<Connection>>,
    ) -> Option<std::io::Result<Arc<Mutex<Events>>>> {
        let shutdown = tokio::signal::ctrl_c();

        let mut polling = Polling::new(send_poll);

        let res = tokio::select! {
            res = polling.recieve_event() => {
                if let Some(val) = res {
                    Ok(val.clone())
                } else {
                    Err(std::io::Error::new(std::io::ErrorKind::Other , "error"))
                }
            }
            _ = shutdown => {
                // Doesn't work here
                // polling.handle.abort();
                println!("shutdown starting...");
                // If read takes more time then this handler will run on shutdown signal
                // TODO: The abort doesn't seem to work here. Ideally, the timeout in polling isn't needed
                // TODO: the abort should take care of closing the thread.
                let keys_lock = send_conn.lock().await;
                let keys = keys_lock.clients.keys().cloned().collect::<Vec<Token>>();

                // drop manually for the lock in for loop to work
                // since the lifetime of keys_lock is valid for the whole scope of shutdown handler
                drop(keys_lock);

                // This way is highly inefficient way of closing the clients.
                // ideally, the clients should listen to broadcast sender drop for
                // server and drop themselves somehow.
                for key in keys {
                    let _ = send_conn.lock().await.clients.remove(&key);
                }
                return None;

            }
        };

        Some(res)
    }

    pub async fn accept_connections(
        &mut self,
        noti: Sender<()>,
        shutdown_notifier: mpsc::Sender<()>,
        send_poll: Arc<Mutex<Poll>>,
        send_conn: Arc<Mutex<Connection>>,
    ) -> std::io::Result<()> {
        loop {
            let mut client = match self.socket.accept() {
                Ok((connection, _)) => {
                    // add this to shared object
                    WebSocketClient::new(
                        connection,
                        self.sender.clone(),
                        noti.clone(), // cloning to avoid moved value in loop
                        shutdown_notifier.clone(),
                    )
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // If we get a `WouldBlock` error we know our
                    // listener has no more incoming connections queued,
                    // so we can return to polling and wait for some
                    // more.
                    break;
                }
                Err(_e) => {
                    // If it was any other kind of error, something went
                    // wrong and we terminate with an error.
                    // return Err(e);
                    break;
                }
            };
            let token = self.next();
            send_poll.lock().await.registry().register(
                &mut client.connection.socket,
                token,
                Interest::READABLE,
            )?;
            let _ = send_conn.lock().await.clients.insert(token, client);
        }
        Ok(())
    }

    // Handles the client events
    async fn process_client_events(
        event: &Event,
        token: Token,
        send_conn: Arc<Mutex<Connection>>,
        send_poll: Arc<Mutex<Poll>>,
    ) {
        let mut mutg = send_conn.lock().await;
        if let Some(client) = mutg.clients.get_mut(&token) {
            if event.is_readable() {
                let send_conn = send_conn.clone();

                tokio::spawn(async move {
                    let _ = process_event(send_poll, send_conn, token).await;
                });
            } else if event.is_writable() {
                let write = match client.connection.state {
                    ClientState::HandshakeResponse => {
                        let poll = &mut send_poll.lock().await;
                        client.connection.write_handshake(poll, &token)
                    }
                    _ => Ok(()),
                };
                match write {
                    Ok(()) => {}
                    Err(e) => {
                        println!("err -> {:?}", e);
                    }
                }
            }
        };
    }

    // listens for new events on the poll
    pub async fn listen_and_handle_events(
        &mut self,
        send_poll: Arc<Mutex<Poll>>,
        send_conn: Arc<Mutex<Connection>>,
    ) -> std::io::Result<()> {
        let (noti, _) = tokio::sync::broadcast::channel::<()>(100);
        let (shutdown_notifier, mut shutdown_receiver) = mpsc::channel::<()>(10);

        loop {
            let res = WebSocketServer::get_events(send_poll.clone(), send_conn.clone()).await;
            if let None = res {
                drop(shutdown_notifier); // drop the first notifier
                println!("waiting for client to drop");
                let _ = shutdown_receiver.recv().await;
                println!("clients dropped");
                break; // stop the program
            }

            // Need to bind arc to local var to set the lifetime
            let events_arc = res.unwrap().unwrap();
            // ensures deref at lock to transfer lifetime to mutexguard
            // https://stackoverflow.com/a/73578894/1930053
            let guard = events_arc.lock().await;

            for event in guard.iter() {
                // println!("event received {:?}", event);
                match event.token() {
                    SERVER => {
                        let _ = self
                            .accept_connections(
                                noti.clone(),
                                shutdown_notifier.clone(),
                                send_poll.clone(),
                                send_conn.clone(),
                            )
                            .await;
                    }
                    token => {
                        WebSocketServer::process_client_events(
                            event,
                            token,
                            send_conn.clone(),
                            send_poll.clone(),
                        )
                        .await;
                    }
                }
            }
        }
        Ok(())
    }
}

pub async fn run() -> io::Result<()> {
    let poll = Poll::new()?;
    let shared = Shared::new(poll);

    // Setup the TCP server socket.
    let addr = "127.0.0.1:9000".parse().unwrap();
    let mut server = TcpListener::bind(addr)?;

    // used in a thread while listening to messages from clients
    let recv_conn = shared.connection.clone();
    let send_conn = shared.connection.clone();
    shared
        .poll
        .lock()
        .await
        .registry()
        .register(&mut server, SERVER, Interest::READABLE)?;

    let mut server = WebSocketServer::new(server, recv_conn);

    let _res = server
        .listen_and_handle_events(shared.poll, send_conn)
        .await;

    Ok(())
}

async fn process_event(
    send_poll: Arc<Mutex<Poll>>,
    send_conn: Arc<Mutex<Connection>>,
    token: Token,
) -> std::io::Result<()> {
    let shutdown = tokio::signal::ctrl_c();
    let mut new_poll = send_poll.lock().await;
    let mut send_conn = send_conn.lock().await;

    if let Some(client) = send_conn.clients.get_mut(&token) {
        tokio::select! {
            _res = client.connection.read(&mut new_poll, &token) => {},
            _ = shutdown => {
                // if read suspends for a long time, then ctrl+c signal gets
                // captured in this branch
                let cl = send_conn.clients.remove(&token);
                // this also happens automatically at the end of shutdown handler scope
                // added here just to make it explicit that the mpsc shutdown_notifier in
                // client is getting dropped sending message to shutdown receiver.
                drop(cl);
            }
        }
    };
    Ok(())
}

// listens to any messages from clients and broadcasts it
fn listen_incoming_message(
    recv_conn: Arc<Mutex<Connection>>,
    mut rx: mpsc::Receiver<ServerMessage>,
) {
    tokio::spawn(async move {
        loop {
            if let Some(rec) = rx.recv().await {
                match rec {
                    ServerMessage::Text(ref data, self_token) => {
                        let payload = std::str::from_utf8(&data).unwrap();
                        let mut conn = recv_conn.lock().await;

                        for (k, v) in &mut conn.clients.borrow_mut().into_iter() {
                            if k.0 != self_token.0 {
                                // println!("token: {:?}", k);
                                let _ = v.connection.write(payload);
                            } else {
                                // println!("excluding {:?} value {:?}", k, payload);
                            }
                        }
                    }
                    ServerMessage::Close(token) => {
                        // client closed, remove from webserver object
                        // TODO final drop for client
                        let _ = recv_conn.lock().await.clients.remove(&token);
                    }
                    ServerMessage::None => {}
                }
            } else {
                // todo!()
            };
        }
    });
}
