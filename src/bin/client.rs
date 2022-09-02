use std::net::TcpStream;
use std::thread::JoinHandle;
use tokio::sync::mpsc::{self, Receiver};

use tokio::signal;
use websocket::sync::{Reader, Writer};
use websocket::{ClientBuilder, Message, OwnedMessage};

fn send_message(writer: &mut Writer<TcpStream>, msg: String) {
    let m = Message::text(msg);
    let _ = writer.send_message(&m);
}

struct StdInput {
    recv: Receiver<String>,
}

impl StdInput {
    fn new() -> StdInput {
        let (snd, recv) = mpsc::channel::<String>(500);
        std::thread::spawn(move || loop {
            let mut buffer = String::new();
            let _word = std::io::stdin().read_line(&mut buffer);

            let _ = snd.blocking_send(buffer);
        });

        StdInput { recv }
    }

    async fn receive_input(&mut self, sender: &mut Writer<TcpStream>) {
        loop {
            let res = self.recv.recv().await;
            send_message(sender, res.unwrap());
        }
    }
}
struct Connection {
    // builder: Client<TcpStream>,
    sender: Writer<TcpStream>,
    receiver: Reader<TcpStream>,
}
impl Connection {
    fn new(addr: &str) -> Connection {
        let cb = ClientBuilder::new(addr)
            .unwrap()
            .connect_insecure()
            .unwrap();
        let (recv, send) = cb.split().unwrap();
        Connection {
            sender: send,
            receiver: recv,
        }
    }

    fn close(sender: &mut Writer<TcpStream>) {
        let close_msg = Message::close();
        let res = sender.send_message(&close_msg);
        // println!("res {:?}", res);
    }
}

async fn receive_incoming_messages(
    mut recv: Reader<TcpStream>,
    send: mpsc::Sender<()>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        for msg in recv.incoming_messages() {
            match msg {
                Ok(OwnedMessage::Close(None)) => {
                    println!("received close message");
                    // send close message to server before breaking.
                    break;
                }
                Ok(OwnedMessage::Text(s)) => {
                    println!("message {}", s);
                }
                Err(_err) => {
                    let _ = send.blocking_send(());
                    break;
                }
                _ => break,
            }
        }
    })
}

#[tokio::main]
async fn main() {
    let mut conn = Connection::new("ws://127.0.0.1:9000");
    // let (recv, mut send) = conn.split().unwrap();
    let shutdown = signal::ctrl_c();

    let mut reader = StdInput::new();
    let (send, mut recv) = tokio::sync::mpsc::channel::<()>(10);
    let hand = receive_incoming_messages(conn.receiver, send).await;

    // capture in tokio select
    tokio::select! {
            _res = reader.receive_input(&mut conn.sender) => {
                // println!("accepting stops");
            }
            _ = shutdown => {
                // println!("captured shutdown");
                // websocket close frame.
                Connection::close(&mut conn.sender);
                let _ = hand.join();
            }
            _ = accept_message(&mut recv) => {
                // closing connection
                // println!("closing connection");
            }

    }
    // std::thread::sleep(std::time::Duration::from_secs(5));
}

async fn accept_message(recv: &mut Receiver<()>) -> std::io::Result<()> {
    loop {
        let done = match recv.recv().await {
            Some(_c) => true,
            None => false,
        };
        if done {
            return Ok(());
        }
    }
}
