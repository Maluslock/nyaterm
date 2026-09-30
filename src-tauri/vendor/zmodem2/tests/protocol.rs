// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2017-2020 Alexey Arbuzov
// Copyright (c) 2023-2026 Jarkko Sakkinen

use rstest::rstest;
use zmodem2::{
    Encoding, Frame, Header, Receiver, ReceiverEvent, Sender, SenderEvent, SubpacketType,
    ZfileManagementOption, Zrinit, XON, ZDLE, ZPAD,
};

#[rstest]
#[case(Encoding::ZBIN, Frame::ZRQINIT, &[0; 4], &[ZPAD, ZDLE, Encoding::ZBIN as u8, 0, 0, 0, 0, 0, 0, 0])]
#[case(Encoding::ZBIN32, Frame::ZRQINIT, &[0; 4], &[ZPAD, ZDLE, Encoding::ZBIN32 as u8, 0, 0, 0, 0, 0, 29, 247, 34, 198])]
#[case(Encoding::ZBIN, Frame::ZRQINIT, &[1; 4], &[ZPAD, ZDLE, Encoding::ZBIN as u8, 0, 1, 1, 1, 1, 98, 148])]
#[case(Encoding::ZHEX, Frame::ZRQINIT, &[1; 4], &[ZPAD, ZPAD, ZDLE, Encoding::ZHEX as u8, b'0', b'0', b'0', b'1', b'0', b'1', b'0', b'1', b'0', b'1', 54, 50, 57, 52, b'\r', b'\n', XON])]
pub fn test_header_write(
    #[case] encoding: Encoding,
    #[case] frame: Frame,
    #[case] flags: &[u8; 4],
    #[case] expected: &[u8],
) {
    let header = Header::new(encoding, frame, flags);
    let mut port = vec![];
    assert!(header.write(&mut port) == Ok(Some(())));
    assert_eq!(port, expected);
}

#[test]
fn test_header_write_with_escape_control_round_trips_binary_flags() {
    let header = Header::new(Encoding::ZBIN32, Frame::ZDATA, &[b'\n', 0x1b, 0x80, 0x9f]);
    let mut wire = Vec::new();
    header.write_with_escape_control(&mut wire, true).unwrap();

    assert!(wire.windows(2).any(|pair| pair == [ZDLE, b'J']));
    let mut reader = &wire[2..];
    assert!(Header::read(&mut reader).unwrap() == Some(header));
}

#[rstest]
#[case(&[Encoding::ZHEX as u8, b'0', b'1', b'0', b'1', b'0', b'2', b'0', b'3', b'0', b'4', b'a', b'7', b'5', b'2'], Encoding::ZHEX, Frame::ZRINIT, &[0x1, 0x2, 0x3, 0x4])]
#[case(&[Encoding::ZBIN as u8, Frame::ZRINIT as u8, 0xa, 0xb, 0xc, 0xd, 0xa6, 0xcb], Encoding::ZBIN, Frame::ZRINIT, &[0xa, 0xb, 0xc, 0xd])]
#[case(&[Encoding::ZBIN32 as u8, Frame::ZRINIT as u8, 0xa, 0xb, 0xc, 0xd, 0x99, 0xe2, 0xae, 0x4a], Encoding::ZBIN32, Frame::ZRINIT, &[0xa, 0xb, 0xc, 0xd])]
#[case(&[Encoding::ZBIN as u8, Frame::ZRINIT as u8, 0xa, ZDLE, b'l', 0xd, ZDLE, b'm', 0x5e, 0x6f], Encoding::ZBIN, Frame::ZRINIT, &[0xa, 0x7f, 0xd, 0xff])]
pub fn test_header_read(
    #[case] port: &[u8],
    #[case] encoding: Encoding,
    #[case] frame: Frame,
    #[case] flags: &[u8; 4],
) {
    let port = &mut port.to_vec();
    let port = &mut port.as_slice();
    assert!(Header::read(port) == Ok(Some(Header::new(encoding, frame, flags))));
}

fn zhex_header_from_wire(wire: &[u8]) -> Header {
    assert!(
        wire.starts_with(&[ZPAD, ZPAD, ZDLE, Encoding::ZHEX as u8]),
        "expected ZHEX header, got {wire:?}"
    );
    let mut port = &wire[3..];
    Header::read(&mut port)
        .expect("read header")
        .expect("header")
}

fn write_zrinit(flags: &[u8; 4]) -> Vec<u8> {
    let mut wire = Vec::new();
    Header::new(Encoding::ZHEX, Frame::ZRINIT, flags)
        .write(&mut wire)
        .expect("write zrinit")
        .expect("complete zrinit");
    wire
}

fn write_zrpos(offset: u32) -> Vec<u8> {
    let mut wire = Vec::new();
    Header::new(Encoding::ZHEX, Frame::ZRPOS, &offset.to_le_bytes())
        .write(&mut wire)
        .expect("write zrpos")
        .expect("complete zrpos");
    wire
}

fn write_zskip() -> Vec<u8> {
    let mut wire = Vec::new();
    Header::new(Encoding::ZHEX, Frame::ZSKIP, &[0; 4])
        .write(&mut wire)
        .expect("write zskip")
        .expect("complete zskip");
    wire
}

fn write_zabort() -> Vec<u8> {
    let mut wire = Vec::new();
    Header::new(Encoding::ZHEX, Frame::ZABORT, &[0; 4])
        .write(&mut wire)
        .expect("write zabort")
        .expect("complete zabort");
    wire
}

fn zbin32_header_from_wire(wire: &[u8]) -> Header {
    assert!(
        wire.starts_with(&[ZPAD, ZDLE, Encoding::ZBIN32 as u8]),
        "expected ZBIN32 header, got {wire:?}"
    );
    let mut port = &wire[2..];
    Header::read(&mut port)
        .expect("read header")
        .expect("header")
}

fn zfile_payload_from_wire(wire: &[u8]) -> Vec<u8> {
    assert!(
        wire.starts_with(&[ZPAD, ZDLE, Encoding::ZBIN32 as u8]),
        "expected ZBIN32 header, got {wire:?}"
    );

    let mut port = &wire[2..];
    let header = Header::read(&mut port)
        .expect("read zfile header")
        .expect("zfile header");
    assert_eq!(header.frame(), Frame::ZFILE);

    let mut payload = Vec::new();
    let mut index = 0;
    while index < port.len() {
        let byte = port[index];
        index += 1;
        if byte == ZDLE {
            let next = port[index];
            index += 1;
            if next == SubpacketType::ZCRCW as u8 {
                return payload;
            }
            payload.push(next ^ 0x40);
        } else {
            payload.push(byte);
        }
    }

    panic!("missing ZFILE metadata subpacket terminator");
}

fn sender_zfile_payload(sender: &mut Sender) -> Vec<u8> {
    sender.advance_outgoing(sender.drain_outgoing().len());
    sender.feed_incoming(&write_zrinit(&[0; 4])).unwrap();
    zfile_payload_from_wire(sender.drain_outgoing())
}

fn has_subpacket_kind(wire: &[u8], kind: SubpacketType) -> bool {
    wire.windows(2).any(|window| window == [ZDLE, kind as u8])
}

#[test]
fn test_receiver_zrinit_advertises_large_overlap_window() {
    let receiver = Receiver::new().unwrap();
    let header = zhex_header_from_wire(receiver.drain_outgoing());
    let flags = header.count().to_le_bytes();
    let advertised_buffer_size = u16::from_le_bytes([flags[0], flags[1]]);
    let caps = Zrinit::from_bits_truncate(flags[2] | flags[3]);

    assert!(header == Header::new(Encoding::ZHEX, Frame::ZRINIT, &flags));
    assert_eq!(advertised_buffer_size, u16::MAX);
    assert!(caps.contains(Zrinit::CANFDX));
    assert!(caps.contains(Zrinit::CANOVIO));
    assert!(caps.contains(Zrinit::CANFC32));
}

#[test]
fn test_sender_requests_8k_file_chunks_after_large_zrinit() {
    let mut sender = Sender::new().unwrap();
    sender.start_file(b"large.bin", 20_000).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    let flags = [
        0xff,
        0xff,
        0,
        (Zrinit::CANFDX | Zrinit::CANOVIO | Zrinit::CANFC32).bits(),
    ];
    sender.feed_incoming(&write_zrinit(&flags)).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    sender.feed_incoming(&write_zrpos(0)).unwrap();

    let request = sender.poll_file().expect("file data request");
    assert_eq!(request.offset, 0);
    assert_eq!(request.len, 8 * 1024);
}

fn sender_file_wire_with_zrinit_flags(first_caps: Zrinit, second_caps: Option<Zrinit>) -> Vec<u8> {
    let mut sender = Sender::new().unwrap();
    sender.start_file(b"escape.bin", 3).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    sender
        .feed_incoming(&write_zrinit(&[0, 0, 0, first_caps.bits()]))
        .unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    if let Some(caps) = second_caps {
        sender
            .feed_incoming(&write_zrinit(&[0, 0, 0, caps.bits()]))
            .unwrap();
    }
    sender.feed_incoming(&write_zrpos(0)).unwrap();
    assert_eq!(sender.poll_file().unwrap().offset, 0);
    sender.feed_file(b"\n~.").unwrap();
    sender.drain_outgoing().to_vec()
}

#[test]
fn test_sender_preserves_unescaped_lf_without_escctl() {
    let wire = sender_file_wire_with_zrinit_flags(
        Zrinit::CANFDX | Zrinit::CANOVIO | Zrinit::CANFC32,
        None,
    );
    assert!(wire.windows(3).any(|bytes| bytes == b"\n~."));
}

#[test]
fn test_sender_escapes_control_characters_when_zrinit_requests_escctl() {
    let wire = sender_file_wire_with_zrinit_flags(
        Zrinit::CANFDX | Zrinit::CANOVIO | Zrinit::CANFC32 | Zrinit::ESCCTL,
        None,
    );
    assert!(!wire.windows(3).any(|bytes| bytes == b"\n~."));
    assert!(wire.windows(2).any(|bytes| bytes == [ZDLE, b'J']));
}

#[test]
fn test_sender_keeps_escctl_after_later_zrinit_omits_it() {
    let caps = Zrinit::CANFDX | Zrinit::CANOVIO | Zrinit::CANFC32;
    let wire = sender_file_wire_with_zrinit_flags(caps | Zrinit::ESCCTL, Some(caps));
    assert!(!wire.windows(3).any(|bytes| bytes == b"\n~."));
    assert!(wire.windows(2).any(|bytes| bytes == [ZDLE, b'J']));
}

#[test]
fn test_sender_escapes_full_8k_control_subpacket() {
    let mut sender = Sender::new().unwrap();
    sender.start_file(b"control.bin", 8 * 1024).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());
    sender
        .feed_incoming(&write_zrinit(&[0, 0, 0, Zrinit::ESCCTL.bits()]))
        .unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());
    sender.feed_incoming(&write_zrpos(0)).unwrap();

    let request = sender.poll_file().unwrap();
    assert_eq!(request.len, 8 * 1024);
    sender.feed_file(&vec![b'\n'; request.len]).unwrap();
    let wire = sender.drain_outgoing();
    assert!(wire.len() > 16 * 1024);
    assert!(!wire.windows(2).any(|bytes| bytes == b"\n~"));
}

#[test]
fn test_sender_writes_zfile_management_options() {
    let mut sender = Sender::new().unwrap();
    sender.set_file_options(ZfileManagementOption::ZMCLOB);
    sender.start_file(b"overwrite.bin", 1).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    sender.feed_incoming(&write_zrinit(&[0; 4])).unwrap();

    let header = zbin32_header_from_wire(sender.drain_outgoing());
    assert_eq!(header.frame(), Frame::ZFILE);
    assert_eq!(
        header.count().to_le_bytes(),
        [1, ZfileManagementOption::ZMCLOB.bits(), 0, 0]
    );
}

#[test]
fn test_sender_writes_complete_zfile_metadata() {
    let mut sender = Sender::new().unwrap();
    sender
        .start_file_with_metadata(b"metadata.bin", 7012, 1_710_000_000, 0o100644)
        .unwrap();

    let payload = sender_zfile_payload(&mut sender);
    let metadata = format!("7012 {:o} 100644 0 0 0\0", 1_710_000_000);
    let expected = [b"metadata.bin\0".as_slice(), metadata.as_bytes()].concat();

    assert_eq!(payload, expected);
}

#[test]
fn test_sender_writes_zero_mtime_explicitly() {
    let mut sender = Sender::new().unwrap();
    sender
        .start_file_with_metadata(b"zero-time.bin", 42, 0, 0o100644)
        .unwrap();

    let payload = sender_zfile_payload(&mut sender);

    assert_eq!(
        payload,
        [b"zero-time.bin\0".as_slice(), b"42 0 100644 0 0 0\0"].concat()
    );
}

#[test]
fn test_sender_start_file_uses_complete_safe_metadata() {
    let mut sender = Sender::new().unwrap();
    sender.start_file(b"default.bin", 99).unwrap();

    let payload = sender_zfile_payload(&mut sender);

    assert_eq!(
        payload,
        [b"default.bin\0".as_slice(), b"99 0 100644 0 0 0\0"].concat()
    );
}

#[test]
fn test_sender_reports_zskip_as_file_skipped() {
    let mut sender = Sender::new().unwrap();
    sender.set_file_options(ZfileManagementOption::ZMSKNOLOC);
    sender.start_file(b"existing.bin", 1).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());
    sender.feed_incoming(&write_zrinit(&[0; 4])).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    sender.feed_incoming(&write_zskip()).unwrap();

    assert_eq!(sender.poll_event(), Some(SenderEvent::FileSkipped));
    assert!(sender.poll_file().is_none());
}

#[test]
fn test_sender_reports_remote_abort() {
    let mut sender = Sender::new().unwrap();
    sender.start_file(b"blocked.bin", 1).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());
    sender.feed_incoming(&write_zrinit(&[0; 4])).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    assert_eq!(
        sender.feed_incoming(&write_zabort()),
        Err(zmodem2::Error::RemoteAborted)
    );
}

#[test]
fn test_sender_uses_streaming_subpackets_until_ack_boundary() {
    let mut sender = Sender::new().unwrap();
    sender.start_file(b"window.bin", 70_000).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    let flags = [
        0,
        0,
        0,
        (Zrinit::CANFDX | Zrinit::CANOVIO | Zrinit::CANFC32).bits(),
    ];
    sender.feed_incoming(&write_zrinit(&flags)).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    sender.feed_incoming(&write_zrpos(0)).unwrap();

    for packet_index in 0..8 {
        let request = sender.poll_file().expect("file data request");
        assert_eq!(request.len, 8 * 1024);
        sender.feed_file(&vec![0x55; request.len]).unwrap();
        let wire = sender.drain_outgoing().to_vec();

        if packet_index == 0 {
            assert!(has_subpacket_kind(&wire, SubpacketType::ZCRCG));
        }
        if packet_index == 7 {
            assert!(has_subpacket_kind(&wire, SubpacketType::ZCRCW));
        }

        sender.advance_outgoing(wire.len());
    }

    assert!(sender.poll_file().is_none());
}

#[test]
fn test_receive_malformed_header() {
    let mut receiver = Receiver::new().unwrap();
    receiver.advance_outgoing(receiver.drain_outgoing().len());

    let input = b"malformed data";
    let consumed = receiver.feed_incoming(input).unwrap();

    assert_eq!(consumed, input.len());
    assert!(receiver.drain_outgoing().is_empty());
    assert!(receiver.drain_file().is_empty());
    assert!(receiver.poll_event().is_none());
}

#[test]
fn test_receive_zfile_with_non_utf8_name() {
    let file_name = b"bad\x80name";
    let file_size = 123;

    let mut sender = Sender::new().unwrap();
    sender.start_file(file_name, file_size).unwrap();
    sender.advance_outgoing(sender.drain_outgoing().len());

    let zrinit = Header::new(Encoding::ZHEX, Frame::ZRINIT, &[0; 4]);
    let mut zrinit_bytes = Vec::new();
    zrinit.write(&mut zrinit_bytes).unwrap();
    let consumed = sender.feed_incoming(&zrinit_bytes).unwrap();
    assert!(consumed > 0 && consumed <= zrinit_bytes.len());

    let wire = sender.drain_outgoing().to_vec();
    sender.advance_outgoing(wire.len());

    let mut receiver = Receiver::new().unwrap();
    receiver.advance_outgoing(receiver.drain_outgoing().len());

    let mut input = wire.as_slice();
    let mut got_start = false;
    for _ in 0..(wire.len() * 4) {
        if input.is_empty() {
            break;
        }
        let consumed = receiver.feed_incoming(input).unwrap();
        if consumed == 0 {
            if !receiver.drain_outgoing().is_empty() {
                receiver.advance_outgoing(receiver.drain_outgoing().len());
            }
        } else {
            input = &input[consumed..];
        }

        if let Some(ReceiverEvent::FileStart) = receiver.poll_event() {
            got_start = true;
            break;
        }
    }

    assert!(got_start);
    assert_eq!(receiver.file_name(), file_name);
    assert_eq!(receiver.file_size(), file_size);
}
