use chat_service::server;

#[tokio::main]
pub async fn main() {
    let _x = server::run().await;
    println!("server shutdown!");
}
