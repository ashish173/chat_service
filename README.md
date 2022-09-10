## Chatting application using websockets

Features

1. Server listens to events on `mio::poll`
2. Every new event received on tcp stream is handled in a separate thread
3. Client uses `websocket` crate to establish websocket connection with server
4. Server reads the data from the stream and parses websocket header
5.
# chat_service
