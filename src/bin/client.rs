use std::net::TcpStream;
use std::thread::JoinHandle;
use tokio::sync::mpsc::{self, Receiver};

use tokio::signal;
use tokio::sync::oneshot;
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
        let (send, recv) = mpsc::channel::<String>(500);
        std::thread::spawn(move || loop {
            let mut buffer = String::new();
            let _word = std::io::stdin().read_line(&mut buffer);

            let _ = send.blocking_send(buffer);
        });

        StdInput { recv }
    }

    async fn receive_input(&mut self, sender: &mut mpsc::Sender<NotifyFromMessageRecieved>) {
        loop {
            let res = self.recv.recv().await;
            let _ = sender
                .send(NotifyFromMessageRecieved::TextMessage(res.unwrap()))
                .await;
        }
    }
}
struct Connection {
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
        let _res = sender.send_message(&close_msg);
    }
}

async fn receive_incoming_messages(
    mut recv: Reader<TcpStream>,
    send: mpsc::Sender<NotifyFromMessageRecieved>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        for msg in recv.incoming_messages() {
            match msg {
                Ok(OwnedMessage::Close(None)) => {
                    // send close message to server before breaking.
                    let _ = send.blocking_send(NotifyFromMessageRecieved::ServerClosing(true));
                    break;
                }
                Ok(OwnedMessage::Text(s)) => {
                    println!("message {}", s);
                }
                Err(_err) => {
                    let _ = send.blocking_send(NotifyFromMessageRecieved::ServerClosing(true));
                    break;
                }
                _ => break,
            }
        }
    })
}

#[derive(Debug)]
enum NotifyFromMessageRecieved {
    TextMessage(String),
    ServerClosing(bool), // true when responding to close message, false when initiating close
}

#[tokio::main]
async fn main() {
    let conn = Connection::new("ws://127.0.0.1:9000");
    let shutdown = signal::ctrl_c();

    let mut reader = StdInput::new();
    let (send, mut recv) = tokio::sync::mpsc::channel::<NotifyFromMessageRecieved>(10);
    // let sender_clone = send.clone();
    let hand = receive_incoming_messages(conn.receiver, send.clone()).await;
    let mut send_clone = send.clone();
    // capture in tokio select
    let (final_shutdown_sender, final_receiver) = tokio::sync::oneshot::channel::<()>();

    // listens to any close client message from tcp receiver
    tokio::spawn(async move {
        let _ = accept_message(&mut recv, conn.sender).await;
        let _ = final_shutdown_sender.send(());
        // send message to stop listening
    });

    tokio::select! {
            _res = reader.receive_input(&mut send_clone) => {}
            _ = shutdown => { // when ctrl+c is pressed on client
                let res = send.send(NotifyFromMessageRecieved::ServerClosing(false)).await;
                let _ = hand.join();
            }
            _ = close(final_receiver) => {
                println!("final closing");
            } // when server terminates

    }
}

async fn close(final_receiver: tokio::sync::oneshot::Receiver<()>) -> std::io::Result<()> {
    let _ = final_receiver.await;
    Ok(())
}

async fn accept_message(
    recv: &mut Receiver<NotifyFromMessageRecieved>,
    mut sender: Writer<TcpStream>,
) -> std::io::Result<()> {
    loop {
        let done = match recv.recv().await {
            Some(message) => {
                // close message to server
                let close = match message {
                    NotifyFromMessageRecieved::ServerClosing(closing) => {
                        // send message
                        Connection::close(&mut sender);
                        if closing {
                            true
                        } else {
                            false
                        }
                    }
                    NotifyFromMessageRecieved::TextMessage(msg) => {
                        send_message(&mut sender, msg);
                        false
                    }
                };
                close
            }
            None => false,
        };
        if done {
            return Ok(());
        }
    }
}
