//! Offline P7a framing experiment; not a Cast sender or authentication verifier.
//! Run: cargo test --locked --example cast_frame_probe
//! The future transport must additionally enforce whole-operation deadlines and
//! authenticate the receiver before sending any application command or media URL.

use std::io::{self, Read, Write};

// An application policy, not a claim about the protocol's maximum or compatibility
// with every receiver/CRL. Reject larger frames rather than allocating from the wire.
const MAX_FRAME: usize = 64 * 1024;

fn check_length(length: usize) -> io::Result<()> {
    if length == 0 || length > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Cast frame length is outside the private probe limit",
        ));
    }
    Ok(())
}

fn read_frame(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    check_length(length)?;
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

fn write_frame(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    check_length(payload.len())?;
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(payload)
}

fn main() -> io::Result<()> {
    let mut bytes = Vec::new();
    write_frame(
        &mut bytes,
        b"private framing fixture; not a protobuf message",
    )?;
    let decoded = read_frame(&mut bytes.as_slice())?;
    assert_eq!(decoded, b"private framing fixture; not a protobuf message");
    println!("Offline frame round trip passed; no network or receiver authentication performed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn length_is_checked_before_payload_read_or_allocation() {
        struct HeaderOnly {
            header: Cursor<[u8; 4]>,
        }
        impl Read for HeaderOnly {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                assert!(
                    self.header.position() < 4,
                    "invalid length reached payload read"
                );
                self.header.read(out)
            }
        }
        for length in [0, MAX_FRAME as u32 + 1, u32::MAX] {
            let mut source = HeaderOnly {
                header: Cursor::new(length.to_be_bytes()),
            };
            assert_eq!(
                read_frame(&mut source).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn exact_limit_round_trips_without_consuming_the_next_frame() {
        let payload = vec![0x91; MAX_FRAME];
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &payload).unwrap();
        write_frame(&mut bytes, b"next").unwrap();
        let mut source = bytes.as_slice();
        assert_eq!(read_frame(&mut source).unwrap(), payload);
        assert_eq!(source.len(), 8);
        assert_eq!(read_frame(&mut source).unwrap(), b"next");
        assert!(source.is_empty());
    }

    #[test]
    fn fragmented_header_and_payload_are_reassembled() {
        struct OneByte(Cursor<Vec<u8>>);
        impl Read for OneByte {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let count = out.len().min(1);
                self.0.read(&mut out[..count])
            }
        }
        let mut bytes = Vec::new();
        write_frame(&mut bytes, b"fragmented").unwrap();
        assert_eq!(
            read_frame(&mut OneByte(Cursor::new(bytes))).unwrap(),
            b"fragmented"
        );
    }

    #[test]
    fn every_truncated_prefix_fails_closed() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, b"payload").unwrap();
        for end in 0..bytes.len() {
            assert_eq!(
                read_frame(&mut &bytes[..end]).unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
    }

    #[test]
    fn invalid_outbound_length_writes_nothing() {
        for payload in [Vec::new(), vec![0; MAX_FRAME + 1]] {
            let mut bytes = Vec::new();
            assert_eq!(
                write_frame(&mut bytes, &payload).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert!(bytes.is_empty());
        }
    }

    #[test]
    fn transport_deadline_error_is_not_retried() {
        struct TimedOut;
        impl Read for TimedOut {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::TimedOut.into())
            }
        }
        assert_eq!(
            read_frame(&mut TimedOut).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn partial_write_failure_is_reported_without_restarting_a_frame() {
        struct Broken {
            writes: usize,
        }
        impl Write for Broken {
            fn write(&mut self, input: &[u8]) -> io::Result<usize> {
                self.writes += 1;
                if self.writes == 1 {
                    assert_eq!(input, [0, 0, 0, 4]);
                    Ok(4)
                } else {
                    Err(io::ErrorKind::BrokenPipe.into())
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Broken { writes: 0 };
        assert_eq!(
            write_frame(&mut sink, b"data").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(sink.writes, 2);
    }
}
