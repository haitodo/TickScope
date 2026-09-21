# エージェントへの委任契約

この文書のタスクは将来の実装用。今回の作業では実行しない。主仕様は[spec.md](../spec.md)、共通前提は[README](README.md)、[不変条件](invariants.md)、[公開IF](interfaces.md)、[編集所有権](architecture.md)。

## 全担当共通ルール

- P0が配布した同じ契約revision・共有型・Fixtureを使用する。兄弟の未完成実装を必要とする単体試験を作らない。
- 他担当の具象moduleではなく共通Portを参照し、自分のtest配下にFakeを置く。共有Fixtureの期待値を自分の実装に合わせて変更しない。
- 編集は指定pathだけ。root Cargo、共有型、親所有mod.rs、原仕様/Blueprint、他担当ソースは読み取り専用。新規依存が必要なら親へ具体的な差分を提案する。
- 各タスクは`doc/implementation-reports/<ID>.md`へ、契約revision、変更path、テストと結果、既知の制約、未検証の実機項目を記録できる。未実行試験をpassにしない。
- 完了には実装＋担当範囲の検証＋引渡し報告が必要。全体MVP合格とは区別する。shared契約の矛盾が出たら親へD番号で報告し、独自拡張しない。
- 未許可のRaw drop、synthetic分析足、broker時刻Lead、取引処理、unbounded queue、テキストIPCは禁止。

## P0 — 親: 共通契約と独立ビルドの準備

- 入力: 主仕様全体、本Blueprint全体、D01〜D22、原本hash。
- 出力: 凍結したC1型/Port、設定schema、module/test target骨格、独立Fixture、契約適合用test support、各Aタスクの明確な開始通知。テスト用Clock/Sinkなどの最小Fakeを共有可能にする。
- 編集可能: `src/contracts/**`, `src/lib.rs`, 親所有`src/tick/mod.rs`/`src/metrics/mod.rs`/`src/state/mod.rs`, `Cargo.toml`, `Cargo.lock`, `tests/fixtures/**`, `tests/support/**`, `config/**`。Blueprint改訂は親のみ。子所有ファイルが必要な初期stubは委任前だけ作成し、引渡し後は子が所有。
- 依存: なし。実機Vを検証済みとは扱わない。
- 完了条件: wireの全message、status、flags、未取得値、Clock domain、seq/session、matcher順序、Snapshot schema、log schemaとlimitsに未定型がない。各AタスクがFakeのみで個別ビルド/テストできる。path所有権・新規crateの承認済み集合・test実行方法を配布済み。
- テスト: 固定wire vectorの手計算対照、型/単位と全enum網羅、Golden Fixtureの期待結果を独立レビュー、契約型と各harnessのcompile-only確認、所有path交差検査。Fixtureは実装serializerの出力で自己生成しない。
- やってはいけないこと: 補足Bを原仕様Sとして記載、Vをpass扱い、親のstubを製品実装完成と主張、共有編集競合を残して8担当へ投げる。

## A01 — Agent-Wire: binary protocol Codec

- 入力: C1共有型、[Wire Format](wire-format.md)、T-W01〜T-W04のGolden/異常Fixture。
- 出力: explicit LE encoder/decoder、bounded streaming buffer、typed fatal/WARN、Production切断判断に必要なerror。Socketなしでbyte streamを処理できる。
- 編集可能: `src/protocol/**`, `tests/protocol/**`, `doc/implementation-reports/A01.md`。
- 依存: P0のみ。A02/A03の実装を待たない。
- 完了条件: Header40/Tick72/Heartbeat36/ACK8/Status48、全field offset・符号・float bit・flagsを満たす。valid prefix後のmalformedを正しく区別。NeedMoreとfatalを分け、boundedメモリ・checked長さ計算、未知reserved WARN。encode/decode以外の責務がない。
- テスト: T-W01〜T-W04。全分割位置、1byte入力、coalesced複数Frame、最大長・overflow、truncated EOF、未知version/type、非zero reserved、NaN bit保存、debug scan上限。最低限structured fuzz入力計画を実行しpanic/無制限確保なし。
- やってはいけないこと: ABI直読み、JSON、Socket接続、受信時計付与、未知flags黙認、reserved WARNをfatalに変更、shared constants独自変更。

## A02 — Agent-EA: MQL5回収と送信

- 入力: Wire C1、Tick意味論、EA設定schema、CopyTicks/Socket/時計Fake、Golden Fixture。実機口座・symbolはP3が用意する検証入力であり、推定値を埋めない。
- 出力: A/B両端末で同じソースを使うEA、bounded cursor回収、warmup/live境界、seq/session、binary送信/Reply decoder、Heartbeat/STATUS、再接続、localhost許可設定手順。
- 編集可能: `mt5/**`, `tests/ea/**`, `doc/implementation-reports/A02.md`。売買用EA追加は禁止。
- 依存: P0のみ。Rust serverはFakeでよく、A01/A03待ちにしない。
- 完了条件: same-ms同内容出現を維持、same-ms件数>batchで前進できる範囲を確認、上限では明示CURSOR_BLOCKED。OnTick/Timer budget、short writeのsuffix管理、deadline、有限pending、UNCONFIRMED/DATA_LOSS、同session再接続、seq0未取得区別、Warmup明示。MetaEditor compile合格。未導入ならコード引渡しはできても、この完了条件は未達として報告する。
- テスト: T-T01〜T-T04、T-W01/T-W04 writer適合、T-H01 Timer、burst100/300/1000/5000、同一ms257/1000件、同内容複数、最大走査超過、境界prefix変化、最終burst後無Tick、partial send→断線、buffer上限。実機でしかできないMT5同期時間等はP3へ引継ぐ。
- やってはいけないこと: OnTick=1tick、time+1、hash dedup、count=0、無限retry、pending無限拡張、socket suffixを新接続へ送る、DLL/Win32導入、ACKを永続化確認とする、取引、Candle/Lead計算。

## A03 — Agent-Transport: TCP receiverと受信metadata

- 入力: CodecPort、ClockPort、RawIngress/HealthSink、TransportControl受信、Wire契約、ingress水位契約、transport config、Fake Codec/Sink。
- 出力: broker別localhost listener/receiver、frame単位採時、FIFO ingress/Progress/End、disconnect/reconnect待ち、optional ACK、計測counter。
- 編集可能: `src/transport/**`, `tests/transport/**`, `doc/implementation-reports/A03.md`。
- 依存: P0のみ。A01の具象CodecはP2で注入する。
- 完了条件: 共通run clock、完全frame後queue待ち前の採時、frame内同時刻、pending1frame上限、Fullで所有権維持、watermarkが未送出Frameを越えない、port/broker一致、重複接続拒否、Production malformed切断、stop/deadline、Rust set_nodelay(true)とその結果の診断。
- テスト: T-T05、T-W02/T-W03との境界適合、T-B01 Fake full/closed、T-H01 End、T-I02再接続。時計の進んだ後enqueueしてもrx不変、Receiver別送出順逆転とProgress、ACKはaccepted後・off時ゼロ、loopback fake sender。
- やってはいけないこと: Engine処理時刻への採時延期、broker/EA時刻の差で遅延算出、Rawのlatest優先drop、busy-loop、0.0.0.0 listen、MQL5送信Nagle無効化を保証、Candle/sequence ledger所有。

## A04 — Agent-Candle: UTC NormalizerとCandleBook

- 入力: ObservedTick、TimeProfile、SymbolMeta/PriceMode、共通型、Candle意味論、Fake UTC clock、T-C Fixture。
- 出力: rawを保存した正規化結果、period汎用CandleBook、固定Slot、late revision、Empty/Active/Closed、coverage、bounded CandleView。
- 編集可能: `src/tick/normalize.rs`, `src/tick/candle.rs`, `tests/candle/**`, `doc/implementation-reports/A04.md`。
- 依存: P0のみ。実Transport/Engineを使わずfixture Tickを直接投入。
- 完了条件: 1/5/10/60秒、BID default、ASK/MIDの型設計、A/B共通UTC index、floor境界、同時刻seq tie、正しいOHLC件数、EmptyのOHLC=None、遅着Closed改訂、保持外診断、segment分離、未検証offsetを正常UTCに見せない。
- テスト: T-C01〜T-C03、T-C04の合成部分、tick入力順を変えてOHLCが不変、負UTC、checked overflow、DST/profile変更、巨大wall-clock jump、no-tick timer、same-ms同内容複数。実端末UTC/CopyRatesはP3。
- やってはいけないこと: rxでSlot生成、Heartbeatで足生成、前値フラット足を分析へ追加、CopyRatesを本番OHLC入力にする、GUIでの再計算、offsetをTimeCurrent-TimeGMT単発から固定。

## A05 — Agent-Metrics: 価格計算・イベント・matcher

- 入力: ordered ObservedTick/MoveEvent、SymbolMeta、Lead config、Fake mono clock、Tick/Lead意味論、共通Fixture。
- 出力: Spread/min/max/warning、Bid/Ask/Mid/Spread差、age/validity、significant MoveDetector、1対1EventMatcher、signed raw/EMA、bounded状態。
- 編集可能: `src/metrics/spread.rs`, `src/metrics/price_diff.rs`, `src/metrics/lead_lag.rs`, `src/tick/matcher.rs`, `tests/metrics/**`, `doc/implementation-reports/A05.md`。`src/metrics/mod.rs`は親所有。
- 依存: P0のみ。A04の正規化/Candleを使わない。
- 完了条件: as-of差分とSTALE区別、非有効quote除外、event anchor固定/発火時reset、cooldown境界、actual delta、quality、最小差1候補、event再利用ゼロ、同時刻非match、signed=tB-tA、EMA表示分離、Warmup/segment reset、pending上限診断。
- テスト: T-L01〜T-L04、T-M01。片側未取得、異なるdigits/point/pip、NaN/Inf/crossed quote、一定offset、ノイズ、trend/巨大jump、tie/窓境界、同一frameの複数Tick、detectorとmatcherを個別に検証。
- やってはいけないこと: broker時刻Lead、absolute価格一致、次Tick勝負、cooldown中anchor追随、巨大jumpを架空複数eventへ分割、多重match、EMAで原値置換、SpreadDrivenを無断除外、point_size推測。

## A06 — Agent-Storage: binary raw logger

- 入力: [保存契約L1](storage-format.md)、LogRecord schema、LogSink/HealthSink、容量・flush/終了設定、Fake file writer、raw/diagnostic Fixture。
- 出力: bounded logger worker、version付き追記binary log、flush/durability/failed range報告、検証reader、truncated tailの検出。
- 編集可能: `src/storage/**`, `tests/storage/**`, `doc/implementation-reports/A06.md`。
- 依存: P0のみ。Receiver/Engine実装からファイルを作ってもらわずfixtureを入力する。
- 完了条件: 全raw field・ID・受信時刻/run・phase/frame境界・分析採否・設定epochを復元可能。診断とTickを区別。queue受理とdisk完了の差を公開、Fullはrecord返却、disk errorをhealth通知、有限buffer、deadline付きdrain、既存logを無断上書きしない。
- テスト: T-G01/T-G02。bit完全roundtrip、同内容別seq、再送観測、run再起動、最大record長、partial write/flush failure、disk full、tail切断、version不一致、slow sinkによるbackpressure、最後の未永続化範囲。
- やってはいけないこと: Raw drop/coalesce、UI thread I/O、JSON/CSV tick保存へ置換、queue acceptedをdurable扱い、壊れたtailを無言成功、分析値を書き戻す、Replay製品機能追加。

## A07 — Agent-Snapshot: Snapshot構築と最新値交換

- 入力: EngineProjection/HealthState/PublishClocks、Snapshot schema、全状態のfixture、Snapshot意味論。
- 出力: immutable UiSnapshot builder、ArcSwap latest exchange、chunk共有、age/coverage表示用view。公開周期のスケジューラはP1 runtimeから呼ばれる。
- 編集可能: `src/state/snapshot.rs`, `tests/snapshot/**`, `doc/implementation-reports/A07.md`。
- 依存: P0のみ。Candle/Metricsの実装を使わずProjection fixture。
- 完了条件: 1projection由来の一貫性、health freshness別管理、未知値Option維持、最新1個交換、履歴deep-copy回避、有限chunks、GUI長期未読でもRawへ影響しない。
- テスト: T-S01/T-S03、revision混在防止、健康状態overlayで価格時刻不変、thread間load/store、古いArc保持時も内容不変、両brokerの共通Slot view、builderで分析結果が変わらない。
- やってはいけないこと: Candle/Spread/Lead再計算、unbounded Snapshot queue、Tickごとの強制公開、Arc内mutable分析状態、GUIの描画中lock要求、raw countをcoalesce。

## A08 — Agent-UI: 固定監視パネル

- 入力: UiSnapshot/DisplayConfig/SymbolMeta、Snapshot fixture、主仕様§35–46,§74–75,§88。
- 出力: egui Painterの価格・Candle・差分・Lead・status画面、Debug overlay、Close等UiIntent、fixtureで起動可能な表示検証。
- 編集可能: `src/ui/**`, `tests/ui/**`, `doc/implementation-reports/A08.md`。
- 依存: P0のみ。A07の具象exchange不要、P0 fixture Arcで描画可能。
- 完了条件: M1主表示、A/B共通X軸、形成足更新、Emptyは空白、Bid/Ask/Mid/Spreadと各差分、Observed Lead/raw/EMA、Tick rate、直交status、unknown/stale/age、固定右端、モニター更新頻度に従う。Main appのrun_native結線はP2。
- テスト: T-S02のfixture/描画geometry・smoke、同じArcで1frame描画、同値OHLCの可視性、Emptyとsyntheticの区別、A/B leaderと符号、長いbroker名/桁、縮小窓、status重複。実データ高負荷の操作試験はP3。
- やってはいけないこと: socket/disk/分析をUI callbackに追加、汎用chart/WebView/Tauri、zoom/pan、発注、broker変更UI、取引上の市場先行と断言、足を自分で補完。

## P1 — 親: Engine・設定・clock・state統合

- 入力: C1共通Port/型、全意味論、設定schema、Fake Port。A01〜A08の実装はこのタスク開始の前提ではない。
- 出力: broker別bounded ingressとwatermark merge、seq/session ledger、raw保存受理→分析の順序、analysis segment、rings、health、publish coordinator、Config/Clock、shutdown手順。
- 編集可能: `src/tick/engine.rs`, `src/config.rs`, `src/runtime/**`, 親所有module宣言、`tests/integration/engine/**`, `tests/support/**`, `config/**`, `doc/implementation-reports/P1.md`。
- 依存: P0。子と並行可能だが、複数境界に責任を持つため親が担当。
- 完了条件: 時刻の違うdomainを混ぜない、watermarkより先を確定しない、同一ID重複と正当な同内容別seqを区別、Warmup/Liveとnormalization条件、Raw fullで所有権維持、Logger停止中もhealth公開とUI応答、設定値validation、有限memory、期限付きstop。
- テスト: T-T04/T-T05、T-B01/T-B02、T-H01、T-S01、T-I02、Fake複数Receiverのenqueue順逆転、tickゼロ中のage/slot進行、seq gapでmatcher reset、Full/Closed/Fault全戻り分岐。config未知key・不正上限・overflowも検証。
- やってはいけないこと: 子の領域を同時編集、一時的にunbounded queueで統合、受信時刻をEngine dequeue時に変更、warmupをliveへ混在、Raw loggerの失敗をSnapshot更新で隠す。

## P2 — 親: 実モジュール結線と統合合格

- 入力: A01〜A08とP1の実装・報告・単体結果、C1、主仕様MVP条件。
- 出力: Windows app起動/停止、全具象Port結線、config配布、EA接続手順、end-to-end試験結果。主仕様のPhase1〜7を最終結線順序として確認する。
- 編集可能: `src/main.rs`, `src/lib.rs`, 親所有module宣言、Cargo関連、`src/runtime/**`, `tests/integration/**`, `config/**`, `doc/validation/**`, `doc/implementation-reports/P2.md`。子所有コード修正が必要なら担当へ返却し、引渡し済みの所有権移管を明示してから触る。
- 依存: P1 + A01〜A08。単体完成と統合完成を混同しない。
- 完了条件: fake sender2系統からbinary→保存/分析→Snapshot→GUI、reconnect、malformed、disk stall、UI pause、shutdownが仕様通り。共有型の無断variant変更なし。Raw入出力件数・gap・未確定範囲を照合できる。
- テスト: T-I01/T-I02、T-W02/T-W03を実Codec+Transportで再現、T-B01/T-B02を実queue+Loggerで実施、T-H01、shutdown未完了報告。Candle/Leadの原値とFixture oracleを照合。
- やってはいけないこと: 実機未検証をMVP完成扱い、統合の都合で原仕様不変条件変更、ACK forcedを無計測で初期化、test期待値を実結果に合わせて緩める。

## P3 — 親: 実機検証・性能測定・MVP判定

- 入力: P2のWindows build、2つのMT5端末/EA、broker実symbol、観測可能な実Tick、P0/P2のテスト結果、validation計画。
- 出力: UTC/point/pip検証表、CopyRates比較、遅延/負荷/長時間測定、未達と限界、MVP合否。`doc/validation/`に環境・条件・sample数・結果を保存する。
- 編集可能: `tests/system/**`, `doc/validation/**`, `doc/implementation-reports/P3.md`。本番コードを直す場合は原因moduleの所有者へ差分を戻して再検証。
- 依存: P2。MT5/市場データ/MetaEditorがない場合は実行不能事項を具体的に報告し、そのgateは未完了のまま。
- 完了条件: T-C04/T-C05、T-P01/T-P02、T-I02/T-I03を実施し、§88を項目別に確認。性能は実測値と条件を提示、TCP/ACK効果は誤差・試行条件付き。数時間〜数日の試験は実際の継続時間を記録する。
- テスト: 100/500/1000/2000tick/s、burst、receiver pause、disk stall、2MT5/EA再起動、Rust再起動、GUI操作、DST等時刻検証、ACK3mode比較、CPU/RAM/threads/handles/logサイズ、OHLC/Tick Volume/current bar分離。
- やってはいけないこと: EA時計とRust時計を無校正で引いて一方向遅延とする、短いsmokeを数日安定と称する、seq連続だけでMT5配信完全性を保証、forced ACK/Named Pipeへ測定なし変更。

## 独立性と親担当の理由

| タスク群 | 他の子のコードへの依存 | 親に残す理由／開始条件 |
|---|---|---|
| A01〜A08 | なし。C1とFakeだけ | P0が型と期待値を固定すれば完全独立の実装・単体検証が可能 |
| P0 | なし | 同じ意味を複数人が別々に定義することを防ぐ |
| P1 | 製品具象には未依存、複数契約には依存 | sequence/clock/backpressure/phaseを横断して整合させる責任者が必要 |
| P2 | 全実装へ依存 | 結線、実際のqueue順序、終了、共通設定は単一ownerで確認 |
| P3 | 実装＋実端末＋環境へ依存 | 複数broker時計と通信を同じ条件で測る必要がある |

委任メッセージは「Axx、契約C1、入力文書、編集path、実行するT番号、引渡し先」を含める。『適当に実装して後で合わせる』という追加裁量は与えない。戻りの報告はinterface差分ゼロを基本とし、差分があるなら親が全消費者へ反映してから再開する。
