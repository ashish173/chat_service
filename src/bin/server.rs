use chat_service::server;
use tokio::signal;

#[tokio::main]
pub async fn main() {
    let _x = server::run().await;
}
