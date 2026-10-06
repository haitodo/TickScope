use super::bytes::le_u32;
use super::packet::{ProtocolError, decode_header, decode_tick_record, decode_heartbeat, decode_batch_ack, decode_status, encode_header, encode_tick_record, encode_heartbeat, encode_batch_ack, encode_status};
use super::wire::{Frame, HEADER_LENGTH, MSG_TYPE_TICK_BATCH, TICK_RECORD_LENGTH, FramePayload, MSG_TYPE_HEARTBEAT, MSG_TYPE_BATCH_ACK, MSG_TYPE_STATUS, MAGIC_TICK, HEARTBEAT_PAYLOAD_LENGTH, BATCH_ACK_PAYLOAD_LENGTH, STATUS_PAYLOAD_LENGTH};
use std::sync::{Arc, OnceLock};

static EMPTY_RAW_WIRE_BYTES: OnceLock<Arc<Vec<u8>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFrame {
    pub frame: Frame,
    pub raw_wire_bytes: Arc<Vec<u8>>,
    pub warnings: Vec<String>,
}

pub struct StreamingDecoder {
    buffer: Vec<u8>,
    consumed: usize,
    max_payload_length: usize,
    debug_resync_limit: usize,
    capture_raw_wire_bytes: bool,
}

impl StreamingDecoder {
    #[must_use]
    pub fn new(max_payload_length: usize, debug_resync_limit: usize) -> Self {
        Self::new_with_raw_capture(max_payload_length, debug_resync_limit, true)
    }

    /// Construct a decoder that can skip the per-frame raw-byte copy when raw
    /// capture is disabled. The default constructor keeps the historical API
    /// behaviour and retains raw bytes.
    #[must_use]
    pub fn new_with_raw_capture(
        max_payload_length: usize,
        debug_resync_limit: usize,
        capture_raw_wire_bytes: bool,
    ) -> Self {
        Self {
            buffer: Vec::with_capacity(8192),
            consumed: 0,
            max_payload_length,
            debug_resync_limit,
            capture_raw_wire_bytes,
        }
    }

    #[must_use]
    pub fn buffer_len(&self) -> usize {
        self.buffer.len() - self.consumed
    }

    /// Append received bytes to the reassembly buffer, returning how many were consumed.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::ArithmeticOverflow`] when the buffer arithmetic overflows, and
    /// [`ProtocolError::PayloadLengthExceedsMax`] when the pending data would exceed twice the largest
    /// legal frame.
    pub fn push(&mut self, data: &[u8]) -> Result<usize, ProtocolError> {
        let max_total = (HEADER_LENGTH as usize)
            .checked_add(self.max_payload_length)
            .ok_or(ProtocolError::ArithmeticOverflow)?;
        
        let pending = self.buffer_len().checked_add(data.len())
            .ok_or(ProtocolError::ArithmeticOverflow)?;
        let max_buffer = max_total.checked_mul(2)
            .ok_or(ProtocolError::ArithmeticOverflow)?;
        if pending > max_buffer {
            return Err(ProtocolError::PayloadLengthExceedsMax(
                pending.min(u32::MAX as usize) as u32,
            ));
        }

        // Compact once per read, never once per decoded frame in a burst.
        if self.consumed > 0 {
            self.buffer.drain(..self.consumed);
            self.consumed = 0;
        }
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    /// Decode the next complete frame, if one is buffered.
    ///
    /// Returns `Ok(None)` when more bytes are needed; the buffered data is left untouched.
    ///
    /// # Errors
    ///
    /// Returns any [`ProtocolError`] reported by [`decode_header`] for a complete header (magic,
    /// version, message type, flags or field length violations),
    /// [`ProtocolError::PayloadLengthExceedsMax`] when the declared payload is larger than this
    /// decoder accepts, and [`ProtocolError::ArithmeticOverflow`] when the frame length calculation
    /// overflows.
    pub fn next_frame(&mut self) -> Result<Option<DecodedFrame>, ProtocolError> {
        if self.buffer_len() < HEADER_LENGTH as usize {
            return Ok(None);
        }

        let header = match decode_header(&self.buffer[self.consumed..self.consumed + HEADER_LENGTH as usize]) {
            Ok(hdr) => hdr,
            Err(ProtocolError::NeedMore { .. }) => return Ok(None),
            Err(e) => return Err(e),
        };

        if header.payload_length as usize > self.max_payload_length {
            return Err(ProtocolError::PayloadLengthExceedsMax(header.payload_length));
        }

        let total_frame_len = (HEADER_LENGTH as usize)
            .checked_add(header.payload_length as usize)
            .ok_or(ProtocolError::ArithmeticOverflow)?;

        if self.buffer_len() < total_frame_len {
            return Ok(None);
        }

        let frame_end = self.consumed + total_frame_len;
        let payload_start = self.consumed + HEADER_LENGTH as usize;
        let payload_slice = &self.buffer[payload_start..frame_end];
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

        let raw_wire_bytes = if self.capture_raw_wire_bytes {
            Arc::new(self.buffer[self.consumed..frame_end].to_vec())
        } else {
            EMPTY_RAW_WIRE_BYTES
                .get_or_init(|| Arc::new(Vec::new()))
                .clone()
        };

        self.consumed = frame_end;
        if self.consumed == self.buffer.len() {
            self.buffer.clear();
            self.consumed = 0;
        }

        Ok(Some(DecodedFrame {
            frame: Frame {
                header,
                payload,
            },
            raw_wire_bytes,
            warnings,
        }))
    }

    /// Attempt to scan forward for the magic bytes in case of debug resync.
    pub fn try_resync(&mut self) -> bool {
        let limit = self.debug_resync_limit.min(self.buffer_len());
        for i in 1..limit {
            if i + 4 <= self.buffer_len() {
                let start = self.consumed + i;
                let magic = le_u32(&self.buffer, start);
                if magic == MAGIC_TICK {
                    self.consumed += i;
                    return true;
                }
            }
        }
        false
    }
}

/// Encode a frame into wire bytes.
///
/// # Errors
///
/// Returns [`ProtocolError::ArithmeticOverflow`] when the payload length or the total frame length
/// cannot be represented.
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
