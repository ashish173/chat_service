use clap::Parser;
use std::net::TcpStream;
use websocket::sync::{Client, Writer};
use websocket::{ClientBuilder, Message, OwnedMessage};

fn create_connection() -> Client<std::net::TcpStream> {
    let client = ClientBuilder::new("ws://localhost:9000")
        .unwrap()
        .connect_insecure()
        .unwrap();
    client
}

fn send_message(writer: &mut Writer<TcpStream>, msg: String) {
    let m = Message::text(msg);
    let _ = writer.send_message(&m);
}

fn _close(_conn: &mut Client<TcpStream>) {
    // send close request
}

#[derive(Parser, Debug)]
struct Cli {
    name: String,
}

fn main() -> std::io::Result<()> {
    // command
    let conn = create_connection();
    let (mut rcv, mut snd) = conn.split().unwrap();

    std::thread::spawn(move || {
        for msg in rcv.incoming_messages() {
            match msg {
                Ok(OwnedMessage::Text(mut ress)) => {
                    let resss = ress.len();
                    ress.truncate(resss - 1);
                    println!("{:?}", ress);
                }
                Err(_err) => {
                    println!("Server Stopped");
                    break;
                }
                _ => {}
            }
        }
    });

    loop {
        let mut buffer = String::new();
        // TODO capture ctrl+c , close socket connection and Exit.
        let _word = std::io::stdin().read_line(&mut buffer)?;
        // capture in tokio select
        send_message(&mut snd, buffer);
    }
}
