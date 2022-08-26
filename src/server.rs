use crate::client::{ClientState, WebSocketClient};
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
    None,
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

pub struct Connection {
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
use tokio::sync::broadcast::Sender;

pub async fn fun(notifier: Sender<()>) -> io::Result<()> {
    let addr = "127.0.0.1:9000".parse().unwrap();
    let mut server = TcpListener::bind(addr)?;
    // loop {
    //     let (socket, _) = server.accept()?;
    // }
    let mut listen = notifier.subscribe();
    println!("before message");
    if let Ok(res) = listen.recv().await {
        println!("res {:?}", res);
    }
    println!("after message");

    Ok(())
}

// #[tokio::main]
// #[cfg(not(target_os = "wasi"))]
pub async fn run() -> io::Result<()> {
    // use tokio::sync::broadcast::Sender;
    let (notifier, shutdown_receiver) = tokio::sync::broadcast::channel::<()>(100);

    use crate::client::ClientState;
    use std::borrow::BorrowMut;
    let shutdown = tokio::signal::ctrl_c();

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

    let _ = listen_events(send_poll, &mut events, &mut server, notifier, send_conn).await;

    // tokio::select! {
    //     res = listen_events(send_poll, &mut events, &mut server, notifier, send_conn) => {
    //         println!("return from god {:?}", res);
    //         // return Ok(())
    //     },
    //     _ = shutdown => {
    //         println!("real deal");
    //     }
    // }
    // drop(notify_shutdown)
    // drop(notify)
    // recv.await // err when all senders are closed
    // final close
    println!("I am returning");
    Ok(())
}

struct Polling {
    recv: mpsc::Receiver<Arc<Mutex<Events>>>,
}

impl Polling {
    fn new(poll: Arc<Mutex<Poll>>) -> Polling {
        // mpsc
        let (send, recv) = mpsc::channel::<Arc<Mutex<Events>>>(10);
        let mut events = Arc::new(Mutex::new(Events::with_capacity(100)));
        // new thread
        tokio::spawn(async move {
            // loop {
            let mut clone_events = events.lock().await;
            let _ = poll.lock().await.poll(&mut clone_events, None);
            // let event_clone = events.cl
            let _ = send.send(events.clone()).await;
            // }
        });

        Polling { recv }
    }

    async fn recieve_event(&mut self) -> Arc<Mutex<Events>> {
        self.recv.recv().await.unwrap()
        // r.lock().await.into()
    }
}

pub async fn listen_events(
    send_poll: Arc<Mutex<Poll>>,
    mut events: &mut Events,
    mut server: &mut WebSocketServer,
    notifier: Sender<()>,
    send_conn: Arc<Mutex<Connection>>,
) -> std::io::Result<()> {
    let (noti, _) = tokio::sync::broadcast::channel::<()>(100);

    // TODO why doesn't the below 2 line code doesn't work but single line works?
    // let mut poll = send_poll.lock().await;
    // poll.poll(&mut events, Some(std::time::Duration::from_millis(100)))?;
    let (shutdown_notifier, mut shutdown_receiver) = mpsc::channel::<()>(10);
    loop {
        println!("in looooop");
        let shutdown = tokio::signal::ctrl_c();

        let mut n = Polling::new(send_poll.clone());

        let res = tokio::select! {
            res = n.recieve_event() => {
                // println!("got events {:?}", res);
                Ok(res.clone())
            }
            _ = shutdown => {
                println!("in shutdown");
                Err(())
                // break;
            }
        };

        println!("result: {:?}", res);
        if res.is_err() {
            println!("dropping notifier");
            drop(notifier);
            drop(shutdown_notifier);

            let mes = shutdown_receiver.recv().await;
            println!("finally all senders are dropped");

            // drop(n);
            break;
        }
        println!("break didn't work");
        // send_poll
        //     .lock()
        //     .await
        //     .poll(&mut events, Some(std::time::Duration::from_millis(100)))?;
        let new_events = res;
        let r = new_events.unwrap();
        let s = r.lock().await;
        for event in s.iter() {
            println!("event received");
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
    // println!("return from accept loop");
    Ok(())
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
        }
        println!("coming out of the read tokio select");
    };

    Ok(())
}
