//! Streaming frame decoder and encoder for binary protocol.
//! Reference: docs/blueprint/wire-format.md

use crate::contracts::types::*;
use crate::protocol::packet::*;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFrame {
    pub frame: Frame,
    pub raw_wire_bytes: Arc<Vec<u8>>,
    pub warnings: Vec<String>,
}

pub struct StreamingDecoder {
    buffer: Vec<u8>,
    max_payload_length: usize,
    debug_resync_limit: usize,
}

impl StreamingDecoder {
    pub fn new(max_payload_length: usize, debug_resync_limit: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(8192),
            max_payload_length,
            debug_resync_limit,
        }
    }

    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn push(&mut self, data: &[u8]) -> Result<usize, ProtocolError> {
        let max_total = (HEADER_LENGTH as usize)
            .checked_add(self.max_payload_length)
            .ok_or(ProtocolError::ArithmeticOverflow)?;
        
        if self.buffer.len() + data.len() > max_total * 2 {
            return Err(ProtocolError::PayloadLengthExceedsMax(
                (self.buffer.len() + data.len()) as u32,
            ));
        }

        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    pub fn next_frame(&mut self) -> Result<Option<DecodedFrame>, ProtocolError> {
        if self.buffer.len() < HEADER_LENGTH as usize {
            return Ok(None);
        }

        let header = match decode_header(&self.buffer[..HEADER_LENGTH as usize]) {
            Ok(hdr) => hdr,
            Err(ProtocolError::NeedMore { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };

        let total_frame_len = (HEADER_LENGTH as usize)
            .checked_add(header.payload_length as usize)
            .ok_or(ProtocolError::ArithmeticOverflow)?;

        if self.buffer.len() < total_frame_len {
            return Ok(None);
        }

        let frame_bytes = self.buffer[..total_frame_len].to_vec();
        let payload_slice = &frame_bytes[HEADER_LENGTH as usize..total_frame_len];
        let mut warnings = Vec::new();

        let payload = match header.message_type {
            MSG_TYPE_TICK_BATCH => {
                let count = header.tick_count as usize;
                let mut ticks = Vec::with_capacity(count);
                for i in 0..count {
                    let offset = i * TICK_RECORD_LENGTH;
                    let (tick, warn) =
                        decode_tick_record(&payload_slice[offset..offset + TICK_RECORD_LENGTH]);
                    if let Some(w) = warn {
                        warnings.push(w);
                    }
                    ticks.push(tick);
                }
                FramePayload::TickBatch(ticks)
            }
            MSG_TYPE_HEARTBEAT => {
                let hb = decode_heartbeat(payload_slice);
                FramePayload::Heartbeat(hb)
            }
            MSG_TYPE_BATCH_ACK => {
                let ack = decode_batch_ack(payload_slice);
                FramePayload::BatchAck(ack)
            }
            MSG_TYPE_STATUS => {
                let status = decode_status(payload_slice)?;
                FramePayload::Status(status)
            }
            _ => return Err(ProtocolError::UnknownMessageType(header.message_type)),
        };

        self.buffer.drain(..total_frame_len);

        Ok(Some(DecodedFrame {
            frame: Frame {
                header,
                payload,
            },
            raw_wire_bytes: Arc::new(frame_bytes),
            warnings,
        }))
    }

    /// Attempt to scan forward for the magic bytes in case of debug resync.
    pub fn try_resync(&mut self) -> bool {
        let limit = self.debug_resync_limit.min(self.buffer.len());
        for i in 1..limit {
            if i + 4 <= self.buffer.len() {
                let magic = u32::from_le_bytes(self.buffer[i..i + 4].try_into().unwrap());
                if magic == MAGIC_TICK {
                    self.buffer.drain(..i);
                    return true;
                }
            }
        }
        false
    }
}

pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, ProtocolError> {
    let payload_len = match &frame.payload {
        FramePayload::TickBatch(ticks) => ticks
            .len()
            .checked_mul(TICK_RECORD_LENGTH)
            .ok_or(ProtocolError::ArithmeticOverflow)?,
        FramePayload::Heartbeat(_) => HEARTBEAT_PAYLOAD_LENGTH,
        FramePayload::BatchAck(_) => BATCH_ACK_PAYLOAD_LENGTH,
        FramePayload::Status(_) => STATUS_PAYLOAD_LENGTH,
    };

    let total_len = (HEADER_LENGTH as usize)
        .checked_add(payload_len)
        .ok_or(ProtocolError::ArithmeticOverflow)?;

    let mut buf = vec![0u8; total_len];
    encode_header(&frame.header, &mut buf[..HEADER_LENGTH as usize]);

    let payload_buf = &mut buf[HEADER_LENGTH as usize..];
    match &frame.payload {
        FramePayload::TickBatch(ticks) => {
            for (i, tick) in ticks.iter().enumerate() {
                let offset = i * TICK_RECORD_LENGTH;
                encode_tick_record(tick, &mut payload_buf[offset..offset + TICK_RECORD_LENGTH]);
            }
        }
        FramePayload::Heartbeat(hb) => {
            encode_heartbeat(hb, payload_buf);
        }
        FramePayload::BatchAck(ack) => {
            encode_batch_ack(ack, payload_buf);
        }
        FramePayload::Status(status) => {
            encode_status(status, payload_buf);
        }
    }

    Ok(buf)
}
