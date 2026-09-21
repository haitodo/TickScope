# 検証計画と完了判定

この文書は将来実行する試験の設計。現時点で試験コード・実機結果は存在しない。各IDは[tasks.md](tasks.md)の完了条件と[不変条件](invariants.md)へ接続する。

## 1. 共通Fixtureの責任

P0が独立した期待値を`tests/fixtures/`に置く。wire bytes、logical Tick、共通mono時刻、UTC設定、期待OHLC/イベント/match/statusを分離する。Rust encoderを通した値だけでMQL5 decoderの正しさを証明しない。双方が同じ誤実装になる可能性を除くため、byte offset表と手計算oracleを持つ。

Clockはテストで制御可能にする。broker/EA/mono/wallは別々に変えられる。sleepで境界時刻を偶然作る試験に依存しない。Socket/file/queueはshort write、full、timeout、断線を注入可能にする。

## 2. 試験一覧

| ID | 入力・手順 | 合格条件 | 主担当 / 出典 |
|---|---|---|---|
| T-W01 | Header40/Tick72と全ControlのGolden bytes、signed/float/seq0 | 全offset/length/LE一致、MQL5/Rust相互decode一致 | A01,A02 / §19,§95.2 |
| T-W02 | 全split位置、1byte単位、Header/record途中、複数Frame結合 | 境界によらず同じFrame列、欠落・二重排出なし | A01,A03,P2 / §18.2,§95.2 |
| T-W03 | magic/version/type/flags/length異常、overflow、EOF途中 | Productionは診断・切断、finite memory、NeedMoreを早期誤判定しない | A01,A03 / §18.2.1 |
| T-W04 | reserved非zero、NaN bit、debug resync limit、STATUS/ACK不整合 | reservedはWARN、raw bit維持、scan上限厳守、その他規定通りfatal | A01,A02 / §95.2 |
| T-T01 | 同一ms4件、同内容別出現、境界再取得、257/1000件同一ms | 全新規出現1回ずつ、time+1/hash dedupなし、上限時明示fault | A02 / §10,§65 |
| T-T02 | burst100/300/1000/5000、イベントbudget、後続OnTickゼロ | 分割回収とTimerで追随、有限memory、cursor異常を隠さない | A02 / §10.3,§95.1 |
| T-T03 | short write、0byte進捗、timeout、断線、pending超過 | suffix追跡、有限deadline、UNCONFIRMED/DATA_LOSSとknown/unknown区別 | A02 / §17.1,§21 |
| T-T04 | seq0、gap、同ID同payload/異payload、再接続/new session/Rust restart | 再送観測と新Tickを区別、分析二重加算なし、prefix未観測を損失断定しない | A02,P1 / §20,§64,§67–68 |
| T-T05 | Frame内複数Tick、enqueue逆転、receiver idle/blocked、clock変更 | 共通rx/frame同時刻、watermark以下の正しい順、queue待ちで採時変更なし | A03,P1 / §14,§21 |
| T-C01 | UTC境界、負epoch、同時刻seq、遅着順列 | fixed half-open Slot、正しいOHLC/count、到着順に依存しない | A04 / §29–33 |
| T-C02 | Aのみ停止、両側停止、Heartbeatのみ、advance UTC | 共通Slot位置、Empty/None、偽OHLCなし | A04,A08 / §31.1,§95.1 |
| T-C03 | Closed遅着、保持外、60秒warmup、segment/config変更 | revision/coverage明示、有限保持、raw維持、過去を黙って改変しない | A04,P1 / §12,§31–34 |
| T-C04 | 実端末複数sample、DST、Tick停止、OnTick/Timer、PC時刻変更 | offsetと誤差を記録、単発coarse値を採用しない、UTCの検証状態が追える | P3 / §13,§33,§95.2 |
| T-C05 | CopyRatesのM1と同じsymbol/価格モードの生成足 | 確定OHLC/volume/現行足を別評価、乖離原因を所定順で調査 | P3 / §63 |
| T-M01 | A/B最新quote、片側未取得/stale、point差、invalid quote | Bid/Ask/Mid/Spread差=A-B、age、None、不正分析除外、raw維持 | A05 / §27–28,§45–46,§75,§88 |
| T-L01 | 同じ価格/rxでbroker/EA/wall時刻だけ変える | Lead結果不変、Warmup非投入、UTC設定に依存しない | A05 / §42 |
| T-L02 | 一方向trend、巨大jump、1point往復、Spread拡大、cooldown境界 | 有意Mid発火、即再アンカー、cooldown中anchor固定、quality保持 | A05 / §42,§95.1–95.2 |
| T-L03 | 候補複数、tie、delta0/100/101ms、逆方向、逆順送出 | 最小差1件、片event一度、strict positive/inclusive window、順序確定後判定 | A05,P1 / §43 |
| T-L04 | leader反転、EMA、gap/session/reset、pending上限 | signed=tB-tA、raw不変、cross-segmentなし、分析overload明示 | A05 / §44 |
| T-H01 | connected無Tick＋Heartbeat、Heartbeatも停止、TCP切断、warmup同期 | 直交status、Heartbeatは市場Tickでない、年齢はmono | P1,A02,A08 / §47–49,§71 |
| T-B01 | UI pause、slow Engine、Raw full、Logger full、回復 | Raw無通知drop0、容量以内、TCPへbackpressure、UIのみ置換 | A03,A06,P1,P2 / §24–25 |
| T-B02 | disk full、Receiver停止、health通知中のRaw full、shutdown期限 | fault/status表示、未永続化・不確定range報告、終了無限待ちなし | P1,P2 / §17.1,§25,§72 |
| T-G01 | 同一ms/同内容/異seq、全raw field、run/phase/config metadataの保存読戻し | bit完全、ID/frame時刻復元、rawと分析採否分離 | A06 / §50–51 |
| T-G02 | partial file write、flush error、crash相当tail、未知log version | 末尾不完全検出、有限length、完了/durable範囲の虚偽なし | A06 / §50–52,§72 |
| T-S01 | 2000tick/s、UI停止/再開、60Hz公開、無Ticktimer | Raw count保持、公開上限、最新状態、age正確 | A07,P1 / §41,§53 |
| T-S02 | UIの全状態fixture、Empty、符号、最小化/移動/終了 | 正しい固定描画、単一revision、GUI内I/O/分析なし | A08,P3 / §35–46,§69 |
| T-S03 | 長期publishとArc保持、chunk変更、表示ring overflow | finite memory、全履歴deep-copyなし、表示短縮とRaw損失を区別 | A07,P3 / §34,§54,§70 |
| T-I01 | Fake EA2系統→実Codec/Receiver/Engine/Logger/UI | §88の機能を結線、binary/TCP/eguiのみ、取引なし | P2 / §77–80,§88 |
| T-I02 | MT5終了/再起動、EA再attach、Rust終了/再起動、partial Frame中切断 | 自動再接続、session/seq整合、GUI停止なし、保証外range明示 | P2,P3 / §67–68,§79 |
| T-I03 | 連続数時間〜数日、ウィンドウ操作、ログ成長 | RAM/thread/handleが説明不能に増えず、log成長は入力に比例 | P3 / §69–70 |
| T-P01 | 同条件でACK off/diagnostic/forced、Batchサイズ・頻度を固定 | 時計domainを守ったp50/p95/p99、条件・標本数・誤差。改善なしならforced不採用 | P3 / §16.1,§18.1,§95.1 |
| T-P02 | 100/500/1000/2000tick/s、burst、各処理時間を測定 | Raw silent drop0、bounded、GUI操作可能、queueとlatencyを分離記録 | P3 / §66,§89–90 |

## 3. 遅延を測るときの契約

EA: CopyTicks、packet構築、SocketSendの開始/終了を同じea_elapsed_usで測る。Rust: decode、ingress待ち、Engine、Logger、Snapshot age、GUI frameを共通monoで測る。Logger queue待ちとdisk処理を別々に記録する。

EA送信時刻とRust受信時刻はepochが違う。`rx_mono_ns - ea_elapsed_us*1000`を一方向遅延として報告しない。ACKの往復はEA同clockでRTTを計測できるが、RTT/2を校正なしに一方向値と呼ばない。必要なら複数の往復sampleからclock対応と不確かさを推定し、誤差を超える変化だけを評価する。

報告にはMT5/Rust/Windows build、CPU、broker、負荷、sample数、測定区間、Batchサイズ、送信頻度、timeout、ACK mode、Rust NODELAY設定、分位点を含める。ウォームアップ同期と通常Liveを分離。性能の絶対保証値は原仕様にないため、事前に記録した評価条件で比較する。

## 4. MVP判定

| 主仕様§88 | 完了証拠 |
|---|---|
| 2社MT5、Bid/Ask、sequence、再接続 | T-I01の結線に加えP3実MT5とT-I02結果 |
| TickからM1、形成中更新、共通時間軸 | T-C01〜T-C05、T-S02、UTC検証済profile |
| Bid/Ask/Mid/Spread差、Lead/Lag | T-M01、T-L01〜T-L04、PC観測という表示 |
| 軽量固定GUI、zoom/pan/indicatorなし | T-S02、T-P02、対象外機能が混入しないreview |
| 生Tick binary保存 | T-G01/T-G02とRaw full系試験 |

単体テスト合格だけではP3完了にならない。実機データがない項目は未検証として残し、対象条件でのRaw完全性・UTC・TCP遅延が確認されたとは主張しない。

## 5. 主仕様のカバレッジ

| 主仕様範囲 | Blueprintの対応 |
|---|---|
| §1–7 目的・基本設計・環境 | README、I18/I22、architecture、A08/P2 |
| §8–14 EA回収・clock | Tick意味論、I01–I04/I16/I19/I20、A02/A04/P1 |
| §15–21 通信・wire・seq | Wire、D01–D08、A01–A03 |
| §22–28 module・queue・計算 | architecture/interfaces、A05/P1 |
| §29–34 Candle・時間・保持 | Candle意味論、A04 |
| §35–41 GUI | Snapshot意味論、A08 |
| §42–46 Lead/Spread | Lead意味論、A05 |
| §47–49.1 状態・Heartbeat | Tick/Snapshot意味論、Wire、P1 |
| §50–57 保存・共有・設定・symbol | interfaces、保存契約、A06/A07/P0/P1 |
| §58–62 負荷・技術選定 | I22、A08/P3、T-P02 |
| §63–70 検証 | 本文T-C/T-T/T-I/T-P |
| §71–80 状態・診断・禁止・整合性 | I14–I24、P1/P2、T-H/T-B/T-I |
| §81–88 実装順序・MVP | tasks P0〜P3、上記MVP表 |
| §89–94 性能・時計・最終構造 | architecture、時計契約、T-P01/T-P02 |
| §95–95.3 追加不変条件・試験 | invariants、全T-W/T-T/T-L/T-C、D台帳 |
| §96 / Appendix A | README、invariants全体、実測と保証の分離 |
