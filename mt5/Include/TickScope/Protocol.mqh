//+------------------------------------------------------------------+
//|                                                     Protocol.mqh |
//|                                  Copyright 2026, TickScope Team  |
//|                                            https://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright "TickScope Team"
#property link      "https://www.mql5.com"
#property strict

#define MAGIC_TICK 0x5449434B
#define PROTOCOL_VERSION 1
#define HEADER_LENGTH 40
#define TICK_RECORD_LENGTH 72
#define HEARTBEAT_PAYLOAD_LENGTH 36
#define BATCH_ACK_PAYLOAD_LENGTH 8
#define STATUS_PAYLOAD_LENGTH 48

#define MSG_TYPE_TICK_BATCH 1
#define MSG_TYPE_HEARTBEAT 2
#define MSG_TYPE_BATCH_ACK 3
#define MSG_TYPE_STATUS 4

#define HEADER_FLAG_WARMUP 0x0001
#define HB_FLAG_HAS_LAST_TICK 0x0002
#define HB_FLAG_HAS_OFFSET_SAMPLE 0x0004

#define STATUS_FLAG_HAS_SEQUENCE_RANGE 0x0001
#define STATUS_FLAG_HAS_EXACT_COUNT 0x0002

#define PHASE_WARMING 1
#define PHASE_LIVE 2

#define STATUS_CODE_PHASE 1
#define STATUS_CODE_TICK_BACKLOG 2
#define STATUS_CODE_CURSOR_BLOCKED 3
#define STATUS_CODE_TRANSPORT_FAULT 4
#define STATUS_CODE_DATA_LOSS 5
#define STATUS_CODE_UNCONFIRMED 6
#define STATUS_CODE_RECOVERY 7

// Structure representation matching 72-byte wire record
struct WireTickRecord
{
   ulong    sequence;
   long     broker_time_msc;
   ulong    ea_elapsed_us;
   double   bid;
   double   ask;
   double   last;
   ulong    volume;
   double   volume_real;
   uint     flags;
   uint     reserved;
};

// Convert double to 8 bytes Little Endian
void StringToDoubleBytes(double val, uchar &dst[])
{
   union DoubleUnion
   {
      double d;
      uchar b[8];
   } u;
   u.d = val;
   for(int i = 0; i < 8; i++)
   {
      dst[i] = u.b[i];
   }
}

// Pack header into 40 bytes LE
void PackHeader(uchar &buffer[], int offset,
                ushort msg_type, ushort header_flags,
                uint broker_id, ulong session_id,
                ulong seq_start, uint tick_count, uint payload_len)
{
   // magic (uint 4 bytes)
   buffer[offset + 0] = 0x4B;
   buffer[offset + 1] = 0x43;
   buffer[offset + 2] = 0x49;
   buffer[offset + 3] = 0x54;
   
   // protocol_version (ushort 2 bytes)
   buffer[offset + 4] = (uchar)(PROTOCOL_VERSION & 0xFF);
   buffer[offset + 5] = (uchar)((PROTOCOL_VERSION >> 8) & 0xFF);
   
   // message_type (ushort 2 bytes)
   buffer[offset + 6] = (uchar)(msg_type & 0xFF);
   buffer[offset + 7] = (uchar)((msg_type >> 8) & 0xFF);
   
   // header_length (ushort 2 bytes)
   buffer[offset + 8] = (uchar)(HEADER_LENGTH & 0xFF);
   buffer[offset + 9] = (uchar)((HEADER_LENGTH >> 8) & 0xFF);
   
   // header_flags (ushort 2 bytes)
   buffer[offset + 10] = (uchar)(header_flags & 0xFF);
   buffer[offset + 11] = (uchar)((header_flags >> 8) & 0xFF);
   
   // broker_id (uint 4 bytes)
   for(int i = 0; i < 4; i++) buffer[offset + 12 + i] = (uchar)((broker_id >> (i * 8)) & 0xFF);
   
   // session_id (ulong 8 bytes)
   for(int i = 0; i < 8; i++) buffer[offset + 16 + i] = (uchar)((session_id >> (i * 8)) & 0xFF);
   
   // sequence_start (ulong 8 bytes)
   for(int i = 0; i < 8; i++) buffer[offset + 24 + i] = (uchar)((seq_start >> (i * 8)) & 0xFF);
   
   // tick_count (uint 4 bytes)
   for(int i = 0; i < 4; i++) buffer[offset + 32 + i] = (uchar)((tick_count >> (i * 8)) & 0xFF);
   
   // payload_length (uint 4 bytes)
   for(int i = 0; i < 4; i++) buffer[offset + 36 + i] = (uchar)((payload_len >> (i * 8)) & 0xFF);
}

// Pack 72-byte tick record into buffer LE
void PackTickRecord(uchar &buffer[], int offset, const WireTickRecord &tick)
{
   // sequence (ulong 8 bytes)
   for(int i = 0; i < 8; i++) buffer[offset + 0 + i] = (uchar)((tick.sequence >> (i * 8)) & 0xFF);
   
   // broker_time_msc (long 8 bytes)
   for(int i = 0; i < 8; i++) buffer[offset + 8 + i] = (uchar)(((ulong)tick.broker_time_msc >> (i * 8)) & 0xFF);
   
   // ea_elapsed_us (ulong 8 bytes)
   for(int i = 0; i < 8; i++) buffer[offset + 16 + i] = (uchar)((tick.ea_elapsed_us >> (i * 8)) & 0xFF);
   
   // bid (double 8 bytes binary64)
   uchar double_bytes[8];
   StringToDoubleBytes(tick.bid, double_bytes);
   for(int i = 0; i < 8; i++) buffer[offset + 24 + i] = double_bytes[i];
   
   // ask
   StringToDoubleBytes(tick.ask, double_bytes);
   for(int i = 0; i < 8; i++) buffer[offset + 32 + i] = double_bytes[i];
   
   // last
   StringToDoubleBytes(tick.last, double_bytes);
   for(int i = 0; i < 8; i++) buffer[offset + 40 + i] = double_bytes[i];
   
   // volume (ulong 8 bytes)
   for(int i = 0; i < 8; i++) buffer[offset + 48 + i] = (uchar)((tick.volume >> (i * 8)) & 0xFF);
   
   // volume_real (double 8 bytes)
   StringToDoubleBytes(tick.volume_real, double_bytes);
   for(int i = 0; i < 8; i++) buffer[offset + 56 + i] = double_bytes[i];
   
   // flags (uint 4 bytes)
   for(int i = 0; i < 4; i++) buffer[offset + 64 + i] = (uchar)((tick.flags >> (i * 8)) & 0xFF);
   
   // reserved (uint 4 bytes)
   for(int i = 0; i < 4; i++) buffer[offset + 68 + i] = (uchar)((tick.reserved >> (i * 8)) & 0xFF);
}

// Pack 36-byte Heartbeat payload
void PackHeartbeatPayload(uchar &buffer[], int offset,
                          ulong session_id, ulong last_sequence,
                          long last_tick_time_msc, int server_utc_offset_sec,
                          ulong heartbeat_elapsed_us)
{
   for(int i = 0; i < 8; i++) buffer[offset + 0 + i] = (uchar)((session_id >> (i * 8)) & 0xFF);
   for(int i = 0; i < 8; i++) buffer[offset + 8 + i] = (uchar)((last_sequence >> (i * 8)) & 0xFF);
   for(int i = 0; i < 8; i++) buffer[offset + 16 + i] = (uchar)(((ulong)last_tick_time_msc >> (i * 8)) & 0xFF);
   for(int i = 0; i < 4; i++) buffer[offset + 24 + i] = (uchar)(((uint)server_utc_offset_sec >> (i * 8)) & 0xFF);
   for(int i = 0; i < 8; i++) buffer[offset + 28 + i] = (uchar)((heartbeat_elapsed_us >> (i * 8)) & 0xFF);
}

// Pack 48-byte Status payload
void PackStatusPayload(uchar &buffer[], int offset,
                       ushort status_code, ushort phase, uint detail_flags,
                       ulong seq_first, ulong seq_last, ulong affected_count,
                       ulong ea_elapsed_us, long detail_value)
{
   buffer[offset + 0] = (uchar)(status_code & 0xFF);
   buffer[offset + 1] = (uchar)((status_code >> 8) & 0xFF);
   buffer[offset + 2] = (uchar)(phase & 0xFF);
   buffer[offset + 3] = (uchar)((phase >> 8) & 0xFF);
   for(int i = 0; i < 4; i++) buffer[offset + 4 + i] = (uchar)((detail_flags >> (i * 8)) & 0xFF);
   for(int i = 0; i < 8; i++) buffer[offset + 8 + i] = (uchar)((seq_first >> (i * 8)) & 0xFF);
   for(int i = 0; i < 8; i++) buffer[offset + 16 + i] = (uchar)((seq_last >> (i * 8)) & 0xFF);
   for(int i = 0; i < 8; i++) buffer[offset + 24 + i] = (uchar)((affected_count >> (i * 8)) & 0xFF);
   for(int i = 0; i < 8; i++) buffer[offset + 32 + i] = (uchar)((ea_elapsed_us >> (i * 8)) & 0xFF);
   for(int i = 0; i < 8; i++) buffer[offset + 40 + i] = (uchar)(((ulong)detail_value >> (i * 8)) & 0xFF);
}
