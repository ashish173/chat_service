extern crate sha1;
use rustc_serialize::base64::{ToBase64, STANDARD};
use sha1::Digest;

pub fn gen_key(key: &String) -> String {
    let mut m = sha1::Sha1::new();
    let mut _buf = [0u8; 20];

    let data = [
        key.as_bytes(),
        "258EAFA5-E914-47DA-95CA-C5AB0DC85B11".as_bytes(), // appending a key
    ]
    .concat();
    m.update(data);

    let _len = m.finalize();

    return _len.to_base64(STANDARD);
}
