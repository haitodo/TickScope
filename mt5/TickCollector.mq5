//+------------------------------------------------------------------+
//|                                                TickCollector.mq5 |
//|                                  Copyright 2026, TickCompare Team|
//|                                            https://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright "TickCompare Team"
#property link      "https://www.mql5.com"
#property version   "1.00"
#property description "TickCompare Multi-Broker Real-time Tick Collector EA"

#include <TickScope/Protocol.mqh>
#include <TickScope/SocketClient.mqh>

//--- Inputs
input uint   InpBrokerID            = 1;              // Broker ID (1, 2, 3...)
input string InpServerHost          = "127.0.0.1";    // TickCompare Server Host
input uint   InpServerPort          = 39001;          // TickCompare Server Port
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
ushort        g_current_phase          = PHASE_WARMING;
ulong         g_ea_start_microsecond   = 0;

// Cursor tracking
long          g_cursor_time_msc        = 0;
uint          g_cursor_same_ms_count   = 0;
long          g_last_tick_time_msc     = 0;
ulong         g_last_heartbeat_us      = 0;

// Reusable scratch buffers
MqlTick       g_tick_buffer[];
uchar         g_packet_buffer[];

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
                        g_session_id, g_current_sequence,
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

   // Construct TICK_BATCH packet
   uint payload_len = (uint)new_tick_count * TICK_RECORD_LENGTH;
   ArrayResize(g_packet_buffer, HEADER_LENGTH + payload_len);

   ushort hdr_flags = is_warmup ? HEADER_FLAG_WARMUP : 0;
   ulong batch_start_seq = g_current_sequence;

   PackHeader(g_packet_buffer, 0, MSG_TYPE_TICK_BATCH, hdr_flags,
              InpBrokerID, g_session_id, batch_start_seq,
              (uint)new_tick_count, payload_len);

   WireTickRecord wire_tick;
   int offset = HEADER_LENGTH;

   for(int i = 0; i < new_tick_count; i++)
   {
      MqlTick src = g_tick_buffer[start_index + i];

      wire_tick.sequence = g_current_sequence;
      wire_tick.broker_time_msc = src.time_msc;
      wire_tick.ea_elapsed_us = GetEaElapsedUs();
      wire_tick.bid = src.bid;
      wire_tick.ask = src.ask;
      wire_tick.last = src.last;
      wire_tick.volume = src.volume;
      wire_tick.volume_real = src.volume_real;
      wire_tick.flags = src.flags;
      wire_tick.reserved = 0;

      PackTickRecord(g_packet_buffer, offset, wire_tick);
      offset += TICK_RECORD_LENGTH;

      // Advance sequence
      g_current_sequence++;
      g_last_tick_time_msc = src.time_msc;

      // Advance cursor
      if(src.time_msc == g_cursor_time_msc)
      {
         g_cursor_same_ms_count++;
      }
      else
      {
         g_cursor_time_msc = src.time_msc;
         g_cursor_same_ms_count = 1;
      }
   }

   g_socket.QueueBytes(g_packet_buffer, ArraySize(g_packet_buffer));
   return true;
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

   // Send initial STATUS (WARMING)
   SendStatus(STATUS_CODE_PHASE, PHASE_WARMING, 0, 0, 0, 0, 0);

   // Collect backlog until current time
   int loops = 0;
   while(loops < 50)
   {
      if(!CollectAndStreamTicks(true))
      {
         break;
      }
      loops++;
   }

   // Transition to LIVE
   g_current_phase = PHASE_LIVE;
   SendStatus(STATUS_CODE_PHASE, PHASE_LIVE, 0, 0, 0, 0, 0);
   PrintFormat("Warmup completed. Transitioned to LIVE at seq %d", g_current_sequence);
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
   g_last_tick_time_msc = 0;
   g_last_heartbeat_us = 0;

   g_socket.Init(InpServerHost, InpServerPort, InpSocketTimeoutMs);

   if(!g_socket.Connect())
   {
      PrintFormat("Initial connection to %s:%d failed. Will retry on timer.", InpServerHost, InpServerPort);
   }

   PerformWarmup();

   EventSetMillisecondTimer(InpHeartbeatIntervalMs);
   return INIT_SUCCEEDED;
}

//+------------------------------------------------------------------+
//| Expert deinitialization function                                 |
//+------------------------------------------------------------------+
void OnDeinit(const int reason)
{
   EventKillTimer();
   g_socket.Flush();
   g_socket.Disconnect();
   PrintFormat("TickCollector deinitialized (reason %d)", reason);
}

//+------------------------------------------------------------------+
//| Expert tick function                                             |
//+------------------------------------------------------------------+
void OnTick()
{
   // Invariant I01: OnTick is a retrieval trigger. We collect all new ticks.
   int passes = 0;
   while(passes < 8) // Bounded execution budget
   {
      if(!CollectAndStreamTicks(g_current_phase == PHASE_WARMING))
      {
         break;
      }
      passes++;
   }
   
   g_socket.Flush();
}

//+------------------------------------------------------------------+
//| Expert timer function                                            |
//+------------------------------------------------------------------+
void OnTimer()
{
   // 1. Flush any pending buffered bytes
   g_socket.Flush();

   // 2. Poll ACKs from server
   g_socket.PollReplies(g_last_acked_sequence);

   // 3. Check heartbeat interval
   ulong now_us = GetEaElapsedUs();
   if(now_us - g_last_heartbeat_us >= ((ulong)InpHeartbeatIntervalMs * 1000))
   {
      SendHeartbeat();
   }

   // 4. Drain any pending tick backlog in case of quiet market
   CollectAndStreamTicks(g_current_phase == PHASE_WARMING);
}
//+------------------------------------------------------------------+
