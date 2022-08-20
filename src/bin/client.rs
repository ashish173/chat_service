use clap::Parser;
use std::env;
use std::net::TcpStream;
use websocket::sync::{Client, Reader, Writer};
use websocket::{ClientBuilder, Message};

fn create_connection() -> Client<std::net::TcpStream> {
    let mut client = ClientBuilder::new("ws://localhost:9000")
        .unwrap()
        .connect_insecure()
        .unwrap();
    client
    // Client//
}

fn send_message(writer: &mut Writer<TcpStream>, msg: String) {
    let m = Message::text(msg);

    // let res = conn.send_message(&m);
    let _ = writer.send_message(&m);
}

fn close(conn: &mut Client<TcpStream>) {
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

    // receive_thread(&mut rcv);
    std::thread::spawn(move || {
        for msg in rcv.incoming_messages() {
            println!("{:?}", msg)
        }
    });

    loop {
        let mut buffer = String::new();
        let word = std::io::stdin().read_line(&mut buffer)?;
        // println!("{}", buffer);
        send_message(&mut snd, buffer);
    }
    Ok(())
}
