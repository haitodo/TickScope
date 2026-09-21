# 実装上の不変条件

出典はすべて[spec.md](../spec.md)。下記は最適化、例外処理、再接続、テスト用経路でも守る。Sは原仕様、詳細な補足は対応する意味論文書を参照。

| ID | 契約 | 主な出典 | 所有者／検証 |
|---|---|---|---|
| I01 | **OnTick != tick count**。OnTickは回収トリガー、`CopyTicks(COPY_TICKS_ALL)`で全未処理Tickを回収 | §9–11, §92 | A02 / T-T01,T-T02 |
| I02 | **broker_time_msc != unique tick ID**。同一ms・同一内容の正当な複数Tickを維持。Hash Setで潰さず、`last_time_msc+1`で飛ばさない | §10, §65 | A02,P1 / T-T01 |
| I03 | **Lead/Lag clock = Rust monotonic receive clock**。全Receiver共通epoch。EA時計・broker時刻・wall-clockを差分時計にしない | §13–14, §42 | A03,A05,P1 / T-L01,T-T05 |
| I04 | **Candle clock = normalized UTC axis**。受信順・到着時刻をSlotキーにしない。raw時刻は維持 | §13, §31–33 | A04 / T-C01,T-C04 |
| I05 | **Raw Tick Path = lossless by default**。通常時のdrop/coalesce禁止。障害時も有限メモリ、損失・不確定範囲を明示 | §17.1, §24–25, §90 | A02,A03,A06,P1 / T-B01,T-B02 |
| I06 | **UI Snapshot Path = latest-state priority**。中間Snapshotのみ置換可能。Tick処理・永続化と分離 | §24–25, §53–54 | A07,A08,P1 / T-S01 |
| I07 | **TCP_NODELAY behavior = measured**。Rust設定はRust送信方向だけ。MQL5送信Nagleの状態・効果を推定で保証しない | §16.1, §18.1, §66 | A03,P3 / T-P01 |
| I08 | **DATA_STALE != EA disconnected**。TCP、Tick鮮度、Heartbeat、負荷は直交した状態 | §47–49, §71, §79 | P1,A08 / T-H01 |
| I09 | **Heartbeat != market tick**。sequence増加・Candle・価格・Lead/Lag・Tick rateへ加算しない | §19.4, §49.1 | A02,P1 / T-H01 |
| I10 | **Candle Empty Slot != synthetic analytical candle**。TickゼロならEmpty。表示補助線・疑似足をOHLC集計・raw logに混ぜない | §31.1 | A04,A08 / T-C02 |
| I11 | **Lead/Lag = significant Mid-move event**。絶対価格一致・次Tick到着・無条件1point変化による判定禁止 | §42 | A05 / T-L01,T-L02 |
| I12 | **Lead/Lag = one-event-to-one-event matching**。同方向、厳密に正の時間差、窓内、最小時間差候補1件。再利用禁止 | §43 | A05 / T-L03 |
| I13 | 発火時にMidへ即再アンカー。Cooldown中はanchorを追随させず、観測値だけ更新 | §42.1, §95.2 | A05 / T-L02 |
| I14 | TCPはstream。Header 40 byte、TickRecord 72 byte、明示Little Endian。ABI・cast・read境界へ依存しない | §18.2–19, §95.2 | A01,A02 / T-W01–T-W03 |
| I15 | malformed frameはProductionで診断・切断。無制限resync・Length由来の無制限確保は禁止 | §18.2.1 | A01,A03 / T-W03 |
| I16 | CopyTicksは明示countと有限catch-up。Socket送信は有限deadline、short write追跡、失敗を状態化 | §10.3, §17.1, §21 | A02 / T-T02,T-T03 |
| I17 | 全queue、pending集合、履歴、receive bufferは有限。GUI停止はRaw Tickを捨てる理由にならない | §24–25, §34 | 全担当,P2 / T-B01 |
| I18 | GUIはSnapshot参照と描画のみ。TCP、Tick集計、Logger I/OをMain threadへ持ち込まない | §4.3, §23, §53, §92 | A08,P2 / T-S02 |
| I19 | sequenceはEAが処理した範囲の連続性だけを検証。MT5配信段階の完全性を証明しない | §20, §64 | A02,P1,P3 / T-T04 |
| I20 | UTC offsetは設定・校正・検証。`TimeCurrent()-TimeGMT()`を恒久値とせず、生時刻を上書きしない | §13.1, §33, §95.2 | A04,P3 / T-C04 |
| I21 | EMAは表示補助。raw Lead/Lag、Raw Tick、イベント条件を置換しない。市場の因果的先行と表現しない | §3.3, §44 | A05,A08 / T-L04 |
| I22 | localhost TCP、Windows x64、MQL5標準Socket、Rust/egui Painter。取引・WebView・汎用チャート・外部配信追加は範囲外 | §4–5, §15–18, §36, §77–78, §93 | 全担当 / T-I01 |
| I23 | CopyRates比較は検証目標。確定足・現行足、OHLC・Tick Volumeを分け、完全一致の成否だけで断定しない | §29, §63, Appendix A.2.2 | P3 / T-C05 |
| I24 | エラー・切断・過負荷はログとGUI statusで観測可能。全経路停止時にも最後の状態を成功扱いしない | §25, §72, §95 | P1,P2 / T-B02,T-H01 |

`reserved != 0`は原仕様§95.2の通りWARNであり、それだけで不正frameとして切断する実装に変えない。

検証IDの手順と期待結果は[validation.md](validation.md)。補足契約の変更時は、影響するI番号と検証IDを必ず示す。
