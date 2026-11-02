//! Messages from the runner to a GUI node process, sent over the child's stdin.
//!
//! Wire format: each message is `u32 LE length` followed by that many bytes, which start with a
//! one-byte tag:
//! - `1` Frame: `u32 LE width`, `u32 LE height`, then `width * height * 4` straight-alpha RGBA bytes
//! - `2` Error: UTF-8 message
//! - `3` Close
//!
//! Hand-rolled rather than using a serialization crate: it's three messages, and frames can be
//! written straight from the image buffer without copying.

use std::io::{self, ErrorKind, Read, Write};

/// Upper bound on a single message, so a corrupt length can't trigger a huge allocation.
pub const MAX_MESSAGE_LEN: u32 = 1 << 30;

const TAG_FRAME: u8 = 1;
const TAG_ERROR: u8 = 2;
const TAG_CLOSE: u8 = 3;

#[derive(Clone, PartialEq, Eq)]
pub enum Msg {
    Frame {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    Error(String),
    Close,
}

// Manual impl: deriving would dump every pixel.
impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Msg::Frame { width, height, .. } => write!(f, "Frame({width}x{height})"),
            Msg::Error(e) => f.debug_tuple("Error").field(e).finish(),
            Msg::Close => f.write_str("Close"),
        }
    }
}

pub fn write_frame(w: &mut impl Write, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    if rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        return Err(invalid("frame size does not match width * height * 4"));
    }
    let len = message_len(1 + 8 + rgba.len())?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&[TAG_FRAME])?;
    w.write_all(&width.to_le_bytes())?;
    w.write_all(&height.to_le_bytes())?;
    w.write_all(rgba)?;
    w.flush()
}

pub fn write_error(w: &mut impl Write, message: &str) -> io::Result<()> {
    let len = message_len(1 + message.len())?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&[TAG_ERROR])?;
    w.write_all(message.as_bytes())?;
    w.flush()
}

pub fn write_close(w: &mut impl Write) -> io::Result<()> {
    w.write_all(&1u32.to_le_bytes())?;
    w.write_all(&[TAG_CLOSE])?;
    w.flush()
}

/// Reads the next message. Returns `Ok(None)` on a clean end of stream (sender went away).
pub fn read_msg(r: &mut impl Read) -> io::Result<Option<Msg>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len);
    if len == 0 || len > MAX_MESSAGE_LEN {
        return Err(invalid(format!("bad message length {len}")));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;

    // The payload follows the tag byte; reuse `body` for it instead of allocating a copy.
    let tag = body[0];
    match tag {
        TAG_FRAME => {
            let header = body
                .get(1..9)
                .ok_or_else(|| invalid("frame header truncated"))?;
            let width = u32::from_le_bytes(header[..4].try_into().expect("4 bytes"));
            let height = u32::from_le_bytes(header[4..].try_into().expect("4 bytes"));
            body.drain(..9);
            let rgba = body;
            if rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
                return Err(invalid("frame size does not match width * height * 4"));
            }
            Ok(Some(Msg::Frame {
                width,
                height,
                rgba,
            }))
        }
        TAG_ERROR => {
            body.remove(0);
            let message =
                String::from_utf8(body).map_err(|_| invalid("error message is not UTF-8"))?;
            Ok(Some(Msg::Error(message)))
        }
        TAG_CLOSE => Ok(Some(Msg::Close)),
        other => Err(invalid(format!("unknown message tag {other}"))),
    }
}

fn message_len(len: usize) -> io::Result<u32> {
    u32::try_from(len)
        .ok()
        .filter(|l| *l <= MAX_MESSAGE_LEN)
        .ok_or_else(|| invalid("message too large"))
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_messages_in_sequence() {
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|i| i as u8).collect();
        let mut buf = Vec::new();
        write_frame(&mut buf, 2, 3, &rgba).unwrap();
        write_error(&mut buf, "comp.toml: oops ✗").unwrap();
        write_close(&mut buf).unwrap();

        let mut r = buf.as_slice();
        assert_eq!(
            read_msg(&mut r).unwrap(),
            Some(Msg::Frame {
                width: 2,
                height: 3,
                rgba
            })
        );
        assert_eq!(
            read_msg(&mut r).unwrap(),
            Some(Msg::Error("comp.toml: oops ✗".into()))
        );
        assert_eq!(read_msg(&mut r).unwrap(), Some(Msg::Close));
        assert_eq!(read_msg(&mut r).unwrap(), None);
    }

    #[test]
    fn rejects_mismatched_frame_size() {
        assert!(write_frame(&mut Vec::new(), 2, 2, &[0; 15]).is_err());
    }

    #[test]
    fn rejects_corrupt_input() {
        // Truncated body.
        let mut buf = Vec::new();
        write_error(&mut buf, "hello").unwrap();
        buf.pop();
        assert!(read_msg(&mut buf.as_slice()).is_err());

        // Unknown tag, zero length, oversized length.
        assert!(read_msg(&mut [1, 0, 0, 0, 9].as_slice()).is_err());
        assert!(read_msg(&mut [0, 0, 0, 0].as_slice()).is_err());
        assert!(read_msg(&mut u32::MAX.to_le_bytes().as_slice()).is_err());

        // Frame header says 2x1 (8 bytes of pixels) but only 4 bytes follow.
        let mut bad = vec![];
        bad.extend(13u32.to_le_bytes());
        bad.push(TAG_FRAME);
        bad.extend(2u32.to_le_bytes());
        bad.extend(1u32.to_le_bytes());
        bad.extend([0u8; 4]);
        assert!(read_msg(&mut bad.as_slice()).is_err());
    }
}
