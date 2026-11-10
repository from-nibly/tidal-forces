//! Retain partial application frames across read deadlines on the same TLS stream.
use std::io::{self, Read};

#[derive(Default)]
pub(super) struct Frames {
    header: [u8; 4],
    header_read: usize,
    payload: Vec<u8>,
    payload_read: usize,
}
#[derive(Debug)]
pub(super) enum Error {
    Read(io::ErrorKind),
    Bounds,
}
fn fill(reader: &mut impl Read, bytes: &mut [u8], position: &mut usize) -> Result<(), Error> {
    while *position < bytes.len() {
        match reader.read(&mut bytes[*position..]) {
            Ok(0) => return Err(Error::Read(io::ErrorKind::UnexpectedEof)),
            Ok(count) => *position += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(Error::Read(error.kind())),
        }
    }
    Ok(())
}
impl Frames {
    pub(super) fn read(&mut self, reader: &mut impl Read) -> Result<Vec<u8>, Error> {
        fill(reader, &mut self.header, &mut self.header_read)?;
        let size = u32::from_be_bytes(self.header) as usize;
        if size == 0 || size > super::MAX_MESSAGE {
            return Err(Error::Bounds);
        }
        self.payload.resize(size, 0);
        fill(reader, &mut self.payload, &mut self.payload_read)?;
        self.header_read = 0;
        self.payload_read = 0;
        Ok(std::mem::take(&mut self.payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct InterruptedFrame {
        bytes: io::Cursor<Vec<u8>>,
        stop: u64,
        interrupted: bool,
    }
    impl Read for InterruptedFrame {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted && self.bytes.position() == self.stop {
                self.interrupted = true;
                return Err(io::ErrorKind::TimedOut.into());
            }
            let count = if self.interrupted {
                out.len()
            } else {
                out.len().min((self.stop - self.bytes.position()) as usize)
            };
            self.bytes.read(&mut out[..count])
        }
    }
    #[test]
    fn partial_header_and_payload_survive_a_timeout_without_losing_framing() {
        let bytes = [0, 0, 0, 3, 10, 11, 12, 0, 0, 0, 1, 99].to_vec();
        for stop in 0..7 {
            let mut reader = InterruptedFrame {
                bytes: io::Cursor::new(bytes.clone()),
                stop,
                interrupted: false,
            };
            let mut frames = Frames::default();
            assert!(matches!(
                frames.read(&mut reader),
                Err(Error::Read(io::ErrorKind::TimedOut))
            ));
            assert_eq!(frames.read(&mut reader).unwrap(), [10, 11, 12]);
            assert_eq!(frames.read(&mut reader).unwrap(), [99]);
        }
    }
    #[test]
    fn zero_oversized_and_truncated_frames_fail() {
        for size in [0u32, super::super::MAX_MESSAGE as u32 + 1, u32::MAX] {
            assert!(matches!(
                Frames::default().read(&mut io::Cursor::new(size.to_be_bytes())),
                Err(Error::Bounds)
            ));
        }
        assert!(matches!(
            Frames::default().read(&mut io::Cursor::new([0, 0])),
            Err(Error::Read(io::ErrorKind::UnexpectedEof))
        ));
    }
}
