use chat_service::server;
use tokio::signal;

#[tokio::main]
pub async fn main() {
    // let (tx, rx) = tokio::sync::broadcast::channel::<()>(50);
    let shutdown = signal::ctrl_c();
    // let sender = tx.clone();
    // let _x = server::run(tx).await;

    tokio::select! {
        res = server::run() => {
            println!("server returned data");
        },
        _ = shutdown => {
            println!("in final shutdown");
        }
    }
    println!("last part...");
}
