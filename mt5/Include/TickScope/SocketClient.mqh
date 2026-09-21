//+------------------------------------------------------------------+
//|                                                 SocketClient.mqh |
//|                                  Copyright 2026, TickCompare Team|
//|                                            https://www.mql5.com |
//+------------------------------------------------------------------+
#property copyright "TickCompare Team"
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
   
   bool IsConnected() const { return m_connected; }
   
   bool Connect()
   {
      if(m_connected) return true;
      
      ulong now_msc = GetMicrosecondCount() / 1000;
      if(now_msc - m_last_connect_attempt_msc < 1000)
      {
         return false; // reconnect cooldown 1000ms
      }
      m_last_connect_attempt_msc = now_msc;
      
      // Invariant: Do not carry over partial write buffer to a brand new connection
      ClearSendBuffer();
      
      m_socket = SocketCreate();
      if(m_socket == INVALID_HANDLE)
      {
         PrintFormat("SocketCreate failed, error: %d", GetLastError());
         return false;
      }
      
      if(!SocketConnect(m_socket, m_host, m_port, m_timeout_ms))
      {
         int err = GetLastError();
         SocketClose(m_socket);
         m_socket = INVALID_HANDLE;
         return false;
      }
      
      m_connected = true;
      PrintFormat("Connected to TickCompare Rust Receiver at %s:%d", m_host, m_port);
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
      if(!m_connected)
      {
         if(!Connect()) return false;
      }
      
      while(m_send_offset < m_send_len)
      {
         int to_send = m_send_len - m_send_offset;
         uchar chunk[];
         ArrayResize(chunk, to_send);
         ArrayCopy(chunk, m_send_buffer, 0, m_send_offset, to_send);
         
         int sent = SocketSend(m_socket, chunk, to_send);
         if(sent > 0)
         {
            m_send_offset += sent;
         }
         else
         {
            int err = GetLastError();
            // In non-blocking socket, 0 or error might mean socket full or disconnected
            if(err != 0 && err != 5273) // 5273 = ERR_NETSOCKET_WOULDBLOCK
            {
               PrintFormat("SocketSend error: %d, disconnecting", err);
               Disconnect();
               return false;
            }
            break; // Socket full for now, will retry next call
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
      if(!m_connected) return;
      
      uint readable = SocketIsReadable(m_socket);
      if(readable > 0)
      {
         int read = SocketRead(m_socket, m_recv_buffer, ArraySize(m_recv_buffer), m_timeout_ms);
         if(read > 0)
         {
            // Parse BATCH_ACK (Header 40 bytes + 8 bytes seq_end = 48 bytes)
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
      }
   }
};
