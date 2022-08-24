use std::net::TcpStream;
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
        println!("res {:?}", res);
    }
}

fn receive_incoming_messages(mut recv: Reader<TcpStream>) {
    std::thread::spawn(move || {
        for msg in recv.incoming_messages() {
            match msg {
                Ok(OwnedMessage::Close(None)) => {
                    println!("received close message");
                    break;
                }
                Ok(OwnedMessage::Text(s)) => {
                    println!("message {}", s);
                }
                Err(_err) => return,
                _ => break,
            }
        }
    });
}

#[tokio::main]
async fn main() {
    let mut conn = Connection::new("ws://127.0.0.1:9000");
    // let (recv, mut send) = conn.split().unwrap();
    let shutdown = signal::ctrl_c();

    receive_incoming_messages(conn.receiver);

    let mut reader = StdInput::new();

    // capture in tokio select
    tokio::select! {
            _res = reader.receive_input(&mut conn.sender) => {
                println!("accepting stops");
            }
            _ = shutdown => {
                println!("captured shutdown");
                // websocket close frame.
                Connection::close(&mut conn.sender);
            }
    }
}
