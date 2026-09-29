//! A minimal WebSocket client. About 200 lines, no dependencies.
//!
//! It exists because Chrome's DevTools Protocol speaks WebSocket and nothing
//! else, and pulling in a full async runtime plus a WS crate for one localhost
//! connection would cost more than writing the framing.
//!
//! Scope is deliberately narrow: client side, plaintext, localhost. No TLS, no
//! compression, no fragmentation on send. That is all DevTools needs.

use crate::error::{AtlasError, Result};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const OP_TEXT: u8 = 0x1;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

pub struct WebSocket {
    stream: TcpStream,
    rng: u64,
}

impl WebSocket {
    /// `url` is a ws:// URL, as handed out by Chrome's /json endpoint.
    pub fn connect(url: &str, timeout: Duration) -> Result<WebSocket> {
        let rest = url
            .strip_prefix("ws://")
            .ok_or_else(|| AtlasError::Platform(format!("not a ws:// url: {url}")))?;
        let (host, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };

        let addr = host
            .to_string()
            .parse()
            .or_else(|_| {
                use std::net::ToSocketAddrs;
                host.to_socket_addrs()
                    .ok()
                    .and_then(|mut a| a.next())
                    .ok_or(())
            })
            .map_err(|_| AtlasError::Platform(format!("cannot resolve {host}")))?;

        let stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|e| AtlasError::Platform(format!("connect {host}: {e}")))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;

        let mut ws = WebSocket { stream, rng: seed() };
        ws.handshake(host, path)?;
        Ok(ws)
    }

    fn handshake(&mut self, host: &str, path: &str) -> Result<()> {
        let mut key_bytes = [0u8; 16];
        for b in key_bytes.iter_mut() {
            *b = (self.next_rand() & 0xFF) as u8;
        }
        let key = crate::b64::encode(&key_bytes);
        let req = handshake_request(host, path, &key);
        self.stream.write_all(req.as_bytes())?;

        // Read headers only — the response body of a 101 is the WS stream.
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            let n = self.stream.read(&mut byte)?;
            if n == 0 {
                return Err(AtlasError::Platform("server closed during handshake".into()));
            }
            head.push(byte[0]);
            if head.len() > 8192 {
                return Err(AtlasError::Platform("handshake response too large".into()));
            }
        }
        let text = String::from_utf8_lossy(&head);
        if !text.starts_with("HTTP/1.1 101") {
            let line = text.lines().next().unwrap_or("").to_string();
            return Err(AtlasError::Platform(format!("websocket upgrade refused: {line}")));
        }
        Ok(())
    }

    fn next_rand(&mut self) -> u64 {
        // xorshift64. Masking keys need to vary, not to be cryptographic.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    pub fn send_text(&mut self, payload: &str) -> Result<()> {
        let mut mask = [0u8; 4];
        for b in mask.iter_mut() {
            *b = (self.next_rand() & 0xFF) as u8;
        }
        let frame = encode_frame(OP_TEXT, payload.as_bytes(), mask);
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Next text message. Pings are answered and control frames skipped, so
    /// callers only ever see application data.
    pub fn recv_text(&mut self) -> Result<String> {
        loop {
            let (opcode, payload) = self.read_frame()?;
            match opcode {
                OP_TEXT => return Ok(String::from_utf8_lossy(&payload).to_string()),
                OP_PING => {
                    let mut mask = [0u8; 4];
                    for b in mask.iter_mut() {
                        *b = (self.next_rand() & 0xFF) as u8;
                    }
                    let pong = encode_frame(OP_PONG, &payload, mask);
                    self.stream.write_all(&pong)?;
                }
                OP_CLOSE => return Err(AtlasError::Platform("websocket closed by peer".into())),
                _ => continue,
            }
        }
    }

    fn read_frame(&mut self) -> Result<(u8, Vec<u8>)> {
        let mut h = [0u8; 2];
        self.stream.read_exact(&mut h)?;
        let opcode = h[0] & 0x0F;
        let masked = h[1] & 0x80 != 0;
        let len = match h[1] & 0x7F {
            126 => {
                let mut b = [0u8; 2];
                self.stream.read_exact(&mut b)?;
                u16::from_be_bytes(b) as usize
            }
            127 => {
                let mut b = [0u8; 8];
                self.stream.read_exact(&mut b)?;
                u64::from_be_bytes(b) as usize
            }
            n => n as usize,
        };
        if len > 64 * 1024 * 1024 {
            return Err(AtlasError::Platform("websocket frame absurdly large".into()));
        }

        let mut mask = [0u8; 4];
        if masked {
            self.stream.read_exact(&mut mask)?;
        }
        let mut payload = vec![0u8; len];
        self.stream.read_exact(&mut payload)?;
        if masked {
            unmask(&mut payload, mask);
        }
        Ok((opcode, payload))
    }

    pub fn close(&mut self) {
        let mut mask = [0u8; 4];
        for b in mask.iter_mut() {
            *b = (self.next_rand() & 0xFF) as u8;
        }
        let _ = self.stream.write_all(&encode_frame(OP_CLOSE, &[], mask));
    }
}

pub fn handshake_request(host: &str, path: &str, key: &str) -> String {
    format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n"
    )
}

/// Client frames are always masked, per RFC 6455.
pub fn encode_frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | opcode); // FIN set, single frame
    let n = payload.len();
    if n < 126 {
        out.push(0x80 | n as u8);
    } else if n <= u16::MAX as usize {
        out.push(0x80 | 126);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(n as u64).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    for (i, b) in payload.iter().enumerate() {
        out.push(b ^ mask[i % 4]);
    }
    out
}

pub fn unmask(payload: &mut [u8], mask: [u8; 4]) {
    for (i, b) in payload.iter_mut().enumerate() {
        *b ^= mask[i % 4];
    }
}

fn seed() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545F4914F6CDD1D);
    t | 1
}
