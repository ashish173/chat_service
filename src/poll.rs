use std::sync::Arc;

use mio::{Events, Poll};
use tokio::sync::{mpsc, Mutex};

pub struct Polling {
    recv: mpsc::Receiver<Arc<Mutex<Events>>>,
}

impl Polling {
    pub fn new(poll: Arc<Mutex<Poll>>) -> Polling {
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

    pub async fn recieve_event(&mut self) -> Option<Arc<Mutex<Events>>> {
        self.recv.recv().await
    }
}
