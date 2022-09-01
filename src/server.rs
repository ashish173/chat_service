use crate::client::{ClientState, WebSocketClient};
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

// Setup some tokens to allow us to identify which event is for which socket.
const SERVER: Token = Token(0);

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
    // poll: Poll,
}
unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}

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
            // poll: poll,
        }
    }
}

pub async fn run() -> io::Result<()> {
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

    println!("starting process");
    // broadcast replies to clients
    tokio::spawn(async move {
        loop {
            if let Some(rec) = rx.recv().await {
                // rec = rec;
                match rec {
                    ServerMessage::Text(ref data, self_token) => {
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
                    ServerMessage::None => {}
                }
            } else {
                // todo!()
                println!("empty event generated");
            };

            // println!("testing .... {:?}", rec);
        }
    });

    let _ = listen_events(send_poll, &mut events, &mut server, send_conn).await;

    Ok(())
}

struct Polling {
    recv: mpsc::Receiver<Arc<Mutex<Events>>>,
}

impl Polling {
    fn new(poll: Arc<Mutex<Poll>>) -> Polling {
        let (send, recv) = mpsc::channel::<Arc<Mutex<Events>>>(10);
        let events = Arc::new(Mutex::new(Events::with_capacity(100)));
        tokio::spawn(async move {
            let mut clone_events = events.lock().await;
            //TODO: the poll will timeout at 1 sec. If no timeout is passed then the
            //TODO: thread is alive waiting for readiness and thread doens't go out of
            //TODO: scope. This more of a hack. Ideally, we should abort this task using joinhandle.
            let _ = poll
                .lock()
                .await
                .poll(&mut clone_events, Some(std::time::Duration::from_secs(1)));
            let _ = send.send(events.clone()).await;
        });

        Polling { recv }
    }

    async fn recieve_event(&mut self) -> Option<Arc<Mutex<Events>>> {
        self.recv.recv().await
    }
}

pub async fn listen_events(
    send_poll: Arc<Mutex<Poll>>,
    _events: &mut Events,
    server: &mut WebSocketServer,
    send_conn: Arc<Mutex<Connection>>,
) -> std::io::Result<()> {
    let (noti, _) = tokio::sync::broadcast::channel::<()>(100);

    // TODO why doesn't the below 2 line code doesn't work but single line works?
    // let mut poll = send_poll.lock().await;
    // poll.poll(&mut events, Some(std::time::Duration::from_millis(100)))?;
    let (shutdown_notifier, mut shutdown_receiver) = mpsc::channel::<()>(10);
    loop {
        // println!("in looooop");
        let shutdown = tokio::signal::ctrl_c();

        let mut n = Polling::new(send_poll.clone());

        let res = tokio::select! {
            res = n.recieve_event() => {
                if let Some(val) = res {
                    Ok(val.clone())
                } else {
                    Err(())
                }
            }
            _ = shutdown => {
                println!("in shutdown");
                // If read takes more time then this handler will run on shutdown signal
                // TODO: The abort doesn't seem to work here. Ideally, the timeout in polling isn't needed
                // TODO: the abort should take care of closing the thread.
                let keys_lock = send_conn.lock().await;
                let keys = keys_lock.clients.keys().cloned().collect::<Vec<Token>>();
                drop(keys_lock);
                for key in keys {
                    println!("in for loop {:?}", key);
                    let _ = send_conn.lock().await.clients.remove(&key);
                }
                Err(())
            }
        };
        if res.is_err() {
            drop(shutdown_notifier);

            let _ = shutdown_receiver.recv().await;
            break; // break when all senders have been dropped
        }

        let new_events = res;
        let r = new_events.unwrap();
        let s = r.lock().await;
        for event in s.iter() {
            println!("event received {:?}", event);
            match event.token() {
                SERVER => loop {
                    // Received an event for the TCP server socket, which
                    // indicates we can accept an connection.
                    let mut client = match server.socket.accept() {
                        Ok((connection, _)) => {
                            // add this to shared object
                            WebSocketClient::new(
                                connection,
                                server.sender.clone(),
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
                    println!("token {:?}", token);
                    // Maybe received an event for a TCP connection.
                    let mut mutg = send_conn.lock().await;
                    let _done = if let Some(client) = mutg.clients.get_mut(&token) {
                        if event.is_readable() {
                            let send_poll = send_poll.clone();
                            let send_conn = send_conn.clone();
                            let token = token.clone();

                            tokio::spawn(async move {
                                let _x = process_method(send_poll, send_conn, token).await;
                                // std::thread::sleep(std::time::Duration::from_secs(5));
                                println!("in tokio spawn end, client dropped");
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
    println!("return from accept loop");
    Ok(())
}

async fn process_method(
    send_poll: Arc<Mutex<Poll>>,
    send_conn: Arc<Mutex<Connection>>,
    token: Token,
) -> std::io::Result<()> {
    // println!("trying to aquire  {:?}", token);
    let shutdown = tokio::signal::ctrl_c();
    let mut new_poll = send_poll.lock().await;
    // println!("already aquire {:?}", token);

    let mut send_conn = send_conn.lock().await;

    if let Some(client) = send_conn.clients.get_mut(&token) {
        // let v = client.connection.read(&mut new_poll, &token);
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
            },
            _ = client.shutdown.listen_shut() => {
                println!("in listen recieve");
                let c = send_conn.clients.remove(&token);
                println!("removed {:?}", c);
            }
            _ = shutdown => {
                println!("in process method shutdown");
                // drop the client; socket message
                // client.close_client_graceful();
                // drop(client);
                let cl = send_conn.clients.remove(&token);
                // this also happens automatically at the end of shutdown handler scope
                // added here just to make it explicit that the mpsc shutdown_notifier in
                // client is getting dropped sending message to shutdown receiver.
                drop(cl);
            }
        }
        println!("coming out of the read tokio select");
    };

    Ok(())
}
