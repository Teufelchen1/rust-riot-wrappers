//! This module implements [coap_message::ReadableMessage] for, and a wrapper that provides
//! [coap_message::WritableMessage] around RIOT's coap_pkt_t.

mod impl_0_3;

use crate::gcoap::{PacketBuffer, PacketBufferOptIter};

pub struct MessageOption<'a> {
    number: u16,
    value: &'a [u8],
}

pub struct OptionsIterator<'a>(PacketBufferOptIter<'a>);
impl<'a> Iterator for OptionsIterator<'a> {
    type Item = MessageOption<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (opt_num, slice) = self.0.next()?;
        Some(MessageOption {
            number: opt_num,
            value: slice,
        })
    }
}

pub struct ResponseMessage<'a> {
    /// Note that this is a slightly weird version of PacketBuffer, where opt_finish is never
    /// called, and .payload() perpetually reports the payload marker as part of the payload.
    message: &'a mut PacketBuffer,
    payload_written: Option<usize>,
}

impl<'a> ResponseMessage<'a> {
    pub fn new(buf: &'a mut PacketBuffer) -> Self {
        // Can't really err; FIXME ensure that such a check won't affect ROM too much
        buf.resp_init(5 << 5).unwrap();

        ResponseMessage {
            message: buf,
            payload_written: None,
        }
    }

    pub(crate) fn rewind(&mut self) {
        self.message.resp_init(5 << 5).unwrap();
    }

    pub fn finish(&self) -> isize {
        self.message.get_length(match self.payload_written {
            None => 0,
            Some(x) => x + 1,
        }) as isize
    }
}
