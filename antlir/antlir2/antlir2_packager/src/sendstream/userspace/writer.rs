/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use super::tlv::Tlv;

const HEADER_LEN: u32 = 10;

pub(crate) struct CommandBuilder {
    buf: Vec<u8>,
}

impl CommandBuilder {
    pub(crate) fn new(cmd: u16) -> Self {
        let mut buf = Vec::new();
        // we don't know how long the command will be yet, so leave the length
        // zeroed
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&cmd.to_le_bytes());
        // crc also starts zeroed
        buf.extend_from_slice(&0u32.to_le_bytes());
        Self { buf }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        let Self { mut buf } = self;
        let len = (buf.len() as u32) - HEADER_LEN;
        buf[0..4].copy_from_slice(&len.to_le_bytes());
        let crc = !crc32c_hw::update(!0, &buf[..]);
        buf[6..10].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    pub(crate) fn tlv(mut self, tlv: &Tlv) -> Self {
        self.buf.extend_from_slice(&tlv.ty().to_le_bytes());
        self.buf.extend_from_slice(&tlv.len().to_le_bytes());
        self.buf.extend_from_slice(tlv.data().as_ref());
        self
    }
}

#[cfg(test)]
mod tests {

    use std::path::Path;

    use super::*;

    /// Test for the simplest command - End which has no data at all, just the
    /// header.
    #[test]
    fn end_command() {
        let buf = CommandBuilder::new(21).finish();
        assert_eq!(
            buf,
            &[0x00, 0x00, 0x00, 0x00, 0x15, 0x00, 0x50, 0x6c, 0xc9, 0x9d]
        );
    }

    fn hex_encode(buf: &[u8]) -> String {
        let mut s = String::new();
        let mut iter = buf.iter().peekable();
        while let Some(b) = iter.next() {
            s.push_str(&format!("{b:02x}"));
            if iter.peek().is_some() {
                s.push(' ');
            }
        }
        s
    }

    /// A more complicated command that has some TLVs associated with it
    #[test]
    fn subvol_command() {
        let uuid = "5c61c955-bbde-ec42-9c3c-ca73ff25f89c"
            .parse()
            .expect("failed to parse test uuid");
        let buf = CommandBuilder::new(1)
            .tlv(&Tlv::Path(Path::new("bar")))
            .tlv(&Tlv::Uuid(uuid))
            .tlv(&Tlv::Ctransid(1283231))
            .finish();
        let actual = hex_encode(&buf);
        let expected = "27 00 00 00 01 00 f9 2c b6 9f 0f 00 03 00 62 61 72 01 \
            00 10 00 5c 61 c9 55 bb de ec 42 9c 3c ca 73 ff 25 f8 9c 02 00 08 \
            00 9f 94 13 00 00 00 00 00";
        assert_eq!(actual, expected);
    }
}
