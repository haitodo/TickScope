//+------------------------------------------------------------------+
//|                                                TickCollector.mq5 |
//|                                  Copyright 2026, TickScope Team  |
//|                                            https://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright "TickScope Team"
#property link      "https://www.mql5.com"
#property version   "1.00"
#property description "TickScope Multi-Broker Real-time Tick Collector EA"

#include <TickScope/Protocol.mqh>
#include <TickScope/SocketClient.mqh>

//--- Inputs
#ifdef TICKSCOPE_BROKER_ID
// Generated broker EAs get connection settings from TickScope configuration.
const uint   InpBrokerID = TICKSCOPE_BROKER_ID;
const string InpServerHost = TICKSCOPE_SERVER_HOST;
const uint   InpServerPort = TICKSCOPE_SERVER_PORT;
#else
input uint   InpBrokerID            = 1;              // Broker ID (1, 2, 3...)
input string InpServerHost          = "127.0.0.1";    // TickScope Server Host
input uint   InpServerPort          = 39001;          // TickScope Server Port
#endif
input uint   InpWarmupSeconds       = 60;             // Warmup history duration (seconds)
input uint   InpBatchCount          = 256;            // CopyTicks batch size
input uint   InpMaxSameMsScanTicks  = 65536;          // Max scan ticks in single millisecond
input uint   InpSocketTimeoutMs     = 10;             // Socket timeout (ms)
input uint   InpHeartbeatIntervalMs = 250;            // Heartbeat interval (ms)

//--- Runtime State
CSocketClient g_socket;
ulong         g_session_id             = 0;
ulong         g_current_sequence       = 0;
ulong         g_last_acked_sequence    = 0;
bool          g_has_acked_sequence     = false;
ushort        g_current_phase          = PHASE_WARMING;
ulong         g_ea_start_microsecond   = 0;

// Cursor tracking
long          g_cursor_time_msc        = 0;
uint          g_cursor_same_ms_count   = 0;
long          g_last_tick_time_msc     = 0;
ulong         g_last_heartbeat_us      = 0;
bool          g_warmup_done            = false;
bool          g_warmup_started         = false;

// Exactly one complete tick batch may be outstanding.  This is deliberately a
// stop-and-wait ledger: it keeps source memory bounded and makes the cursor
// commit point unambiguous.  A TCP write is never treated as delivery; only a
// validated ACK for this batch advances the cursor and sequence.
bool          g_pending_batch          = false;
bool          g_pending_queued         = false;
ulong         g_pending_sequence_start = 0;
ulong         g_pending_sequence_end   = 0;
long          g_pending_cursor_time_msc = 0;
uint          g_pending_cursor_same_ms  = 0;
long          g_pending_last_tick_time_msc = 0;
uchar         g_pending_packet[];

// Reusable scratch buffers
MqlTick       g_tick_buffer[];
uchar         g_packet_buffer[];

//+------------------------------------------------------------------+
//| Queue/replay the one unacknowledged batch                        |
//+------------------------------------------------------------------+
bool QueuePendingBatch()
{
   if(!g_pending_batch) return true;
   if(!g_socket.IsConnected()) return false;
   if(g_pending_queued)
   {
      return g_socket.Flush();
   }
   if(!g_socket.QueueBytes(g_pending_packet, ArraySize(g_pending_packet)))
   {
      return false;
   }
   g_pending_queued = true;
   return true;
}

//+------------------------------------------------------------------+
//| Commit the source cursor only after the exact cumulative ACK     |
//+------------------------------------------------------------------+
bool PollAndCommitAck()
{
   bool received_ack = false;
   bool protocol_fault = false;
   g_socket.PollReplies(g_last_acked_sequence, g_has_acked_sequence,
                        InpBrokerID, g_session_id,
                        received_ack, protocol_fault);
   if(protocol_fault)
   {
      g_pending_queued = false;
      return false;
   }
   if(!received_ack || !g_pending_batch) return true;

   if(g_last_acked_sequence != g_pending_sequence_end)
   {
      PrintFormat("[TickCollector] Unexpected ACK %I64u, expected %I64u; preserving pending batch for replay.",
                  g_last_acked_sequence, g_pending_sequence_end);
      g_socket.Disconnect();
      g_pending_queued = false;
      return false;
   }

   g_cursor_time_msc = g_pending_cursor_time_msc;
   g_cursor_same_ms_count = g_pending_cursor_same_ms;
   g_last_tick_time_msc = g_pending_last_tick_time_msc;
   g_current_sequence = g_pending_sequence_end + 1;
   g_pending_batch = false;
   g_pending_queued = false;
   ArrayResize(g_pending_packet, 0);
   return true;
}

//+------------------------------------------------------------------+
//| Helper: Get EA program elapsed time in microseconds             |
//+------------------------------------------------------------------+
ulong GetEaElapsedUs()
{
   return GetMicrosecondCount() - g_ea_start_microsecond;
}

//+------------------------------------------------------------------+
//| Helper: Generate non-zero unique session ID                      |
//+------------------------------------------------------------------+
ulong GenerateSessionId()
{
   ulong t = (ulong)TimeCurrent();
   ulong us = GetMicrosecondCount();
   ulong session = (t << 32) | (us & 0xFFFFFFFF);
   if(session == 0) session = 1;
   return session;
}

//+------------------------------------------------------------------+
//| Helper: Send STATUS frame                                        |
//+------------------------------------------------------------------+
void SendStatus(ushort status_code, ushort phase, uint detail_flags,
                ulong seq_first, ulong seq_last, ulong affected_count, long detail_value)
{
   ArrayResize(g_packet_buffer, HEADER_LENGTH + STATUS_PAYLOAD_LENGTH);
   PackHeader(g_packet_buffer, 0, MSG_TYPE_STATUS, 0,
              InpBrokerID, g_session_id, 0, 0, STATUS_PAYLOAD_LENGTH);
   PackStatusPayload(g_packet_buffer, HEADER_LENGTH,
                     status_code, phase, detail_flags,
                     seq_first, seq_last, affected_count,
                     GetEaElapsedUs(), detail_value);
   g_socket.QueueBytes(g_packet_buffer, ArraySize(g_packet_buffer));
}

//+------------------------------------------------------------------+
//| Helper: Send HEARTBEAT frame                                     |
//+------------------------------------------------------------------+
void SendHeartbeat()
{
   ushort flags = 0;
   if(g_current_sequence > 0 || g_last_tick_time_msc > 0)
   {
      flags |= HB_FLAG_HAS_LAST_TICK;
   }
   
   // Approximate offset sample for diagnostic
   int offset_sec = (int)(TimeCurrent() - TimeGMT());
   flags |= HB_FLAG_HAS_OFFSET_SAMPLE;

   ArrayResize(g_packet_buffer, HEADER_LENGTH + HEARTBEAT_PAYLOAD_LENGTH);
   PackHeader(g_packet_buffer, 0, MSG_TYPE_HEARTBEAT, flags,
              InpBrokerID, g_session_id, 0, 0, HEARTBEAT_PAYLOAD_LENGTH);
   PackHeartbeatPayload(g_packet_buffer, HEADER_LENGTH,
                        g_session_id, (g_current_sequence > 0 ? g_current_sequence - 1 : 0),
                        g_last_tick_time_msc, offset_sec,
                        GetEaElapsedUs());
   g_socket.QueueBytes(g_packet_buffer, ArraySize(g_packet_buffer));
   g_last_heartbeat_us = GetEaElapsedUs();
}

//+------------------------------------------------------------------+
//| Collect and stream ticks using bounded cursor                    |
//+------------------------------------------------------------------+
bool CollectAndStreamTicks(bool is_warmup)
{
   // Never advance CopyTicks cursor or sequence while a batch has not been
   // acknowledged.  This bounds the resend ledger to one frame and prevents
   // a socket disconnect from becoming an irreversible source-data gap.
   if(g_pending_batch)
   {
      return QueuePendingBatch();
   }

   // Invariant: Bounded request count to avoid infinite loop on same millisecond
   uint request_count = g_cursor_same_ms_count + InpBatchCount;
   if(request_count > InpMaxSameMsScanTicks)
   {
      PrintFormat("CURSOR_BLOCKED: same ms count %d exceeds limit %d",
                  g_cursor_same_ms_count, InpMaxSameMsScanTicks);
      SendStatus(STATUS_CODE_CURSOR_BLOCKED, g_current_phase, 0,
                 g_current_sequence, g_current_sequence, 0, g_cursor_same_ms_count);
      return false;
   }

   int copied = CopyTicks(Symbol(), g_tick_buffer, COPY_TICKS_ALL, g_cursor_time_msc, request_count);
   if(copied <= 0)
   {
      return false;
   }

   // Skip already-processed prefix in the current millisecond
   int start_index = 0;
   if(g_cursor_time_msc > 0 && g_cursor_same_ms_count > 0)
   {
      start_index = (int)g_cursor_same_ms_count;
      if(start_index >= copied)
      {
         // No new ticks yet beyond the cursor
         return false;
      }
   }

   int new_tick_count = copied - start_index;
   if(new_tick_count <= 0) return false;

   // Cap packet size to InpBatchCount
   if(new_tick_count > (int)InpBatchCount)
   {
      new_tick_count = (int)InpBatchCount;
   }

   // Construct an immutable candidate TICK_BATCH packet.  All cursor and
   // sequence changes remain local until the corresponding ACK arrives.
   uint payload_len = (uint)new_tick_count * TICK_RECORD_LENGTH;
   ArrayResize(g_pending_packet, HEADER_LENGTH + payload_len);

   ushort hdr_flags = is_warmup ? HEADER_FLAG_WARMUP : 0;
   ulong batch_start_seq = g_current_sequence;

   PackHeader(g_pending_packet, 0, MSG_TYPE_TICK_BATCH, hdr_flags,
              InpBrokerID, g_session_id, batch_start_seq,
              (uint)new_tick_count, payload_len);

   WireTickRecord wire_tick;
   int offset = HEADER_LENGTH;
   ulong candidate_sequence = g_current_sequence;
   long candidate_cursor_time_msc = g_cursor_time_msc;
   uint candidate_cursor_same_ms = g_cursor_same_ms_count;
   long candidate_last_tick_time_msc = g_last_tick_time_msc;

   for(int i = 0; i < new_tick_count; i++)
   {
      MqlTick src = g_tick_buffer[start_index + i];

      wire_tick.sequence = candidate_sequence;
      wire_tick.broker_time_msc = src.time_msc;
      wire_tick.ea_elapsed_us = GetEaElapsedUs();
      wire_tick.bid = src.bid;
      wire_tick.ask = src.ask;
      wire_tick.last = src.last;
      wire_tick.volume = src.volume;
      wire_tick.volume_real = src.volume_real;
      wire_tick.flags = src.flags;
      wire_tick.reserved = 0;

      PackTickRecord(g_pending_packet, offset, wire_tick);
      offset += TICK_RECORD_LENGTH;

      candidate_sequence++;
      candidate_last_tick_time_msc = src.time_msc;

      if(src.time_msc == candidate_cursor_time_msc)
      {
         candidate_cursor_same_ms++;
      }
      else
      {
         candidate_cursor_time_msc = src.time_msc;
         candidate_cursor_same_ms = 1;
      }
   }

   g_pending_batch = true;
   g_pending_queued = false;
   g_pending_sequence_start = batch_start_seq;
   g_pending_sequence_end = candidate_sequence - 1;
   g_pending_cursor_time_msc = candidate_cursor_time_msc;
   g_pending_cursor_same_ms = candidate_cursor_same_ms;
   g_pending_last_tick_time_msc = candidate_last_tick_time_msc;

   return QueuePendingBatch();
}

//+------------------------------------------------------------------+
//| Perform Warmup retrieval                                         |
//+------------------------------------------------------------------+
void PerformWarmup()
{
   Print("Performing warmup tick collection...");
   g_current_phase = PHASE_WARMING;
   
   // Set cursor to (TimeCurrent - InpWarmupSeconds)
   datetime from_time = TimeCurrent() - InpWarmupSeconds;
   g_cursor_time_msc = ((long)from_time) * 1000;
   g_cursor_same_ms_count = 0;
   g_current_sequence = 0;
   g_last_acked_sequence = 0;
   g_has_acked_sequence = false;
   g_pending_batch = false;
   g_pending_queued = false;
   ArrayResize(g_pending_packet, 0);
   g_warmup_started = true;

   // Send initial STATUS (WARMING)
   SendStatus(STATUS_CODE_PHASE, PHASE_WARMING, 0, 0, 0, 0, 0);

   // The timer/OnTick driver sends one bounded batch at a time and waits for
   // its ACK before asking CopyTicks for the next one.
}

//+------------------------------------------------------------------+
//| Connect, replay pending data, drain ACKs, then collect one batch |
//+------------------------------------------------------------------+
void DriveReliableCollection()
{
   if(!g_socket.IsConnected())
   {
      if(!g_socket.Connect()) return;
      // SocketClient discards only its connection-local partial suffix.  The
      // immutable pending batch is retained here and is replayed in full.
      if(g_pending_batch) g_pending_queued = false;
   }

   if(!g_warmup_started)
   {
      PerformWarmup();
      return;
   }

   if(!PollAndCommitAck()) return;

   if(g_pending_batch)
   {
      QueuePendingBatch();
      return;
   }

   bool is_warmup = (g_current_phase == PHASE_WARMING);
   if(CollectAndStreamTicks(is_warmup)) return;

   // No pending batch and no more history means warmup is complete.  Queueing
   // failures leave g_pending_batch true, so they cannot cause a false LIVE.
   if(is_warmup)
   {
      g_current_phase = PHASE_LIVE;
      g_warmup_done = true;
      SendStatus(STATUS_CODE_PHASE, PHASE_LIVE, 0, 0, 0, 0, 0);
      PrintFormat("Warmup completed. Transitioned to LIVE at acknowledged seq %I64u", g_current_sequence);
   }
}

//+------------------------------------------------------------------+
//| Expert initialization function                                   |
//+------------------------------------------------------------------+
int OnInit()
{
   g_ea_start_microsecond = GetMicrosecondCount();
   g_session_id = GenerateSessionId();
   g_current_sequence = 0;
   g_last_acked_sequence = 0;
   g_has_acked_sequence = false;
   g_last_tick_time_msc = 0;
   g_last_heartbeat_us = 0;
   g_warmup_done = false;
   g_warmup_started = false;

   g_socket.Init(InpServerHost, InpServerPort, InpSocketTimeoutMs);

   if(g_socket.Connect())
   {
      PerformWarmup();
   }
   else
   {
      PrintFormat("[TickCollector] Initial connection to %s:%d failed. Will automatically retry on timer.", InpServerHost, InpServerPort);
   }

   // The timer is also the connection supervisor. It keeps retrying when
   // TickScope was not running yet, so launch order is irrelevant.
   if(!EventSetMillisecondTimer(InpHeartbeatIntervalMs))
   {
      int timer_error = GetLastError();
      PrintFormat("[TickCollector] Failed to start millisecond timer, error: %d. Falling back to 1-second timer.", timer_error);
      if(!EventSetTimer(1))
      {
         PrintFormat("[TickCollector] Failed to start fallback reconnect timer, error: %d", GetLastError());
      }
   }
   return INIT_SUCCEEDED;
}

//+------------------------------------------------------------------+
//| Expert deinitialization function                                 |
//+------------------------------------------------------------------+
void OnDeinit(const int reason)
{
   EventKillTimer();
   if(g_socket.IsConnected())
   {
      g_socket.Flush();
   }
   g_socket.Disconnect();
   PrintFormat("[TickCollector] Deinitialized (reason %d)", reason);
}

//+------------------------------------------------------------------+
//| Expert tick function                                             |
//+------------------------------------------------------------------+
void OnTick()
{
   DriveReliableCollection();
}

//+------------------------------------------------------------------+
//| Expert timer function                                            |
//+------------------------------------------------------------------+
void OnTimer()
{
   DriveReliableCollection();

   // Heartbeats are independent diagnostics. Their reported sequence is the
   // last ACKed sequence, never an unconfirmed candidate batch.
   ulong now_us = GetEaElapsedUs();
   if(now_us - g_last_heartbeat_us >= ((ulong)InpHeartbeatIntervalMs * 1000))
   {
      SendHeartbeat();
   }

}
//+------------------------------------------------------------------+
