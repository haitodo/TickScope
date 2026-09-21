# FX 2社リアルタイム・ティック比較／ローソク足表示アプリ 詳細設計書（改訂版）

> Revision: 1.3 / 2026-09-21
>
> 目的: 元設計の「MT5 → MQL5 → localhost → Rust → egui」という基本方針を維持しつつ、ティック取得境界、Heartbeat、Socket安全性、IPC遅延、UI共有状態、Backpressure、固定時間スロット、Lead/Lagイベント定義に加え、Wire Format、TCP Framing、トレンド時のイベント再アンカー、時刻正規化の検証手順を明確化する。

## 1. 文書概要

### 1.1 仮称

**TickCompare**

本アプリは、2つのFX業者のMetaTrader 5（MT5）からリアルタイムのティックデータを取得し、同一PC上で比較・可視化するためのWindowsネイティブアプリケーションである。

主用途は、**USD/JPYを中心とした秒単位の超短期スキャルピングにおいて、複数業者の価格配信状態を同時監視すること**である。

本アプリは発注システムではない。注文送信、ポジション管理、自動売買は行わず、**価格配信の観察・比較・可視化に専念する**。

---

# 2. 開発目的

## 2.1 背景

FXの短期スキャルピングでは、同じUSD/JPYであってもブローカーごとに、

- Bid
- Ask
- Spread
- Tick到着タイミング
- 価格更新頻度
- 価格の瞬間的な飛び
- スプレッド拡大
- ローソク足の形成

などに差が存在する。

一般的なチャートソフトでは、単一ブローカーの価格を確認することはできるが、**2社のティックを同一PC上で同一時間軸で比較すること**には向いていない。

そのため、本アプリでは2社のMT5をデータ源として使用し、ティックを独自に収集・比較する。

---

# 3. 想定利用シーン

主な利用シーンは以下。

### 3.1 数秒保有のスキャルピング

例：

1. OANDAのUSD/JPYが急に上昇
2. Axiory側のBid/Askを確認
3. どちらが先に動いたかを確認
4. 両社の価格差・Spreadを確認
5. 数秒間の値動きを監視

この判断を、**目視で2つのMT5チャートを比較するよりも高密度な情報で行う**。

### 3.2 ブローカー間の価格差確認

例えば、

```text
OANDA
Bid  158.234
Ask  158.236

Axiory
Bid  158.232
Ask  158.234

Bid差    +0.002
Ask差    +0.002
```

のようにリアルタイムで表示する。

### 3.3 短時間の価格先行・追随の観察

ある価格変化について、

```text
OANDA → 先に本PCへ観測
Axiory → 数ms後に対応変化を観測
```

のような状態を検出・表示する。

ただし、この値は**市場そのものの因果関係や「真の市場先行」を証明するものではない**。

また、MQL5 EAはティックをまとめてTCP送信できるため、Lead/Lagは「個々の元ティックが市場で発生した時刻」ではなく、**このPCで対応する価格変化を観測できた時刻**を基本時間軸とする。

ブローカーのサーバー処理、ネットワーク、MT5端末側のイベント処理、EAのBatch送信などを含むため、本アプリでは「このPC上の受信経路における観測上の先行・遅延」として扱う。

---

# 4. 最重要設計方針

以下を設計上の原則とする。

### 4.1 価格データはMT5から取得

外部FX APIから価格を取得するのではなく、

```text
MT5 A社
MT5 B社
```

をリアルタイムデータ源とする。

### 4.2 MT5はデータ取得専用

MT5側では、

- ティック取得
- 必要な初期履歴取得
- Rustアプリへのデータ送信

だけを行う。

ローソク足生成・価格差計算・描画はRust側で行う。

### 4.3 GUIはデータ処理を行わない

GUIスレッドでティック処理を行わない。

```text
Tick受信
    ↓
Tick Engine
    ↓
Candle / Difference / Lead-Lag
    ↓
UI Snapshot
    ↓
GUI
```

という完全分離とする。

### 4.4 描画はegui/eframeで自前実装

汎用チャートライブラリは使用しない。

今回必要なのは、

- ローソク足
- 価格線
- 差分
- Spread
- 数値表示
- 固定レイアウト

程度であり、eguiの`Painter`で線、矩形、テキスト等を直接描画できる。

eframeはWindowsネイティブアプリとして起動できる。設計時点の公式ドキュメントではeframe/egui 0.36.2が掲載されている。

---

# 5. 対象プラットフォーム

## 5.1 OS

Windows 11 64bitを第一対象とする。

Linux/macOS対応は不要。

## 5.2 CPU

x86_64。

## 5.3 GPU

内蔵GPUで問題ないことを前提とする。

高性能GPUは必要としない。

---

# 6. 全体アーキテクチャ

```text
                    ┌────────────────────┐
                    │     MT5 A社        │
                    │                    │
                    │ MQL5 EA            │
                    │ OnTick + CopyTicks │
                    └─────────┬──────────┘
                              │
                        localhost TCP
                              │
                              ▼
                    ┌────────────────────┐
                    │                    │
                    │ Rust Tick Receiver │
                    │                    │
                    └─────────┬──────────┘
                              │
                              │
                    ┌─────────┴──────────┐
                    │                    │
                    │    Tick Engine     │
                    │                    │
                    │ • Tick整列         │
                    │ • Candle生成       │
                    │ • Price差計算      │
                    │ • Spread計算       │
                    │ • Lead/Lag         │
                    │                    │
                    └─────────┬──────────┘
                              │
             ┌────────────────┼────────────────┐
             │                │                │
             ▼                ▼                ▼
        Ring Buffer      Logger          UI Snapshot
             │                                 │
             │                                 ▼
             │                       ┌─────────────────┐
             │                       │ egui / eframe  │
             │                       │                 │
             │                       │ • Candle       │
             │                       │ • Tick line    │
             │                       │ • Difference   │
             │                       │ • Spread       │
             │                       │ • Status       │
             │                       └─────────────────┘
             │
             │
             ▲
             │
      ┌──────┴──────────┐
      │                 │
      │ localhost TCP   │
      │                 │
      └──────▲──────────┘
             │
     ┌───────┴───────────────┐
     │                       │
┌────┴───────────┐   ┌───────┴─────────┐
│ MT5 B社        │   │ MQL5 EA         │
│                │   │ OnTick+CopyTicks│
└────────────────┘   └─────────────────┘
```

---

# 7. MT5側設計

## 7.1 MT5は2インスタンス

1台のMT5に2社を混在させる方式ではなく、

```text
MT5 Instance A
    ↓
Broker A

MT5 Instance B
    ↓
Broker B
```

とする。

これにより各MT5は、それぞれの業者のリアルタイム配信を直接受信する。

---

# 8. MQL5 EAの責務

各MT5に専用EAを1個配置する。

EAの責務は以下のみ。

1. 対象Symbolのティック監視
2. `OnTick()`をトリガーとして`CopyTicks()`を実行
3. 未処理ティックを回収
4. sequence番号を付与
5. MQL5内部計測時刻を取得
6. Rustアプリへ送信
7. 接続切断時の再接続
8. 異常状態の通知
9. `CopyTicks()`を上限付きBatchで回収し、必要なら複数回に分割して追いつく
10. Socket送信時間を上限管理し、送信失敗時に無限ブロックしない
11. Heartbeatを定期送信する
12. Rustからの制御Replyを受信可能にする

注文処理は一切行わない。

---

# 9. OnTick単独で処理しない

MQL5の`OnTick()`は新しいティックごとに必ず1回呼ばれる仕組みではない。公式仕様では、NewTickイベントが既にキューに存在するか処理中であれば、新しいNewTickイベントは追加されない。したがって、`OnTick()`を「ティックそのもの」とみなして1回につき1ティックを送信する設計は禁止する。

必ず、

```text
OnTick()
    ↓
CopyTicks(COPY_TICKS_ALL)
    ↓
前回以降の未処理ティックを回収
```

とする。

`CopyTicks()`は、端末側に同期されたティックデータベースから受信済みティックを取得でき、`COPY_TICKS_ALL`では全種類のティックを対象にできる。したがって、**OnTickのイベント数と実ティック数を1対1対応させない**ことが本設計の重要な不変条件である。

---

# 10. Tickカーソル管理

ティック取得位置は、単純な

```text
last_time_msc + 1
```

では管理しない。`time_msc`は一意IDではなく、同一ミリ秒に複数ティックが存在し得る。

基本カーソルとして、

```text
last_time_msc
last_same_time_count
```

を保持する。

概念：

```text
last_time_msc = 123456789000
last_same_time_count = 3
```

次回は同一`time_msc`を含む位置から再取得し、既に処理した3件を基準として新規分を処理する。`from = last_time_msc + 1` のように単純に境界を進める方式は禁止する。

### 10.1 同一ms境界の安全策

`time_msc`だけではティックを一意識別できないため、**指紋Hashを単純な重複排除キーとして使用しない**。完全に同一内容の正当なティックが複数回存在し得るためである。

必要に応じて、直近の同一`time_msc`ブロックを保持し、再取得時に

```text
時間 + tick内容 + 同一ms内の出現数
```

の組み合わせで境界を検証する。これは「不正な重複排除」のためではなく、**カーソル境界の再読込による二重処理を検出・診断するため**に使用する。

### 10.2 TickCursorの不変条件

- EAからRustへ送信済みsequenceは単調増加する。
- `broker_time_msc`は保存するが、一意キーとはみなさない。
- 同一msの複数ティックを欠落させない。
- カーソル境界では必要に応じて同一msの再読込を行う。
- 取得異常があった場合、黙って先へ進めず診断状態を残す。

---

### 10.3 CopyTicksの取得上限と追随ループ

ライブ取得経路では`CopyTicks()`の`count`を省略しない。`count=0`を暗黙の既定値として利用せず、明示的なBatch上限を設定する。

初期値候補：

```text
copy_batch_count = 256
```

ただし、これは「1回のOnTickで256件までしか処理できない」という意味ではない。戻り件数が上限に達した場合は、同一カーソル境界から次のBatchを追加取得して追随する。

概念：

```text
OnTick
  ↓
CopyTicks(count=256)
  ↓
256件取得
  ↓
未処理分を送信
  ↓
まだ上限到達？ ── Yes → 次Batch
  ↓ No
return
```

ただし、1回のイベント処理がEAを長時間占有しないよう、以下の上限も設ける。

```text
max_copy_calls_per_event
max_ticks_per_event
max_processing_time_us
```

上限に達した場合は`TICK_BACKLOG`として状態を記録し、次の`OnTick()`で継続する。

`MqlTick`配列は再利用し、通常経路で毎回新規配列を作らない。

MQL5公式仕様では、初回の`CopyTicks()`が端末のTickデータベース同期を開始し、EAでは同期完了待ちが最大45秒となる場合がある。したがって、ウォームアップ時の長い応答時間と通常のライブ増分取得を同一視しない。ウォームアップ中はGUIを`WARMING`として扱い、ライブ経路の診断値も別管理する。

# 11. Tick取得モード

基本：

```text
COPY_TICKS_ALL
```

を使用する。

理由は、Bid/AskだけでなくLast、Volume、Flagsを保持し、後から解析可能にするため。

`MqlTick`には、

- time
- bid
- ask
- last
- volume
- time_msc
- flags
- volume_real

が含まれる。

---

# 12. EA起動時のウォームアップ

EA起動時に、設定した秒数分の最近のティックを取得する。

初期値：

```text
warmup_seconds = 60
```

目的：

- アプリ起動直後でもローソク足がいきなり1本だけにならない
- 直近の価格差波形を表示できる
- 現在形成中の足を途中から再現できる

ウォームアップ処理中はGUIに、

```text
OANDA: WARMING UP
Axiory: CONNECTING
```

のように表示する。

---

# 13. MQL5時刻

各ティックについて最低限、

```text
broker_time_msc
```

を送る。

`MqlTick.time_msc`はミリ秒単位の価格更新時刻として提供される。

ただし、異なる業者のサーバー時刻をそのまま「絶対的に同期した時刻」と解釈しない。

ログには**生のブローカー時刻をそのまま保存**する。

### 13.1 UTC基準への対応

2社のローソク足を共通時間軸へ載せるため、`broker_time_msc`とは別に`normalized_time_utc_ms`を生成する。

ただし、MQL5の`TimeCurrent()`と`TimeGMT()`の差を、常時正確なブローカーUTCオフセットとして固定使用してはならない。

MQL5公式仕様では、`TimeCurrent()`は**最後に受信したMarket Watch上のQuoteに基づくサーバー時刻**であり、`OnTimer()`等では「現在時刻」ではなく最後のQuote時刻になり得る。一方、`TimeGMT()`は**MT5が稼働するPCのローカル時刻から計算したGMT**である。したがって、

```text
TimeCurrent() - TimeGMT()
```

は、ライブ運用での恒久的なUTC変換係数として保証されない。

この差は、必要に応じて**診断用のcoarse offset sample**として記録してよいが、Lead/Lag時計やミリ秒精度のUTC変換の唯一の根拠にはしない。

### 13.1.1 正規化方針

MVPでは、共通時間軸用のUTC offsetを次の優先順で扱う。

```text
1. 設定された broker_utc_offset_sec
2. 実端末でのcalibrationによる検証値
3. MQL5の時刻APIから得た値は診断・照合用
```

設定例：

```toml
[broker_a.time]
source = "manual"
broker_utc_offset_sec = 10800

[broker_b.time]
source = "manual"
broker_utc_offset_sec = 7200
```

ここで設定する値は「永久固定値」とは扱わず、DSTやブローカー設定変更時に更新可能な構成値とする。

### 13.1.2 Calibration Test

自動校正を実装する場合は、MQL5の1回の時刻差だけを使用しない。複数サンプルについて、

```text
candidate_offset_sec
residual_ms
sample_count
stability
```

を評価し、既知のPC wall-clock精度・ネットワーク受信遅延を考慮して妥当性を確認する。

`broker_time_msc`からのUTC正規化は、**ローソク足の共通Slot整列用**であり、Lead/Lag算出には使用しない。

Lead/LagにはRustの共通単調時計`rx_mono_ns`のみを使用する。

# 14. MQL5側の高精度計測時刻

必要に応じて、

```text
ea_elapsed_us
```

も記録する。

MQL5の`GetMicrosecondCount()`は、そのMQL5プログラム開始からの経過時間をマイクロ秒単位で返す。

これは、

- EA内部処理時間
- CopyTicks取得時間
- Packet構築時間
- TCP送信処理時間

を診断する目的で使用する。

ただし、2つのEA間でこの値を直接比較して「OANDAの方が何µs早い」とは解釈しない。

### 14.1 Rust側の単調時計

Lead/Lagの時間差は、Rust側で同一PC上の共通単調時計を使用する。

```text
Instant::now()
```

を基準とし、TCPフレーム受信・デコード時点の`rx_mono_ns`を記録する。

別途、ログ用途としてwall-clockの`rx_unix_ns`も保存してよい。

**重要:** TCPはストリームであり、1回の`SocketSend`と1回の`read()`は1対1対応しない。そのため`rx_mono_ns`は「個々の市場ティック発生時刻」ではなく、**そのTickを含むフレームを本PCで観測した受信時刻**として定義する。

---

# 15. MQL5 → Rust通信方式

## 第一候補：localhost TCP

MQL5には標準Socket APIがあり、

```text
SocketCreate
SocketConnect
SocketIsWritable
SocketSend
SocketTimeouts
SocketClose
```

等が利用できる。

Rust側は、

```text
127.0.0.1:39001 = Broker A
127.0.0.1:39002 = Broker B
```

のように2ポートをlistenする。

インターネット経由ではない。

---

# 16. localhost TCPを採用する理由

本アプリでは、

- DLLを使わない
- Win32 API直接呼び出しを避ける
- MQL5標準APIだけで構成する
- Rust側実装を単純化する
- 再接続処理を簡単にする
- テスト・デバッグしやすくする

ことを優先し、**localhost TCPを第一候補とする**。

Named Pipeは、TCPの実測で有意な遅延・ジッター問題が確認された場合の第二候補とする。

### 16.1 TCP_NODELAYに関する設計方針

MQL5標準Socket APIの公開インターフェースには、`TCP_NODELAY`を設定するためのAPIが明示的に提供されていない。そのため、

- 「MQL5のSocketは必ずNagleが有効」
- 「毎回数十～200msの遅延が発生する」

とは設計上断定しない。

また、Rust側で`TcpStream::set_nodelay(true)`を設定しても、**MQL5→Rust方向の送信側Nagleを無効化するものではない**。これはRust→MQL5方向のTCP送信設定である。

したがって、MVPでは次を実施する。

1. Rust receiver側の`TcpStream`は`set_nodelay(true)`を設定する。
2. MQL5→Rustの遅延は、実測テストでp50/p95/p99を取得する。
3. Batchサイズと送信頻度を固定した上で、TCPループバックのジッターを評価する。
4. 有意な問題が確認された場合のみ、Named Pipe等を代替方式として比較する。

TCP_NODELAYの有無を仕様上の保証値ではなく、**実測・比較対象の非機能要件**として扱う。

---

# 17. MQL5 Socket設定

MQL5のSocket機能では接続先アドレスの許可設定が必要であり、アドレスをプログラムから追加することはできない。したがって初回セットアップ時に`127.0.0.1`を許可する手順を用意する。

EA起動時に、

```text
TickCompare requires local socket access.

Please allow:
127.0.0.1
```

などの案内を表示する。

---

### 17.1 Socket送信安全策

MQL5のSocketはシステムレベルではblocking socketとして扱われるため、`SocketSend()`がEA処理を長時間占有しないよう送信Timeoutを設定する。`SocketIsWritable()`も送信前のガードとして利用する。

ただし、`SocketIsWritable()`がtrueでも直後の`SocketSend()`成功を保証するものではない。そのため、最終的な安全装置は`SocketTimeouts()`と`SocketSend()`の戻り値確認とする。

初期設定候補：

```text
socket_send_timeout_ms = 10
socket_receive_timeout_ms = 10
```

この値は固定保証値ではなく、実測に基づいて調整する。5msを絶対値として仕様固定しない。

`SocketSend()`の戻り値が要求長より短い場合も成功扱いにせず、未送信部分を明示管理する。再送できない状態になった場合は無限待ちせず、`TRANSPORT_FAULT`へ遷移して再接続処理へ移行する。

通常運用では生Tickを静かに捨てない。ただしRustが停止している等の障害時まで無限メモリ保持を行う設計にはしない。送信待ちバッファには有限上限を設け、上限超過時は`DATA_LOSS`と欠落件数を記録する。

# 18. TCPプロトコル

テキスト通信は禁止する。

以下は使用しない。

```text
JSON
CSV
TSV
文字列フォーマット
```

採用：

```text
binary framed protocol
```

---

### 18.1 双方向Control Reply

TCPのDelayed ACKと送信側Nagleの相互作用が実環境で遅延スパイクを起こす可能性があるため、プロトコルとして任意のControl Replyを定義する。

```text
MQL5 → Rust : TICK_BATCH
Rust → MQL5 : BATCH_ACK(sequence_end)
```

Rust側のTCP送信には`TCP_NODELAY`を設定可能とし、`BATCH_ACK`は極小の固定長フレームとする。

ただし、`BATCH_ACK`をNagle対策として無条件に毎Packet送信することはMVPの必須条件にはしない。

```text
ack_mode = off | diagnostic | forced
```

`diagnostic`ではTCPのp50/p95/p99遅延とACK有無を比較する。`forced`は実測で有効性が確認された場合のみ採用する。

MQL5側は`SocketIsReadable()`でReplyの有無を確認し、読み取り可能なControl Frameを短時間で排出する。Control ReplyはTickデータそのものではなく、送信状態・遅延診断・フロー制御補助に使用する。

この仕組みを「TCP Delayed ACKを必ず解消する保証」とは扱わない。最終判定は実測する。

# 18.2 TCP Framing / 再同期戦略

TCPはメッセージ境界を保持しないため、`SocketSend()` 1回とRust側の`read()` 1回を1対1対応させない。

本プロトコルでは、以下のLength-Prefixed Framingを採用する。

```text
[fixed Header][payload]
```

Headerには少なくとも、

```text
magic
protocol_version
message_type
header_length
header_flags
broker_id
session_id
sequence_start
tick_count
payload_length
```

を含める。

Rust Decoderは内部のreceive bufferにデータを蓄積し、次の不変条件で処理する。

```text
1. Header全長未満なら追加readを待つ
2. magic / version / header_lengthを検証
3. payload_length <= max_frame_payload_size を検証
4. Header + payload 全体が揃うまで待つ
5. フレーム全体が揃ったらdecodeしてEngineへ渡す
6. 処理済みバイトだけbuffer先頭から消費する
```

### 18.2.1 異常フレーム処理

TCPは順序保証・再送を行うため、通常のネットワーク条件で「途中の1バイトだけ壊れる」ことを前提にはしない。

したがって、Production Modeでは、

```text
invalid magic
invalid version
invalid header length
payload length exceeds maximum
malformed field
```

を検出した場合、無制限のMagic Scanで復旧しようとせず、**当該接続を異常終了して再接続する**ことを基本とする。

一方、Protocol Fuzz Test / Debug Modeでは、誤った先頭位置からの復旧を検証するため、有限長の`magic`スキャンを許可してよい。

```text
resync_scan_limit = 64 KiB  // 初期候補
```

この上限を超えても有効なHeaderを発見できなければ接続を破棄する。

また、絶対上限として、

```text
max_frame_payload_size
```

を設定し、受信データのLengthフィールドだけでメモリを無制限に確保しない。

# 19. Packet設計

論理的なフィールドだけでなく、**Wire Formatのバイト配置を仕様として固定する**。

Rustの`#[repr(C)]`やMQL5の構造体レイアウトへ依存せず、両側で明示的にLittle Endianへserialize / deserializeする。`transmute`や生ポインタcastによる構造体直読みは使用しない。

## 19.1 固定Header Wire Format

Headerは40 bytesとする。

| Offset | Size | Field | Type |
|---:|---:|---|---|
| 0 | 4 | magic | u32 |
| 4 | 2 | protocol_version | u16 |
| 6 | 2 | message_type | u16 |
| 8 | 2 | header_length | u16 |
| 10 | 2 | header_flags | u16 |
| 12 | 4 | broker_id | u32 |
| 16 | 8 | session_id | u64 |
| 24 | 8 | sequence_start | u64 |
| 32 | 4 | tick_count | u32 |
| 36 | 4 | payload_length | u32 |

`header_length`は現行値40とし、将来拡張時に後続フィールドを追加できるようにする。

Magicは例として、

```text
0x5449434B   // "TICK"
```

を使用する。Endian上のWire Byte列もテストで固定する。

## 19.2 TickRecord Wire Format

1 Tick = 72 bytes。

| Offset | Size | Field | Type |
|---:|---:|---|---|
| 0 | 8 | sequence | u64 |
| 8 | 8 | broker_time_msc | i64 |
| 16 | 8 | ea_elapsed_us | u64 |
| 24 | 8 | bid | f64 |
| 32 | 8 | ask | f64 |
| 40 | 8 | last | f64 |
| 48 | 8 | volume | u64 |
| 56 | 8 | volume_real | f64 |
| 64 | 4 | flags | u32 |
| 68 | 4 | reserved | u32 |

`reserved`は現行では0とし、将来拡張用に確保する。これはコンパイラの暗黙paddingを利用するためではなく、**Wire Protocol上の明示的なフィールド**である。

したがって、物理サイズ72 bytesはコンパイラのABIに依存せず、Rust/MQL5双方で明示的に検証する。

## 19.3 Wire Formatの原則

- すべてLittle Endian
- Wire上の整数・浮動小数点サイズを固定
- 送信前に明示serialize
- 受信後に明示deserialize
- `#[repr(packed)]`による非整列参照を使用しない
- `#[repr(C)]`はWire Formatの正しさを保証する根拠にしない
- `reserved`は将来拡張のための明示フィールドとして扱う
- `header_length`と`payload_length`をDecoderで検証する

## 19.4 Heartbeat Payload

HeartbeatもLength-Prefixed Frameとし、Wire Formatを固定する。

```text
session_id
last_sequence
last_tick_time_msc
server_utc_offset_sec
heartbeat_elapsed_us
```

`server_utc_offset_sec`は**自動推定値や診断値を意味し得るが、無条件に正しいUTC変換係数とはみなさない**。

# 20. sequence設計

EA起動時に、

```text
session_id
```

を生成する。

EA処理したティックごとに、

```text
sequence = 0, 1, 2, 3...
```

と連番を付与する。

これによりRust側で、

```text
100
101
102
104
```

のような欠落を検出できる。

ただし、このsequenceは**EAが処理したティックの欠落検出用**であり、MT5内部の価格配信段階でティックが失われていないことを保証するものではない。

---

# 21. Batch送信

基本的には、`CopyTicks()`で取得した未処理ティックを**1回の`CopyTicks()`結果単位でBatch化**してTCP送信する。

例：

```text
OnTick
 ↓
CopyTicks
 ↓
15 ticks取得
 ↓
15 ticksを1 packetにまとめる
 ↓
SocketSend
```

目的：

- SocketSend呼び出し回数を削減
- IPC負荷を削減
- EA処理時間を短縮

ただし、Lead/Lag観測ではこのBatch境界が重要となる。Packet内の全Tickに厳密な個別受信時刻を与えることはできないため、Rust側ではPacket受信時刻をフレーム単位の`rx_mono_ns`として記録する。

### 21.1 最大Batch遅延

MVPでは「追加の待ち時間を作らない」ことを原則とする。つまり、`CopyTicks()`で未処理Tickを取得したら、その結果を直ちに送信する。

必要なら、将来の比較実験用に、

```text
max_batch_delay_us = 0
max_ticks_per_packet = configurable
```

を設定可能にする。明示的なタイマー待ちでBatchを貯める設計は採用しない。

---

`SocketSend()`の戻り値がPacket全長に満たない場合は、未送信範囲を追跡して継続送信する。ただし、Socket Timeoutや接続切断が発生した場合は再送可能性を無条件に仮定せず、Packetのsequence範囲を`UNCONFIRMED`として記録する。

# 22. Rust側モジュール構成

```text
src/
├── main.rs
├── config.rs
├── protocol/
│   ├── mod.rs
│   ├── packet.rs
│   └── codec.rs
├── transport/
│   ├── mod.rs
│   └── tcp.rs
├── tick/
│   ├── mod.rs
│   ├── engine.rs
│   ├── candle.rs
│   └── matcher.rs
├── metrics/
│   ├── spread.rs
│   ├── price_diff.rs
│   └── lead_lag.rs
├── storage/
│   ├── mod.rs
│   └── tick_log.rs
├── state/
│   └── snapshot.rs
└── ui/
    ├── mod.rs
    ├── dashboard.rs
    └── chart.rs
```

---

# 23. Rust側スレッド構成

基本構成：

```text
Thread 1
TCP Listener A
    ↓
Tick Channel

Thread 2
TCP Listener B
    ↓
Tick Channel

Thread 3
Tick Engine
    ↓
Candle / Difference / Metrics
    ↓
UI Snapshot

Thread 4
Logger

Main Thread
egui / eframe
```

GUIスレッドでTCP受信を行わない。

---

# 24. Channel

最初の実装では、

```text
crossbeam-channel
```

のbounded channelを使用してよい。

ただし、標準ライブラリの`sync_channel`で十分と判断できる場合は追加依存を減らしてもよい。

重要なのは**unbounded queueを作らないこと**。

### 24.1 データ経路を2層に分離する

本アプリでは、**生Tickの整合性**と**GUIの最新性**を同一キューで競合させない。

```text
Raw Tick Path
TCP → Receiver → Tick Engine → Logger
                     │
                     └─ losslessを基本とする

UI Path
Tick Engine → Snapshot Publisher → UI
                          └─ 中間Snapshotはcoalesce/drop可
```

これにより、GUI描画が一時的に遅れても生Tick処理を理由なく破棄しない。

---

# 25. Backpressure

通常時：

```text
Tick受信
 ↓
Engine処理
```

が十分高速であるため、キューはほぼ空。

異常時に、

```text
GUI停止
Disk停止
Engine負荷上昇
```

等が発生しても、メモリが無制限に増えないようにする。

### 25.1 生Tickは原則dropしない

監視ツールであっても、保存・再現・欠落調査に使う生Tickを静かにdropすると診断能力が失われる。そのため、**Raw Tick Pathはlosslessを基本契約とする**。

ただし、GUIはリアルタイム表示だけが目的なので、未表示の中間Snapshotは破棄してよい。

```text
Raw Tick       : DROP禁止を基本
UI Snapshot    : 最新優先、古い中間状態をcoalesce/drop可
Diagnostic Log : 明示的な欠落・overloadを記録
```

### 25.2 Queue full時の挙動

Raw Tick Pathが満杯になった場合は、

1. `OVERLOAD`状態へ遷移
2. キュー深度・発生時刻を記録
3. Receiver→Engineの流れを過剰に無理やり進めない
4. TCPのbackpressureを利用して送信側へ遅延を伝播させる
5. 回復後にsequence gapがないか検証

とする。

**latest-tick優先dropはUI Snapshot Pathに限定する。Raw Tick Pathでの無通知dropは行わない。**

---

# 26. Tick Engine

Tick Engineは全ティックを入力として受け取る。

主な処理：

```text
Tick
 ↓
Normalize
 ↓
CandleBuilder
 ↓
SpreadCalculator
 ↓
PriceDifferenceCalculator
 ↓
LeadLagDetector
 ↓
RingBuffer
 ↓
UI Snapshot
```

---

# 27. 価格表現

各ブローカーについて保持：

```text
Bid
Ask
Last
Spread
Mid
```

Mid：

```text
mid = (bid + ask) / 2
```

Spread：

```text
spread = ask - bid
```

表示時はpointsとpipsの双方を扱えるようにする。

---

# 28. Broker間価格差

基本：

```text
bid_diff = bid_A - bid_B
ask_diff = ask_A - ask_B
mid_diff = mid_A - mid_B
```

必要なら、

```text
cross_gap_AB = bid_A - ask_B
cross_gap_BA = bid_B - ask_A
```

も計算可能にする。

ただし、デフォルトUIでは情報量を増やしすぎない。

---

# 29. ローソク足生成

ローソク足はMQL5の`CopyRates()`から取得するのではなく、**受信したティックからRust側で自前生成する**。

MQL5の`MqlRates`はOHLC、tick volume、spread等を保持する構造体であり、これを検証用の比較対象として利用できる。

---

# 30. ローソク足の価格モード

デフォルト：

```text
BID
```

つまりBidティックから、

```text
Open
High
Low
Close
```

を生成する。

将来的に、

```text
ASK
MID
```

を追加できるよう内部設計では`PriceMode`を持たせる。

---

# 31. Candle構造体

概念：

```rust
struct Candle {
    start_time_ms: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    tick_count: u32,
}
```

必要に応じて、

```text
spread_min
spread_max
spread_avg
```

を追加する。

---

### 31.1 Fixed Time Slot

CandleBuilder内部では、受信順ではなく`normalized_time_ms`をperiod境界へ丸めた固定Slotを使用する。

```rust
struct CandleSlot {
    start_time_ms: i64,
    state: CandleSlotState,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    tick_count: u32,
}

enum CandleSlotState {
    Empty,
    Active,
    Closed,
}
```

同じUTC Slot indexをA/B双方で使用することで、Tick頻度の違いによってX軸上のSlot位置がずれないようにする。

重要：Tickが1件もないSlotを、分析用の「前値引継ぎフラット足」として生成してはならない。これは実際の市場データが存在しないことを隠すためである。

GUIでは`Empty` Slotを空白として表示する。必要なら前回Close位置に薄い基準線だけを描画する。将来、固定本数表示のために疑似足を描画する場合も、`synthetic_display_only=true`として分析値・保存Tick・Replayデータから分離する。

# 32. 時間足

MVPで必要：

```text
1秒
5秒
10秒
1分
```

ただしUI上の主表示は**1分足**とする。

秒足は後から簡単に変更できるよう、CandleBuilderは汎用化する。

例：

```text
period_ms = 1000
period_ms = 5000
period_ms = 10000
period_ms = 60000
```

---

# 33. 時間軸の扱い

ブローカーごとの生時刻はそのまま保持する。

一方で、2社を比較する表示では共通時間軸を必要とする。

そのため、

```text
raw_broker_time
normalized_time
```

を分離する。

`raw_broker_time`をログに保存し、`normalized_time`は設定された時刻基準に基づいて生成する。

実装開始時に実データを使って、各MT5の`time_msc`のタイムゾーン・基準を確認する。

**時刻変換を推測で固定しない。**

---

# 34. 直近データの保持

GUI用に、

```text
last 10 seconds
last 30 seconds
last 60 seconds
```

程度をRing Bufferに保持する。

表示本数は時間足によって決める。

例：

```text
1秒足 × 60本
5秒足 × 24本
1分足 × 10本
```

---

# 35. GUI設計

## 35.1 基本思想

GUIは「チャートソフト」ではなく、

**スキャルピング中のリアルタイム監視パネル**

として設計する。

そのため、機能を意図的に削減する。

---

# 36. GUIで実装しないもの

MVPでは以下を実装しない。

- マウスズーム
- マウスパン
- 自由なスクロール
- インジケータ
- 移動平均
- RSI
- MACD
- 水平線
- トレンドライン
- 描画ツール
- 発注
- 自動売買
- 注文管理
- ポジション管理
- ニュース
- 経済指標
- ブローカー切替
- 複数画面エディタ

---

# 37. GUIレイアウト案

```text
┌──────────────────────────────────────────────┐
│ TickCompare     USDJPY       ● LIVE         │
├──────────────────────────────────────────────┤
│                                              │
│ OANDA                       Axiory           │
│ Bid 158.234                 Bid 158.233      │
│ Ask 158.236                 Ask 158.235      │
│ Spr 0.2 pip                 Spr 0.2 pip      │
│                                              │
├──────────────────────────────────────────────┤
│               M1 CANDLE                      │
│                                              │
│       │        │        │       │            │
│    ┌──┴──┐  ┌──┴──┐  ┌──┴──┐ ┌─┴─┐          │
│    │     │  │     │  │     │ │   │          │
│    └─────┘  └─────┘  └─────┘ └───┘          │
│                                              │
├──────────────────────────────────────────────┤
│              PRICE DIFFERENCE                │
│                                              │
│ +0.2 pip ───────╮                            │
│  0.0 pip ───────┼────────────                │
│ -0.2 pip        ╰────                         │
│                                              │
├──────────────────────────────────────────────┤
│ Lead: OANDA   3.2 ms                         │
│ Bid Δ: +0.1 pip    Ask Δ: +0.1 pip           │
│                                              │
│ A: ● CONNECTED      B: ● CONNECTED           │
└──────────────────────────────────────────────┘
```

---

# 38. ローソク足表示

eguiの`Painter`を使用する。

1本のローソク足：

```text
wick
  │
  │
┌─┴─┐
│   │ body
└─┬─┘
  │
  │
```

を、

```text
line_segment()
rect_filled()
```

等で描画する。

eguiのPainterは線、矩形、テキスト等を直接描画できるため、この用途には十分である。

---

# 39. チャート操作

チャートは固定方式とする。

禁止：

```text
drag → pan
wheel → zoom
```

時間軸は常に、

```text
現在時刻 = 右端
```

とする。

新しいデータが来れば左方向へ流れる。

---

# 40. 最新足

現在形成中のローソク足を毎フレーム更新する。

例えば1分足の途中なら、

```text
Open = 158.230
High = 158.237
Low  = 158.228
Close = 158.235
```

をリアルタイムで変更する。

---

# 41. GUI更新頻度

TickごとにGUIを再描画しない。

```text
Tick Engine
    ↓
最新State更新

GUI
    ↓
display refresh
```

と分離する。

目標：

```text
60～120 Hz
```

実際のモニター更新速度に従う。

Tickレートが1000/sでもGUIは1000fpsにはしない。

---

# 42. Lead/Lag検出

Lead/Lagは絶対価格の一致ではなく、**相対的な価格変化イベント**の対応関係として実装する。

基本価格はMidを使用する。

```text
mid = (bid + ask) / 2
Δmid = mid_now - event_anchor_mid
```

イベント時刻はRust側の共通単調時計：

```text
event_time = rx_mono_ns
```

を使用する。`broker_time_msc`はLead/Lagの差分時計には使用しない。

### 42.1 Significant Move Event

微小なBid/Askバウンスによるゴースト判定を抑えるため、単一Tickの1point変化だけを即イベント化しない。イベントは設定可能な閾値を持つ。

初期値候補：

```text
trigger_move_points = 2
matching_window_ms = 100
event_cooldown_ms = 20
```

値はUSD/JPYの桁、ブローカー、観測目的に応じて設定変更可能とする。`3 points`を固定値として仕様化しない。

イベント状態として、少なくとも以下を保持する。

```text
event_anchor_mid
last_event_time_ns
cooldown_until_ns
```

イベント検出：

```text
|mid_now - event_anchor_mid| >= trigger
    ↓
SIGNIFICANT_MOVE
    ↓
イベント発行
    ↓
event_anchor_mid = mid_now
    ↓
cooldown開始
```

**重要:** イベント発火時には`event_anchor_mid`を直ちに現在Midへ再アンカーする。Cooldown中にanchorそのものを価格へ追随させる実装は禁止する。

これにより、急騰・急落の一方向トレンドでも、価格がreset条件へ戻るまでイベント検出が停止する問題を防ぐ。

Cooldown中は新規イベントを抑制するが、現在価格・最新Tick・最大乖離などの観測値は通常どおり更新する。Cooldown終了後は、更新せず固定された`event_anchor_mid`から再び閾値判定する。

巨大な1Tickジャンプの場合、そのイベントには、

```text
mid_delta_points
```

の実値を保持し、イベント1件に対する価格変化量を後から評価できるようにする。

### 42.2 Broker Offsetの扱い

ブローカー間に恒常的な価格水準差が存在しても、Δmidベースのイベント判定には影響しない。したがって、Lead/Lag判定のためにA/Bの絶対価格を一致させる補正は行わない。

Price Difference表示については、必要に応じて、

```text
raw_mid_diff
centered_mid_diff
```

を分離して保持する。centered値は表示・分析補助であり、Raw Tickを改変しない。

### 42.3 Bid/Askバウンスとイベント品質

各イベントには、少なくとも以下を保持する。

```text
bid_delta
ask_delta
mid_delta
spread_delta
```

これにより、単純なMidイベントだけでなく、`BID_ONLY`、`ASK_ONLY`、`BOTH_SIDES`、`SPREAD_DRIVEN`等の品質分類を後から行える。

MVPではLead/Lagの基本判定をMidイベントで行い、品質分類を診断情報として保持する。

# 43. Lead/Lagのマッチング

Aで有意変動イベント`t_A`が発生した場合、

```text
0 < t_B - t_A <= matching_window_ms
```

を満たすB側イベントがあり、かつ方向が一致する場合に対応イベントとする。逆方向も同様に判定する。

```text
Lead = t_B - t_A
```

であり、符号とLeader Brokerを別フィールドで保持する。

単純な「次のTickが来た方」をLeaderとはしない。価格イベントの閾値、方向、時間窓、Cooldownをすべて条件とする。

同一イベントへ複数候補が存在する場合は、**時間差が最小の候補を1件だけ対応付け**、他は未対応イベントとして残す。これにより1回の価格変化が複数Brokerイベントへ多重マッチするのを防止する。

# 44. Lead/Lag表示上の注意

UIでは、

```text
Observed Lead: OANDA +3.2 ms
EMA: +2.1 ms
```

のように表示する。

しかし、

**「OANDAの市場価格が3.2ms早い」**

とは表現しない。

正式には、

**「本PCの観測経路上で、OANDA側の対応価格変化を先に受信した」**

という意味とする。

### 44.1 生値と平滑値

Lead/Lagは瞬間的に反転するため、表示用として、

```text
raw_delta_ms
ema_delta_ms
```

を分離する。

EMAは視認性向上だけに使用し、イベント判定・ログ保存の原値を置き換えない。初期値候補：`alpha = 0.1`。

# 45. Spread表示

各ブローカーについて、

```text
spread = ask - bid
```

を常時表示。

さらに、

```text
current
minimum
maximum
```

を保持できる。

表示例：

```text
OANDA
Spread 0.2 pip

Axiory
Spread 0.3 pip
```

異常な急拡大が発生した場合、数値を強調表示する。

---

# 46. スプレッド急拡大

MVPでは閾値だけ実装する。

例：

```text
normal spread <= threshold
warning spread > threshold
```

ただし、閾値はブローカーや銘柄によって異なるため設定可能にする。

---

# 47. 接続ステータス

各データ源について、

```text
CONNECTED
CONNECTING
DISCONNECTED
DATA_STALE
EA_HEARTBEAT_OK
OVERLOAD
```

を表示する。

`CONNECTED`はTCP接続状態、`DATA_STALE`は新Tick未受信状態、`EA_HEARTBEAT_OK`はEAがTimer経由で生存通知を送れている状態を表す。市場が止まったこととは同義ではない。

---

# 48. STALE / Heartbeat判定

一定時間新しいTickがない場合は、

```text
DATA_STALE
```

と表示する。

初期値候補：

```text
stale_after_ms = 1000
heartbeat_interval_ms = 250
heartbeat_timeout_ms = 1500
```

TickとHeartbeatは別の意味を持つため、状態を分離する。

| 状態 | 意味 |
|---|---|
| CONNECTED + DATA_LIVE | TCP接続中かつTick受信中 |
| CONNECTED + DATA_STALE + HEARTBEAT_OK | TCP/EAは生存しているが新Tickなし |
| CONNECTED + HEARTBEAT_TIMEOUT | EA・イベント処理・通信に問題の可能性 |
| DISCONNECTED | TCP切断 |
| OVERLOAD | Receiver / Engine / Logger等の処理が逼迫 |

これは「ブローカーの市場が止まった」という意味ではなく、**本アプリがどの経路まで生存確認できているか**を示す。
---

# 49.1 EA Heartbeat

`EventSetMillisecondTimer()`を使用し、Tickが存在しない時間帯でもEAからHeartbeatを送信する。

初期値：

```text
heartbeat_interval_ms = 250
```

Heartbeatには少なくとも、

```text
session_id
last_sequence
last_tick_time_msc
server_utc_offset_ms
ea_elapsed_us
```

を含める。

MQL5のTimerイベントもEA内のイベントキューで逐次処理されるため、Heartbeatは完全な独立スレッドではない。したがって「TimerがあるからEAの応答性が必ず保証される」とは扱わず、**OnTick処理時間を短く保つこと**を前提とする。

Heartbeatの役割は以下。

- Tickが止まっているだけなのかを判断する補助
- EAがまだイベント処理可能かを判断する補助
- session/sequence/時刻基準情報の定期通知
- Rust側のSTALE判定の誤警報低減

# 49. MT5切断時

Rust側から見てTCP切断を検出した場合：

```text
Axiory: DISCONNECTED
```

と表示。

一定間隔で再接続を待つ。

MT5 EA側もSocket接続が失われた場合に再接続する。

MQL5にはSocket接続状態の確認やSocket timeout設定が用意されている。

---

# 50. Rust側のログ

生ティックをバイナリ形式で保存する。

保存対象：

```text
session_id
sequence
broker_time_msc
ea_elapsed_us
bid
ask
last
volume
volume_real
flags
rust_receive_time
```

---

# 51. ログ形式

MVPでは独自Binary Tick Log。

例：

```text
logs/
  2026-09-21/
    oanda/
      20260921_012301.tlog
    axiory/
      20260921_012301.tlog
```

ファイルは追記方式。

---

# 52. ログとGUIの分離

LoggerがディスクI/Oで遅くなってもGUIを止めない。

```text
Tick
 ├─→ Engine
 └─→ Logger
```

とする。

Logger側ではBuffered I/Oを使用する。

---

# 53. GUI Snapshot

GUIはTick構造体そのものを大量に参照しない。

Engineが、

```rust
UiSnapshot
```

を生成する。

### 53.1 SnapshotはTickごとに公開しない

Tick処理とGUI更新は別レートとする。Engine内部では最新状態を更新し、`repaint_hz`に合わせてSnapshotを公開する。

これにより、1000 tick/sで1000個のSnapshotを生成するような構成を避ける。

例：

```text
UiSnapshot
 ├─ broker_a
 │   ├─ bid
 │   ├─ ask
 │   ├─ spread
 │   ├─ latest_candle
 │   └─ candle_history
 │
 ├─ broker_b
 │   ├─ bid
 │   ├─ ask
 │   ├─ spread
 │   ├─ latest_candle
 │   └─ candle_history
 │
 ├─ diff
 ├─ lead_lag
 └─ connection_state
```

GUIはこのsnapshotだけ参照する。

---

# 54. データ競合

GUIスレッドとEngineスレッドの共有データには同期機構を使用する。

本設計では、UI Snapshotはimmutableな`Arc`として公開し、

```text
arc-swap
```

の`ArcSwap<UiSnapshot>`を第一候補とする。

理由は、UI側は読み取り中心であり、Engine側が新しいSnapshotをatomically publishできるためである。`ArcSwap`は`Arc`を複数スレッドからload/storeする用途を提供している。

ただし、`ArcSwap`採用だけでは性能問題が解決するわけではない。特に重要なのは、

- Snapshot生成をTickごとに行わない
- GUI描画中に大きなLockを保持しない
- Candle履歴などの大量データを毎回deep-copyしない

ことである。

実装簡素化を優先する場合は、`Arc<Mutex<UiSnapshot>>`でも、**GUI側で短時間にcloneして即unlockしてから描画する**条件なら成立する。したがって`ArcSwap`は「必須の正解」ではなく、本アプリの低遅延要件に対する第一候補と位置付ける。

---

# 55. 設定ファイル

Rustアプリ側：

```text
config.toml
```

例：

```toml
symbol = "USDJPY"

[broker_a]
name = "OANDA"
port = 39001

[broker_b]
name = "Axiory"
port = 39002

[display]
timeframe_ms = 60000
visible_seconds = 60
repaint_hz = 60
always_on_top = false

[lead_lag]
minimum_move_points = 1
matching_window_ms = 100

[logging]
enabled = true
directory = "logs"
```

---

# 56. MT5 EA設定

各EAでは、

```text
Broker ID
Symbol
Rust Host
Rust Port
Warmup Seconds
Send Mode
Reconnect Interval
```

をinput parameterとして持たせる。

例えば、

```text
BrokerId = 1
Symbol = USDJPY
Host = 127.0.0.1
Port = 39001
```

---

# 57. Symbolについて

ブローカーによって、

```text
USDJPY
USDJPY.a
USDJPY#
USDJPY.pro
```

等、Symbol名が異なる可能性を考慮する。

EAではBrokerごとの実Symbolを設定する。

Rust側では、

```text
canonical_symbol = USDJPY
```

として扱う。

---

# 58. 重要な非機能要件

## 58.1 レスポンス

通常時：

```text
Tick → Engine処理
```

をできるだけ短くする。

GUI描画遅延とTick処理遅延を混同しない。

---

# 59. GUI負荷

以下を目標とする。

```text
CPU:
通常時 低負荷

GPU:
軽量

メモリ:
安定

GUI:
フレーム落ちを極力発生させない
```

ブラウザ/WebViewは使用しない。

---

# 60. Tauriを採用しない理由

今回、

```text
Rust
+
WebView
+
HTML
+
CSS
+
JavaScript
```

という構成は不要。

UIは、

```text
Rust
 ↓
egui
 ↓
eframe
```

だけで完結させる。

これは今回の、

- 小型固定UI
- チャート機能限定
- 操作少なめ
- リアルタイム表示
- Windows専用

という条件と一致する。

---

# 61. 汎用チャートライブラリを採用しない理由

TradingView系・Web Chart系ライブラリは、

- zoom
- pan
- axis management
- indicators
- interaction
- drawing tools

等に強い。

しかし本アプリではほとんど不要。

したがって、eguiの`Painter`による直接描画を採用する。

---

# 62. liveplot等の汎用plot crate

参考にはする。

しかしMVPでは必須としない。

理由：

- ローソク足を自前描画できる
- 差分波形も自前描画できる
- 表示要件が固定
- 外部依存を減らせる
- UI仕様を完全にコントロールできる

---

# 63. 1分足の再現性検証

実装後、各ブローカーについて、

```text
自前 Tick → Candle
```

と、

```text
MT5 CopyRates → M1
```

を比較する。

対象：

```text
Open
High
Low
Close
Tick Volume
```

ただし、**「完全一致しない = 実装バグ」とは固定しない**。

検証は次の順序で行う。

1. 同一Symbol・同一時間足・同一価格モードか確認
2. ブローカー時刻→共通UTC変換が正しいか確認
3. どのTickが対象バー境界に入っているか確認
4. sequence gap / CopyTicksエラー / 同一ms境界を確認
5. それでも乖離する場合に、データソース差を調査

特に現行足は更新途中なので完全一致の判定対象から分離する。**確定済みバーと現行バーを別評価する。**

MQL5の`CopyRates()`は現行バーを含むデータを取得できるため、検証用データソースとして利用する。

合格条件は「完全一致」の一語ではなく、

```text
closed_bar_ohlc_exact
closed_bar_tick_volume_match
current_bar_expected_dynamic
```

のように項目別に定義する。

---

# 64. Tick取りこぼし検証

意図的に高頻度ティック環境を作り、

```text
EA sequence
Rust sequence
```

に欠番がないことを確認する。

さらに、**sequence gapが存在しないことだけでは「MT5内部で絶対に1ティックも失われていない」ことは証明できない**ことを明記する。sequenceはEAが処理・送信した範囲の連続性を検証する指標である。

例：

```text
1
2
3
4
5
6
```

はOK。

```text
1
2
3
5
6
```

は通信・処理上の欠落として検出する。

---

# 65. 同一msティック試験

テストデータとして、

```text
time_msc = 1000
time_msc = 1000
time_msc = 1000
time_msc = 1001
```

を作り、

```text
4 ticks
```

すべて取得できることを確認する。

---

# 66. 高負荷試験

疑似的に、

```text
100
500
1000
2000
```

tick/sを入力し、

- Tick Engineが詰まらない
- GUIが異常停止しない
- Loggerが追いつく
- queueが無限増加しない
- UI Snapshotだけはcoalesce可能である
- Raw Tick Pathで無通知dropが発生しない

ことを確認する。

追加で、MQL5→Rust TCPの実測として、Packet受信遅延とジッターのp50/p95/p99を測定する。TCP_NODELAYの影響はここで評価し、結果だけを設計判断へ反映する。

---

# 67. 再接続試験

実行中に、

```text
MT5終了
MT5再起動
EA再接続
```

を行う。

期待：

```text
DISCONNECTED
    ↓
CONNECTING
    ↓
CONNECTED
```

---

# 68. Rustアプリ再起動試験

Rustアプリを終了し、再起動する。

MT5側EAは接続エラー後に自動的に再接続する。

期待：

```text
Rust終了
↓
MT5 EA reconnect retry
↓
Rust起動
↓
自動接続
```

---

# 69. GUI応答性試験

高負荷tick入力中でも、

- ウィンドウ移動
- 最小化
- 最大化
- 終了

が正常にできること。

---

# 70. メモリリーク試験

数時間～数日連続稼働させる。

確認：

```text
RAM使用量
Thread数
Handle数
ログサイズ
```

が異常増加しないこと。

---

# 71. アプリ起動時の状態

起動直後：

```text
CONNECTING
```

データ受信：

```text
LIVE
```

初期履歴生成：

```text
WARMING
```

一定時間データなし：

```text
STALE
```

接続失敗：

```text
DISCONNECTED
```

を明示。

---

# 72. エラー処理

エラーは黙って無視しない。

最低限、

```text
ログ
GUI status
```

に残す。

例：

```text
TCP connection failed
CopyTicks failed
Invalid packet
Sequence gap
Decoder error
Logger error
```

---

# 73. ログレベル

```text
ERROR
WARN
INFO
DEBUG
TRACE
```

を用意する。

通常運用は、

```text
INFO
```

とする。

`TRACE`は開発時のみ。

---

# 74. 開発時の重要な診断情報

GUIに必要に応じて、

```text
Tick rate
Packets/sec
Queue depth
Last tick age
Last heartbeat age
Sequence gap count
Logger queue depth
Raw receive p50/p95/p99
Engine processing p50/p95/p99
Snapshot publish age
```

を表示できるDebug Overlayを用意する。

通常画面では非表示。

---

# 75. 秒スキャルピング向けの重要指標

MVPで必須：

```text
Bid
Ask
Spread
Mid
Broker間Bid差
Broker間Ask差
Broker間Mid差
Lead/Lag
Tick rate
Connection state
```

---

# 76. 将来追加可能だがMVPでは入れないもの

将来的には、

```text
1. スプレッド波形
2. 数秒間の価格差分布
3. ブローカー別tick interval
4. 片側だけ価格が動いた回数
5. 先行率
6. 急変イベント検出
7. ニュース時間帯マーカー
8. CSV/Parquet変換
9. リプレイ
10. 3社以上の比較
```

を追加可能。

ただしMVPには入れない。

---

# 77. 自動売買との分離

本アプリから、

```text
OrderSend
PositionOpen
PositionClose
```

等の取引処理を行わない。

MT5 EAにも発注ロジックを実装しない。

本アプリは完全な**read-only market data monitor**とする。

---

# 78. セキュリティ

Rust側TCP listenerは、

```text
127.0.0.1
```

のみでlistenする。

外部LANから接続できない構成とする。

例：

```text
127.0.0.1:39001
127.0.0.1:39002
```

---

# 79. TCP切断時の扱い

TCP connectionが切れたら、

```text
receiver thread
    ↓
disconnect event
    ↓
status = DISCONNECTED
    ↓
reconnect
```

とする。

GUI threadは停止しない。

最後の価格は表示してもよいが、

```text
STALE
```

で明確に区別する。

---

# 80. データ整合性

1. sequence連続性
2. packet length
3. magic
4. protocol version
5. broker_id
6. session_id
7. tick_count
8. payload size

を検証する。

不正packetは無視せずエラー記録する。

---

# 81. 実装優先順位

## Phase 1

MQL5 EA：

```text
OnTick
CopyTicks
sequence
SocketSend
```

だけを実装。

Rust側：

```text
TCP Listener
packet decode
console log
```

まで。

GUIはまだ不要。

---

# 82. Phase 2

Rust：

```text
Tick Engine
Price state
Spread
Difference
```

を実装。

---

# 83. Phase 3

CandleBuilder：

```text
1sec
5sec
10sec
1min
```

を実装。

---

# 84. Phase 4

egui/eframeで、

```text
OANDA
Axiory
Candle
Difference
Spread
```

を表示。

eframeはネイティブアプリとして`run_native()`で起動でき、GUIは`egui::Painter`で直接描画できる。

---

# 85. Phase 5

Logger：

```text
binary tick log
```

を追加。

---

# 86. Phase 6

Lead/Lag検出。

---

# 87. Phase 7

異常系・負荷・長時間稼働テスト。

---

# 88. MVP完成条件

以下を満たせばMVP完成とする。

### データ

- 2社MT5からリアルタイムTickを取得できる
- Bid/Askを表示できる
- Tick sequenceを検証できる
- 接続切断から再接続できる

### チャート

- Tickから1分足を生成できる
- 現在形成中の足がリアルタイム更新される
- OANDAとAxioryを同じ時間軸で表示できる

### 比較

- Bid差
- Ask差
- Mid差
- Spread差
- Lead/Lag

をリアルタイム表示できる。

### GUI

- 軽量
- 固定UI
- ズームなし
- パンなし
- インジケータなし

### 保存

- 生TickをBinary Logとして保存できる。

---

# 89. 性能目標

厳密な固定値を初期設計として保証するのではなく、実測値を記録する。

本アプリでは「低遅延」を構成要素ごとに測定し、単一の総合値で評価しない。

最低限、

```text
MQL5 CopyTicks processing time
MQL5 SocketSend processing time
Rust decode time
Tick Engine processing time
Logger processing time
GUI frame time
```

を開発用計測項目として取得する。

特に、

```text
Tick受信 → UI表示
```

だけでなく、

```text
Tick受信 → Rust Engine
```

の遅延を測る。

---

# 90. 「低遅延」の定義

本アプリで重要なのは、単にGUIが高速ということではない。

優先順位：

```text
1. 生Tickの整合性を維持
2. Tick処理を詰まらせない
3. 比較計算を即時実行
4. GUIの中間状態をcoalesceして描画負荷を制御
5. Loggerを非同期化
6. IPC遅延・ジッターを実測して把握
```

「最新表示を優先する」ことと「生Tickを捨てる」ことは別問題として扱う。

---

# 91. 遅延の解釈

観測する時間は最低2種類持つ。

```text
broker_time_msc
```

と、

```text
Rust local receive timestamp
```

である。

可能ならEA側の、

```text
ea_elapsed_us
```

も保持する。

これにより、

```text
ブローカー時刻
EA処理
ローカルTCP
Rust処理
GUI
```

を後から分離して解析できる。

---

# 92. 本アプリで絶対にしないこと

以下を設計として禁止する。

```text
OnTick 1回 = 1 tick
```

と仮定する。

また、

```text
GUI threadでCopyTicks
GUI threadでTCP受信
GUI threadでLogger
```

を行わない。

さらに、

```text
tick → JSON
tick → CSV
tick → DOM
```

のような重量処理を行わない。

---

# 93. 技術選定最終版

| 項目 | 採用 |
|---|---|
| OS | Windows 11 x64 |
| Broker接続 | 2× MT5 |
| MT5データ取得 | MQL5 EA |
| Tick回収 | `OnTick` + `CopyTicks(COPY_TICKS_ALL)` + explicit count/bounded catch-up |
| Tick欠落対策 | time_msc + same-ms cursor + 境界再読込診断 |
| IPC | **localhost TCP**（実測で代替評価） |
| MQL5 Socket | 標準Socket API |
| DLL | 使用しない |
| Named Pipe | 第二候補 |
| Rust | Stable |
| GUI | **egui / eframe** |
| Chart | `egui::Painter`自前描画 |
| UI Snapshot共有 | **ArcSwap（第一候補）** |
| Candle | Rust自前生成 |
| Channel | bounded channel |
| Tick保存 | Binary |
| Ring Buffer | 使用 |
| Heartbeat | **OnTimer + binary packet** |
| Lead/Lag clock | **Rust `Instant` / `rx_mono_ns`** |
| UI Snapshot | 最新優先coalesce可 |
| JSON | 使用しない |
| CSV | リアルタイム処理には使用しない |
| Tauri | 使用しない |
| WebView | 使用しない |
| Chart Library | 原則使用しない |
| Auto Trade | 実装しない |

---

# 94. 最終アーキテクチャ

```text
                    BROKER A
                       │
                       ▼
                 MT5 Instance A
                       │
                    MQL5 EA
                       │
              OnTick + CopyTicks
                       │
                 Binary TCP
                       │
                       ▼
             ┌───────────────────┐
             │ Rust TCP Receiver │
             └─────────┬─────────┘
                       │
                       │
                    Tick Engine
                       │
       ┌───────────────┼────────────────┐
       │               │                │
       ▼               ▼                ▼
 Candle Builder   Difference       Lead/Lag
       │               │                │
       └───────────────┼────────────────┘
                       │
                       ▼
                  UI Snapshot
                       │
                       ▼
                  egui / eframe
                       │
                       ▼
               リアルタイム表示


                    BROKER B
                       │
                       ▼
                 MT5 Instance B
                       │
                    MQL5 EA
                       │
              OnTick + CopyTicks
                       │
                 Binary TCP
                       │
                       └──────→ 同じRust Engine
```

---

# 95. 開発者への実装上の最重要事項

実装者は以下を特に厳守する。

### 1.

`OnTick()`をティック本体として扱わない。

**必ず`CopyTicks()`で未処理Tickを回収する。**

### 2.

同一`time_msc`内に複数Tickが存在することを前提にする。

### 3.

UIとTick処理を完全に分離する。

### 4.

リアルタイムTickをJSON/CSVに変換しない。

### 5.

価格差・Spread・Lead/Lag計算はRust側で行う。

### 6.

ローソク足は受信TickからRust側で再構成する。

### 7.

Lead/Lagを「市場そのものの先行」と表現しない。

### 8.

通信はまずlocalhost TCPで実装する。MQL5側でTCP_NODELAYを直接設定できることを前提にしない。TCP遅延・ジッターは実測する。

### 9.

Named Pipe / kernel32.dllは最初から採用しない。TCP実測で有意な問題が確認された場合に比較対象とする。

### 10.

`OnTimer()`でHeartbeatを送信し、`DATA_STALE`と`EA_HEARTBEAT_OK`を分離する。

### 11.

Lead/Lagはブローカー時刻ではなくRust側の共通単調時計で判定する。

### 12.

Raw Tick Pathは原則lossless、UI Snapshot Pathのみ最新優先coalesce/dropを許可する。

### 13.

同一msのTickはHash Setで単純排除しない。`time_msc`は一意IDではない。

### 14.

アプリが停止・切断・過負荷になった場合でも、状態をGUI上で明示する。

---

# 95.1 Revision 1.2 追加テスト

### CopyTicks Burst Test

```text
100 / 300 / 1000 / 5000 tick burst
```

について、Batch上限が小さい場合でもsequence欠落が発生せず、`TICK_BACKLOG`から復帰できることを確認する。

### Socket Stall Test

Rust Receiverを停止・pauseし、MQL5 EAが指定Timeout以上に長時間拘束されず、`TRANSPORT_FAULT`または`DISCONNECTED`へ遷移することを確認する。

### ACK Mode Test

```text
ack_mode = off
ack_mode = diagnostic
ack_mode = forced
```

を比較し、localhost TCPでのp50/p95/p99送信→受信遅延を計測する。改善が確認できない場合は`forced`を採用しない。

### Empty Slot Test

一方のBrokerだけ数秒間Tickを停止させ、共通UTC SlotのX軸位置が一致したまま、停止側が`Empty`として表示されることを確認する。

### Lead/Lag Noise Test

以下を含む合成データで誤検知率を確認する。

```text
恒常的価格オフセット
1point上下バウンス
Spread急拡大
片側Quote更新
有意な同方向移動
```

有意イベントだけが対応付けられ、1イベントが複数イベントに多重マッチしないことを確認する。

# 95.2 Revision 1.3 追加テスト

### Wire Format Test

固定バイト列をFixtureとして用意し、Rust Decoderで以下を検証する。

```text
Header = 40 bytes
TickRecord = 72 bytes
```

各FieldについてOffset・Byte Length・Little Endianが期待値と一致することを確認する。

`reserved`は常に0として送信され、未知値を受信した場合はWARNを記録する。

### TCP Fragmentation / Coalescing Test

以下のread境界を意図的に再現する。

```text
1 byteずつread
Header途中で分割
TickRecord途中で分割
複数Frameを1回でread
Frame境界とread境界を完全に一致させない
```

Decoder結果がすべて同一になることを確認する。

### Malformed Frame Test

以下を入力し、無制限にメモリ確保しないことを確認する。

```text
invalid magic
invalid version
header_length < minimum
payload_length > max_frame_payload_size
truncated frame
```

Production Modeでは異常接続を切断・再接続し、Debug/Fuzz Modeだけ有限Resync Scanを許可する。

### Trend Anchor Test

以下を含む合成Tick列でイベント停止・乱発がないことを確認する。

```text
158.200 → 158.201 → 158.202 → 158.203 → ...
158.200 → 158.250
158.250 → 158.251 → 158.252
```

イベント発火後に`event_anchor_mid`が現在価格へ再アンカーされ、Cooldown終了後にさらなる有意変動を検出できることを確認する。

### UTC Calibration Test

`TimeCurrent()`、`TimeGMT()`、`broker_time_msc`、Rust wall-clockの関係を複数サンプルで収集する。

以下を検証する。

```text
候補offsetの安定性
DST切替日前後
Tick停止中
OnTickとOnTimerの差
PC時刻変更時
```

単一の`TimeCurrent() - TimeGMT()`値だけで`normalized_time_utc_ms`を確定しないことを確認する。

# 95.3 Revision 1.3 実装上の最重要追加事項

### 15.

TCPはストリームとして扱い、`SocketSend()`と`read()`の1対1対応を仮定しない。

### 16.

Wire FormatはABIに依存させず、**固定Offset・固定Size・Little Endianのプロトコル仕様**として実装する。

### 17.

Malformed FrameをProduction Modeで無制限Magic Scanして復旧しようとしない。基本はエラー記録・接続破棄・再接続とする。

### 18.

`TimeCurrent() - TimeGMT()`を恒久的なUTC変換式として固定しない。UTC正規化は設定・校正・検証可能な時刻基準として管理する。

### 19.

Lead/Lagイベント発火時は`event_anchor_mid`を現在価格へ再アンカーし、Cooldown中にanchorを追随させない。

# 96. 設計上の最終結論

このアプリは、一般的なFXチャートソフトではなく、

> **「複数FXブローカーのリアルタイム価格配信を、ティックレベルで比較するための低遅延マーケットデータ監視ツール」**

として設計する。

中心となる技術は、

```text
MT5
+
MQL5 CopyTicks
+
localhost TCP
+
Rust
+
egui / eframe
```

である。

そして、

```text
Tick収集
≠
Tick比較
≠
ローソク足生成
≠
GUI描画
```

を完全に分離する。

この分離によって、今後、

```text
1秒足
5秒足
1分足
Spread波形
Price Difference
Lead/Lag
Tick Interval
Replay
```

などを追加しても、基本アーキテクチャを変更せずに拡張できる構造にする。

なお、現行のeframe/eguiはネイティブWindowsアプリとして利用でき、Painterによる直接描画も提供されているため、本仕様の「固定レイアウト・ズームなし・パンなし・インジケータなし」のチャートには十分な機能を持つ。

---

# Appendix A. Revision 1.3の改訂理由とレビュー反映一覧

本節は、元設計書に対して今回のレビューで何を採用・修正したかを明示する。

## A.1 採用した指摘

### A.1.1 Heartbeat

採用する。`OnTick()`だけでは「Tickが来ない」と「EA/TCPが死んでいる」を区別できないため、`OnTimer()`によるHeartbeatを追加した。

ただし、MQL5のTimerも同一EAのイベントキューで逐次処理されるため、Heartbeatは独立監視スレッドではない。

### A.1.2 ブローカー時刻だけでLead/Lagを判定しない

採用する。Lead/LagはRust側の同一PC上の`Instant`による単調時計で判定する。ブローカー時刻はローソク足の時間軸・ログ・診断用として保持する。

### A.1.3 UI Snapshotの非ロック共有

部分採用する。`ArcSwap`を第一候補とする。ただし、`Arc<Mutex>`が必ず破綻するとはしない。Snapshotの生成頻度・clone量・lock保持時間を先に制御し、必要以上に複雑化しない。

### A.1.4 UIだけ最新優先にする

採用する。UIの中間Snapshotはdrop/coalesce可能だが、生Tickの取りこぼしは別問題として扱う。

### A.1.5 Lead/LagのEMA表示

採用する。Raw値とEMA値を分離し、EMAは視認性向上だけに使用する。

## A.2 そのまま採用しなかった指摘

### A.2.1 「MQL5 SocketのNagleが必ず数十～200ms遅延を生む」

そのまま採用しない。MQL5標準Socket APIにはTCP_NODELAY設定APIが明示されていないことは設計上重要だが、実装を見ずに「Nagleが常時有効」「必ず数十ms遅れる」とは断定しない。

Rust側の`set_nodelay(true)`は送信側がRustである場合の設定であり、MQL5→Rust方向の送信Nagleを直接無効化するものではない。したがって、TCP遅延は実測で評価する。

### A.2.2 「CopyRatesとの完全一致は原理的に100%不可能」

そのまま採用しない。`CopyTicks()`は端末側の同期済みTickデータを取得できるため、完全一致を最初から不可能と決めつける根拠は不足している。一方で、現行バー・同期状態・境界処理・データソース差等による乖離は起こり得るため、完全一致を唯一の合格条件にはしない。

### A.2.3 Fingerprint Hashによる重複排除

そのまま採用しない。同一内容の正当なTickをHash Setで1件に潰す危険があるためである。`time_msc + same-ms cursor`を基本にし、必要なら同一ms境界の再読込を診断する。

### A.2.4 「CopyTicksのcount未指定は無条件にメモリ爆発する」

指摘の方向性は採用するが、表現は修正する。MQL5公式仕様では、`CopyTicks()`の`count`既定値は0であり、`from`と`count`の両方を指定しない場合は直近Tickが最大2000件取得される。一方、`from`を指定した増分取得では要求範囲・countに応じた取得となるため、「必ず無制限に全履歴を取得する」とは断定しない。

ただしライブ経路では挙動を暗黙値に依存しないため、`count`を必ず明示し、Batch上限と追随ループを設ける。

### A.2.5 「SocketTimeoutsは5ms固定」「ACKを毎Packet必須」

そのまま採用しない。`SocketTimeouts()`の導入自体は採用するが、5msを絶対値として固定すると正常な一時的スケジューリング遅延まで障害扱いになる可能性がある。送信Timeoutは設定値として持ち、実測で調整する。

また、Rustからの極小Replyは有効な実験手段だが、毎Packet ACKをMVPの必須条件にはしない。TCP_NODELAY・Batchサイズ・ACK modeの組み合わせを実測し、p95/p99の改善が確認できた場合のみ強制する。

## A.3 設計上の新しい不変条件

本改訂版では、以下を実装上の不変条件とする。

```text
1. OnTick event count != tick count
2. broker_time_msc != unique tick ID
3. Lead/Lag clock = Rust monotonic receive clock
4. Candle clock = normalized broker time / UTC axis
5. Raw Tick Path = lossless by default
6. UI Snapshot Path = latest-state priority
7. TCP_NODELAY behavior = measured, not assumed
8. DATA_STALE != EA_DISCONNECTED
9. Heartbeat != market tick
10. CopyRates exact match = validation target, not an unconditional axiom
11. CopyTicks live path uses explicit count and bounded catch-up loops
12. SocketSend never waits indefinitely; timeout/failure becomes an explicit transport state
13. TCP ACK mitigation is an experimentally selectable transport mode
14. Candle Slot is aligned by normalized UTC time, not arrival time
15. No-tick Slot is `Empty`; synthetic flat bars are display-only and never analytical data
16. Lead/Lag uses significant Mid-move events, not absolute price equality
17. Lead/Lag matching is one-event-to-one-event; duplicate multi-match is prohibited
```
