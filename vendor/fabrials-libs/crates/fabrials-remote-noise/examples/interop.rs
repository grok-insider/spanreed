use base64::{Engine, engine::general_purpose::STANDARD};
use fabrials_remote_noise::{Handshake, identity};
use serde_json::json;
use std::io::{BufRead, Write};

fn output(value: serde_json::Value) {
    println!("{value}");
    std::io::stdout().flush().unwrap();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = identity()?;
    output(json!({"publicKey":STANDARD.encode(&key.public)}));
    let mut lines = std::io::stdin().lock().lines();
    let mut read = || -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let line = lines.next().ok_or("input_closed")??;
        if line.len() > 90 * 1024 {
            return Err("input_limit".into());
        }
        let body: serde_json::Value = serde_json::from_str(&line)?;
        Ok(STANDARD.decode(body["frame"].as_str().ok_or("invalid_input")?)?)
    };
    let mut handshake = Handshake::new(&key.private, None, "interop-host", "interop-channel")?;
    assert!(handshake.read(&read()?)?.is_empty());
    output(json!({"frame":STANDARD.encode(handshake.write(&[])?) }));
    assert_eq!(&*handshake.read(&read()?)?, b"synthetic-pairing");
    let mut channel = handshake.finish()?;
    output(json!({"ready":true}));
    loop {
        let frame = read()?;
        if let Some(message) = channel.decrypt(&frame)? {
            for response in channel.encrypt(&message)? {
                output(json!({"frame":STANDARD.encode(response)}));
            }
        }
    }
}
