# 公開インターフェース契約 C1

ここにある型・関数は設計上の署名であり、コード実装ではない。P0が`src/contracts/`に唯一の定義を置く。兄弟moduleの具象型を公開引数にせず、共通のdata typeとPortだけに依存させる。出典: 主仕様§19,§22–28,§31,§42–54。未記載の詳細はB/D22。

## 共通型、単位、所有権

| 型 | 必須内容・契約 |
|---|---|
| `BrokerId` | u32。A=1/B=2を初期設定、portと固定対応 |
| `SessionId`, `Sequence` | u64。EA起動単位／session内0開始。時間・内容をIDにしない |
| `TickId` | `(broker_id, session_id, sequence)` |
| `RunId` | Rust起動ごとの128bit識別子。mono epochを跨ぐ比較禁止 |
| `MonoNs`, `UtcMs`, `BrokerMs`, `EaUs` | 別のnewtype。単位を混ぜて暗黙演算しない。MonoNsはrun起点u64、UTC/Brokerはi64 |
| `ClockReading` | run_id、mono_ns、ログ用optional unix_ns。wall clockはmonoとの差分演算禁止 |
| `TickRecord` | wireにある全フィールドを保持。f64原bitを保持でき、reservedも観測値を残す |
| `Frame` | Headerと`TickBatch / Heartbeat / BatchAck / Status`のvariant。wire byte上のmetadataだけ |
| `ReceivedFrame` | Frame、原wire bytesのimmutable Arc、run_id、rx_mono_ns、optional rx_unix_ns、connection_generation、frame_index |
| `IngressItem` | `Connected / Frame(ReceivedFrame) / Progress(MonoNs) / End(reason)`。broker別FIFO |
| `AnalysisSegmentId` | Rust run＋broker/session/phase/config/continuity変更の区切りを識別。Lead状態を跨いで再利用しない |
| `ObservedTick` | TickId、原Tick、受信metadata、Warmup/Live、segment、integrity分類。Frame時刻を継承 |
| `NormalizedTick` | ObservedTick、`utc_ms`、normalization_epoch。有効なUTC設定がある場合だけ生成 |
| `SymbolMeta` | canonical_symbol、broker実symbol、digits、point_size、pip_size。正値・有限、EA実値と照合 |
| `Quote` | tick_id、bid/ask/mid/spread、受信時刻、optional UTC、phase、age/validity。Mid/SpreadはRust算出 |
| `Diagnostic` | code、severity、broker/session/run、mono、optional sequence範囲、known_count/unknown、詳細数値。Raw欠落とUI coalesceを分ける |
| `HealthState` | connection、data_freshness、heartbeat、phase、overload flags、integrity、logger_state、age、累積counter |
| `Capacity` | 件数とbyte双方の上限。checked arithmetic、予算を越える増殖禁止 |

`Option`は未取得・計算不可を意味する。0価格、0時刻、sequence=0を欠損値として代用しない。原Tickの内容は不正quoteであっても無断補正せず保存する。

## Port一覧

引数の`&`は借用、戻り値は所有データ。`Arc`はimmutable共有。`Result`は成功/失敗を明示し、panicで通常の不正入力を処理しない。子は他の子の実装を呼ぶ代わりにP0のPortを受け取る。

| owner / Port | 公開操作の署名 | 結果と制約 |
|---|---|---|
| P0 / Clock | `sample() -> ClockReading` | 同一origin、thread-safe。Fakeで決定的な採時が可能 |
| A01 / Codec | `push(bytes: &[u8]) -> DecodeStep`、`next_frame() -> Result<Option<DecodedFrame>, ProtocolError>`、`encode(frame: &Frame) -> Result<ByteBuffer, ProtocolError>` | DecodeStepは消費byte数とneed-drain。1回で無制限Vecを返さない。DecodedFrameはFrame＋原wire bytes＋WARN一覧。valid prefixを逐次排出、Fatal後の同接続処理禁止 |
| P0/P1 / RawIngressSink | `try_submit(item: IngressItem) -> SubmitResult` | 結果はAccepted / Full(item) / Closed(item)。full/closed時は所有権を返す。捨てない。broker別FIFOを維持 |
| P0/P1 / HealthSink | `update(delta: HealthDelta)` | bounded最新値/counter。blocking Raw enqueueに依存しない |
| P0/P1 / TransportControl | `close_connection(broker, generation, reason)` | bounded制御通知、同generationだけ閉じる、冪等。旧接続のfaultで新接続を閉じない |
| A03 / Receiver | `run(config, codec: CodecPort, clock: ClockPort, ingress: RawIngressSink, health: HealthSink, control: ControlInbox, shutdown: StopToken) -> ReceiverReport` | 接続ごとCodecをreset。採時→Raw submit待ち。ReceiverReportに未送出range等 |
| A02 / EA facade | `OnInit / OnTick / OnTimer / OnDeinit`と`CopySource / SendSink / EaClock` test adapter | 外部公開契約はwire。内部はcursor回収・送信deadlineをFakeで検証可。OnTimerも有限仕事量 |
| A04 / Normalizer | `normalize(tick: &ObservedTick, profile: &TimeProfile) -> Result<NormalizedTick, TimeError>` | rawを改変しない。未検証/overflowは失敗と診断、Lead入力は別経路 |
| A04 / CandleBook | `on_tick(tick: &NormalizedTick) -> CandleDelta`、`advance_utc(now: UtcMs) -> CandleDelta`、`view(request: CandleViewRequest) -> CandleView`、`begin_segment(segment, profile)` | Slot状態、revision、遅着/保持外診断。GUI期間分のbounded immutable chunkを返す |
| A05 / QuoteMetrics | `on_tick(tick: &ObservedTick, meta: &SymbolMeta) -> QuoteUpdate`、`view(now: MonoNs) -> MetricsView` | 新規の有効quoteに対しSpread/Diff更新。生Tick数とprice event数を分離 |
| A05 / MoveDetector | `on_tick(tick: &ObservedTick, meta: &SymbolMeta) -> Option<MoveEvent>`、`reset(segment)` | liveの順序確定Tickだけ。anchor/cooldown意味論を遵守 |
| A05 / EventMatcher | `on_event(event: MoveEvent) -> Option<LeadLagMatch>`、`advance_watermark(w: MonoNs) -> ExpiredSummary`、`reset(reason)` | ordered eventを前提とする。未対応集合有限、片event一度だけ消費 |
| A06 / LogSink | `try_append(record: Arc<LogRecord>) -> AppendResult`、`flush(request_id, durability)`、`finish(deadline)` | 結果はAccepted / Full(record) / Fault(record,error)。Acceptedはqueue受理。flush/finishの完了は別reply。失敗recordの所有権を保持 |
| A06 / LogWorker | `run(config, bounded_input, health, stop) -> LogReport` | 完了seq/range、durable範囲、未完了範囲を分離。buffered I/O |
| A06 / LogReader | `next_record() -> ReadResult` | 結果はRecord / CleanEof / TruncatedTail / Corrupt(error)。テスト/検証用。製品Replay UIを作らない |
| A07 / SnapshotBuilder | `build(projection: &EngineProjection, health: &HealthState, clocks: PublishClocks) -> Arc<UiSnapshot>` | 分析結果を再計算しない。表示age・status・bounded viewを構成 |
| A07 / SnapshotExchange | `publish(snapshot: Arc<UiSnapshot>)`、`load_latest() -> Arc<UiSnapshot>` | atomic一括公開、latest-state。全historyをqueueしない |
| A08 / Dashboard | `draw(ctx, snapshot: Arc<UiSnapshot>, display: &DisplayConfig) -> UiIntent` | 1描画で同じsnapshotを使用。UiIntentはClose等UI lifecycleだけ。価格処理なし |
| P1 / Engine | `on_ingress(item)`、`on_clock(clock)`、`request_projection() -> Arc<EngineProjection>`、`shutdown(deadline)` | 統合順、session/seq、Raw受理、Port呼出、ring上限を所有 |
| P1 / Config | `load(path) -> Result<AppConfig, ConfigErrors>`、`validate(config)` | unknown/旧keyを黙認しない。全単位・上限・broker対応を検証 |

`CodecPort`はP0のtraitまたは関数adapter、A01はその実装、A03の単体試験ではFakeを注入する。同様にA08はSnapshotExchange具象に依存せず、app側からArcを受け取れる。CandleとMetricsは互いの実装へ依存しない。

## 分析・Snapshot出力型

| 型 | 内容 |
|---|---|
| `CandleSlot` | broker、segment、period_ms、start_utc_ms、Empty/Active/Closed、`ohlc: Option<Ohlc>`、tick_count:u64、revision、coverage/integrity |
| `Ohlc` | open/high/low/close、open_key/close_key=(UTC,seq)。Emptyでは存在しない |
| `CandleView` | 同一slot_start配列上のbroker A/BのSlot、bounded Arc chunks。分析用のsynthetic値を含めない |
| `MetricsView` | A/B quote、bid/ask/mid/spread_diff、optional cross gap、age/validity、spread warning、Tick rate |
| `MoveEvent` | event_id=(segment,broker,trigger_seq)、rx_mono_ns、direction、anchor/current Mid、mid_delta_points、bid/ask/mid/spread_delta、quality分類 |
| `LeadLagMatch` | match_id、event_a/event_b ID、t_A/t_B、signed_delta_ns、leader、abs_delta_ns、raw_delta_ms、optional ema_delta_ms、segment |
| `EngineProjection` | revision、processed watermark、broker/session/segment情報、MetricsView、CandleViews、bounded tick/diff line系列、LeadLagView、analysis health |
| `UiSnapshot` | schema_revision、snapshot_revision、projection_revision、run_id、built_mono_ns、processed_watermark、display_now_utc、A/B views、diff、LeadLag、合成HealthState、diagnostics |

全体は一貫した1つのProjection revisionから作る。healthだけはより新しい観測を併記できるが`health_observed_mono_ns`を別に持つ。Candleや価格のrevisionをhealth更新で進めない。

## P0で固定する設定schemaと初期値（B、性能保証ではない）

| グループ | key / 初期値 |
|---|---|
| broker | A id=1/port=39001、B id=2/port=39002、host=`127.0.0.1`、実symbol・point_size・pip_size・digitsは必須明示 |
| time | broker別source=manual、offset_secと検証状態必須。値未確認ならnormalized Candleを未検証表示 |
| EA取得 | warmup_seconds=60、copy_batch_count=256、max_same_ms_scan_ticks=65536、max_copy_calls_per_event=8、max_ticks_per_event=2048、max_processing_time_us=2000（API callのpreemption保証ではない） |
| EA送信 | max_ticks_per_packet=256、max_batch_delay_us=0、pending_tick_capacity=65536、socket_send/receive_timeout_ms=10、frame_send_deadline_ms=10、reconnect_interval_ms=1000 |
| protocol | payload最大1,048,576 byte、header=40、debug resync上限65,536 byte、ack_mode=off |
| ingress | broker別最大256 Frameかつ8MiB、decode buffer最大header+payload、progress_interval_ms=1、pending完全Frame最大1個/Receiver |
| logger | queue最大1024 recordかつ32MiB、buffered flush_interval_ms=1000、shutdown_drain_timeout_ms=5000、logging.enabled=true |
| matcher | trigger_move_points=2、matching_window_ms=100、event_cooldown_ms=20、ema_alpha=0.1、pending_event_capacity=8192 |
| health | stale_after_ms=1000、heartbeat_interval_ms=250、heartbeat_timeout_ms=1500 |
| display | timeframe_ms=60000、repaint_hz=60（対応60〜120）、always_on_top=false、Tick/差分visible_seconds=60 |
| history | period別slots={1000:60,5000:24,10000:12,60000:10}、raw表示ring最大120000 ticks/broker、health mailbox固定broker2+global、snapshot exchange1 |
| integrity/diagnostic | 重複照合ledger最大16384 TickId/broker、表示segment履歴最大2、詳細診断queue最大1024件かつ1MiB、close制御mailboxはbrokerごと最新1要求 |

リングの古い表示履歴のevictはLoggerの生Tick保存とは別。容量不足は表示保持範囲の短縮として明示する。warmup、tick/diff ring、match queue、log recordにもbyte上限を適用する。受信上限とqueue予算の組合せが1つの最大Frameを受理できることを設定検証する。

queue予算には原wire bytesとdecode済みrecordの両方の実メモリを含める。segmentを増やして保持量を無制限に乗算しない。詳細診断queueが満杯ならhealthの最新fault・累積counterは維持し、詳細省略件数を記録する。この診断集約をRaw Tickのdropへ適用しない。

Spread warning閾値・USDJPY実symbol/point/pip/offsetはbroker固有であり、無根拠な値を固定しない。未設定なら該当補助機能を未設定表示し、価格単位のraw値は表示可能。Leadのpoints閾値はpoint_size確認まで有効化しない。

## LogRecordの公開schema

LogRecordは`RawFrame`、`Metadata`、`Diagnostic`の3variant。RawFrameはReceivedFrame＋config_epoch＋analysis_segment＋Tickごとのsequence採否。Metadataは設定原本と検証状態を含む構成値。Diagnosticは共通Diagnostic型。byte配置と永続化の意味は[storage-format.md](storage-format.md)に固定する。

原wire bytesはCodecからTransport/Engine/LoggerへArc共有し、A06がA01のencoder実装に依存して再構成しない。quote品質など分析後に確定する診断は、元TickIdに紐づく別Diagnosticとして追記する。
