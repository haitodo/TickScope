//+------------------------------------------------------------------+
//|                                                 SocketClient.mqh |
//|                                  Copyright 2026, TickScope Team  |
//|                                            https://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright "TickScope Team"
#property link      "https://www.mql5.com"
#property strict

#include <TickScope/Protocol.mqh>

class CSocketClient
{
private:
   int      m_socket;
   string   m_host;
   uint     m_port;
   uint     m_timeout_ms;
   bool     m_connected;
   ulong    m_last_connect_attempt_msc;
   bool     m_last_connect_logged;
   
   // Partial write buffer
   uchar    m_send_buffer[];
   int      m_send_offset;
   int      m_send_len;
   
   // Reply read buffer
   uchar    m_recv_buffer[];
   int      m_recv_len;

public:
   CSocketClient()
   {
      m_socket = INVALID_HANDLE;
      m_host = "127.0.0.1";
      m_port = 39001;
      m_timeout_ms = 10;
      m_connected = false;
      m_last_connect_attempt_msc = 0;
      m_last_connect_logged = false;
      m_send_offset = 0;
      m_send_len = 0;
      m_recv_len = 0;
      ArrayResize(m_send_buffer, 0);
      ArrayResize(m_recv_buffer, 4096);
   }
   
   ~CSocketClient()
   {
      Disconnect();
   }
   
   void Init(string host, uint port, uint timeout_ms)
   {
      m_host = host;
      m_port = port;
      m_timeout_ms = timeout_ms;
   }
   
   bool IsConnected()
   {
      if(!m_connected || m_socket == INVALID_HANDLE)
      {
         return false;
      }
      ResetLastError();
      if(!SocketIsConnected(m_socket))
      {
         PrintFormat("[TickCollector] Socket connection lost (SocketIsConnected == false). Disconnecting.");
         Disconnect();
         return false;
      }
      return true;
   }
   
   bool Connect()
   {
      if(IsConnected()) return true;
      
      ulong now_msc = GetMicrosecondCount() / 1000;
      // Allow immediate initial connection attempt (m_last_connect_attempt_msc == 0), then 1000ms cooldown
      if(m_last_connect_attempt_msc > 0 && (now_msc - m_last_connect_attempt_msc < 1000))
      {
         return false; // reconnect cooldown 1000ms
      }
      m_last_connect_attempt_msc = (now_msc == 0) ? 1 : now_msc;

      // Clean up previous socket if left open
      if(m_socket != INVALID_HANDLE)
      {
         SocketClose(m_socket);
         m_socket = INVALID_HANDLE;
      }
      m_connected = false;
      
      // Invariant: Do not carry over partial write buffer to a brand new connection
      ClearSendBuffer();
      
      m_socket = SocketCreate();
      if(m_socket == INVALID_HANDLE)
      {
         int err = GetLastError();
         PrintFormat("[TickCollector] SocketCreate failed, error: %d", err);
         return false;
      }
      
      // 200ms is plenty for localhost TCP 3-way handshake.
      // Keeping it small prevents blocking EA thread when TickScope is not running yet.
      uint connect_timeout = 200;
      ResetLastError();
      if(!SocketConnect(m_socket, m_host, m_port, connect_timeout))
      {
         int err = GetLastError();
         if(!m_last_connect_logged)
         {
            if(err == 4014)
            {
               PrintFormat("[TickCollector] SocketConnect to %s:%d FAILED: Error 4014 (Function not allowed). "
                           "Please open MT5: Tools -> Options -> Expert Advisors -> check 'Allow WebRequest for listed URL' "
                           "and add '%s', 'http://%s', and 'http://%s:%d'",
                           m_host, m_port, m_host, m_host, m_host, m_port);
            }
            else if(err == 5272)
            {
               PrintFormat("[TickCollector] SocketConnect to %s:%d FAILED: Error 5272 (Cannot connect). "
                           "TickScope app is not running yet. Retrying automatically in background...",
                           m_host, m_port);
            }
            else if(err == 5273)
            {
               PrintFormat("[TickCollector] SocketConnect to %s:%d FAILED: Error 5273 (Timeout). "
                           "Retrying automatically in background...", m_host, m_port);
            }
            else
            {
               PrintFormat("[TickCollector] SocketConnect to %s:%d FAILED: Error %d. "
                           "Retrying automatically in background...", m_host, m_port, err);
            }
            m_last_connect_logged = true;
         }
         SocketClose(m_socket);
         m_socket = INVALID_HANDLE;
         return false;
      }
      
      m_connected = true;
      m_last_connect_logged = false;
      PrintFormat("[TickCollector] Successfully CONNECTED to TickScope at %s:%d", m_host, m_port);
      return true;
   }
   
   void Disconnect()
   {
      if(m_socket != INVALID_HANDLE)
      {
         SocketClose(m_socket);
         m_socket = INVALID_HANDLE;
      }
      m_connected = false;
      ClearSendBuffer();
   }
   
   void ClearSendBuffer()
   {
      m_send_offset = 0;
      m_send_len = 0;
      ArrayResize(m_send_buffer, 0);
   }
   
   bool HasPendingSend() const
   {
      return (m_send_len > 0 && m_send_offset < m_send_len);
   }
   
   // Queue data to send
   bool QueueBytes(const uchar &data[], int data_len)
   {
      if(data_len <= 0) return true;
      
      // If there's an existing unsent suffix, append
      int remaining = m_send_len - m_send_offset;
      if(remaining > 0)
      {
         uchar temp[];
         ArrayResize(temp, remaining + data_len);
         ArrayCopy(temp, m_send_buffer, 0, m_send_offset, remaining);
         ArrayCopy(temp, data, remaining, 0, data_len);
         ArrayCopy(m_send_buffer, temp);
         m_send_offset = 0;
         m_send_len = remaining + data_len;
      }
      else
      {
         ArrayResize(m_send_buffer, data_len);
         ArrayCopy(m_send_buffer, data, 0, 0, data_len);
         m_send_offset = 0;
         m_send_len = data_len;
      }
      
      return Flush();
   }
   
   // Attempt to flush pending bytes with deadline
   bool Flush()
   {
      if(!IsConnected())
      {
         if(!Connect()) return false;
      }
      
      while(m_send_offset < m_send_len)
      {
         int to_send = m_send_len - m_send_offset;
         uchar chunk[];
         ArrayResize(chunk, to_send);
         ArrayCopy(chunk, m_send_buffer, 0, m_send_offset, to_send);
         
         ResetLastError();
         int sent = SocketSend(m_socket, chunk, to_send);
         if(sent > 0)
         {
            m_send_offset += sent;
         }
         else
         {
            int err = GetLastError();
            // 0 = Would block / no bytes transferred yet
            if(err == 0)
            {
               break; // Socket full for now, will retry next call without disconnecting
            }
            else
            {
               // 5273 (ERR_NETSOCKET_IO_ERROR), 5270 (ERR_NETSOCKET_INVALID_HANDLE), etc.
               // Remote closed connection or broken socket
               PrintFormat("[TickCollector] SocketSend error: %d, disconnecting.", err);
               Disconnect();
               return false;
            }
         }
      }
      
      if(m_send_offset >= m_send_len)
      {
         ClearSendBuffer();
      }
      
      return true;
   }
   
   // Drain replies (ACKs)
   void PollReplies(ulong &last_acked_seq)
   {
      if(!IsConnected()) return;
      
      ResetLastError();
      uint readable = SocketIsReadable(m_socket);
      int is_read_err = GetLastError();
      if(is_read_err != 0 && is_read_err != 5273)
      {
         PrintFormat("[TickCollector] SocketIsReadable error: %d, disconnecting.", is_read_err);
         Disconnect();
         return;
      }

      // MQL5 SocketIsReadable can speculatively return 1 when buffer is empty.
      // BATCH_ACK is 48 bytes (Header 40 + SeqEnd 8).
      // Only proceed if at least 48 bytes are ready.
      if(readable < 48) return;
      
      uint to_read = (readable > (uint)ArraySize(m_recv_buffer)) ? (uint)ArraySize(m_recv_buffer) : readable;
      
      // Use 0 timeout because SocketIsReadable indicated data is already buffered.
      ResetLastError();
      int read = SocketRead(m_socket, m_recv_buffer, to_read, 0);
      if(read > 0)
      {
         int offset = 0;
         while(offset + 48 <= read)
         {
            ushort msg_type = (ushort)(m_recv_buffer[offset + 6] | (m_recv_buffer[offset + 7] << 8));
            if(msg_type == MSG_TYPE_BATCH_ACK)
            {
               ulong seq_end = 0;
               for(int i = 0; i < 8; i++)
               {
                  seq_end |= ((ulong)m_recv_buffer[offset + 40 + i]) << (i * 8);
               }
               if(seq_end > last_acked_seq)
               {
                  last_acked_seq = seq_end;
               }
            }
            offset += 48;
         }
      }
      else if(read < 0)
      {
         int err = GetLastError();
         if(err != 0)
         {
            PrintFormat("[TickCollector] SocketRead fatal error: %d, disconnecting.", err);
            Disconnect();
         }
      }
   }
};
