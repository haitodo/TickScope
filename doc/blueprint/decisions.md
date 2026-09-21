# 原仕様の曖昧さ・矛盾・不足と補足判断

唯一の主要仕様は[spec.md](../spec.md)。この表は原本を修正せず、並列実装者が異なる解釈を実装しないための差分台帳である。Bは今回選んだ設計上の補足であり、実測済みの事実ではない。P0で全担当に同一revisionを配布する。未検証Vを単体試験で代替したことにはしない。

| ID | 出典・問題 | 本Blueprintの判断／残る確認 | 担当・影響 |
|---|---|---|---|
| D01 | §19.1のmagicは「例」、version・message_type数値・flags・payload上限が未定 | B: magic数値`0x5449434B`、LE bytes `4B 43 49 54`、version=1。ASCII `TICK`のbytesとは異なる。type/flags/limitsはwire契約に列挙 | P0,A01,A02,A03 |
| D02 | §19.4は`server_utc_offset_sec`/`heartbeat_elapsed_us`、§49.1は`server_utc_offset_ms`/`ea_elapsed_us`。型・offset・未取得値未定 | B: wireはsec(i32)とheartbeat_elapsed_us(u64)、36 byte payload。存在flagsで未取得を区別。診断値のまま保持 | P0,A01,A02,P1 |
| D03 | §18.1 ACKの境界・受信確認と永続化確認の違いが未定 | B: ACK payload=sequence_end u64、完全frame検証＋Raw ingress受理までの確認。永続化保証・再送指示ではない。初期off | P0,A02,A03,A06 |
| D04 | §12,§71 WARMINGをRustへ伝える手段、history/live境界なし | B: WARMUP flagとSTATUS frameを追加定義。historyはCandle/履歴差分のみ、Live Lead/Lag・鮮度・rate対象外。LIVE開始は明示境界 | P0,A02,P1,A05 |
| D05 | §20 session生成、再接続、Rust再起動、重複/逆行seqの処理未定 | B: EA起動epochごとsession、TCP再接続では維持。ID=(broker,session,sequence)。同じIDの同じpayloadだけ再送重複として診断除外。衝突はfault。Rust再起動は観測runを分割し以前の完全性を推測しない | P0,A02,P1,A06 |
| D06 | §10.3 同一ms既処理件数がcount以上だと、固定count再取得で永久に同じprefixを読む | B: 要求countを既処理境界件数+新規回収枠へ有限拡張。上限で新規位置を証明できなければCURSOR_BLOCKED、gap不確定を報告し停止・診断。`time+1`は禁止。V: 実端末の同一ms順序安定性・回収可能限界 | A02,P3 |
| D07 | §10.3 次OnTick継続では最後のburst後に残留可能。初回CopyTicks最大45秒とHeartbeat timeoutの関係も未定 | B: Timerで有限のbacklog継続を許可するがHeartbeat自体をTickにしない。同期中はWARMINGで長処理を明示、Heartbeat timeoutを隠さない。V: MT5同期blocking時間 | A02,P1,P3 |
| D08 | §14,§21 フレーム受信/decode時刻の厳密地点、2Receiverの統合順未定 | B: 完全frame検証直後・bounded送出待ち前で共通Clockを採時。frame内同時刻。Receiver watermarkで遅れた送出を含めて順序統合、ingest順を時刻へ代用しない | A03,P1 |
| D09 | §31 Closedの確定時刻、遅着/逆行Tick、同一時刻OHLC順序未定 | B: UTCの半開Slot、OHLC順序=(UTC,sequence)、保持中のClosedは遅着でrevision更新可。保持外Tickはraw保存＋診断。EmptyにOHLCなし | A04,P1 |
| D10 | §13,§33 time_mscの実際のepoch/TZが未検証。DST・設定更新境界なし | B: 検証済みoffsetで`raw-offset*1000`、設定epochを付ける。未検証はUTC分析不可を表示。offset変更は分析segmentを分割し過去を黙って再配置しない。V: 実データ確認はP3の必須gate | A04,P0,P3 |
| D11 | §42 trigger=2候補、§55 minimum_move_points=1例。point/pip/digitsの出所なし | B: 正式key=`trigger_move_points`、初期2、cooldown20ms、window100ms。旧keyは設定errorで説明。broker別point_size/pip_size/digitsを設定、実端末照合。3point固定禁止 | P0,A05,P3 |
| D12 | §43 最小差が全未来候補に対する最小か、現在候補間か不明。同時刻/tie/符号/逆順到着未定 | B: watermark確定した受信時刻順でonline greedy、現在未対応集合から最小差1件。後続候補で再マッチしない。同時刻は対象外。signed=t_B-t_A、leader別項目 | A05,P1 |
| D13 | §42.3 delta基準、Spread拡大の誤検知試験とMid方式の関係が不明 | B: 発火前anchorのBid/Ask/Spreadをdelta基準とする。品質は診断のみ。Mid閾値超過のSpread由来イベントを無条件除外する要求はない。Noise試験で件数を報告 | A05,P3 |
| D14 | §24–25,§52 図でLogger分岐位置が揺れる。Disk停止時の無損失・有限メモリ・継続分析は同時保証不能 | B: Engineがraw保存要求を先に受理させて解析。Logger満杯でRaw全体をbackpressure、UIは独立応答。Disk障害は明示fault、永続化成功を偽らない。期限超過終了は未永続化を報告 | P1,A06,P2 |
| D15 | §50–51 binary logにversion/schema、run epoch、normalized時刻、frame境界、破損回復定義なし | B: P0が本Blueprintの内部log schemaを固定。Wireとは別version。原Tick＋受信clock/run/segment/config/診断を保存。検証readerのみ、Replay製品機能はMVP外 | P0,A06 |
| D16 | §34 60秒程度のringとM1×10本、§55 visible_seconds=60が整合しない | B: Tick/差分系列60秒、Candleは60×S1/24×S5/12×S10/10×M1。初期表示M1×10、ウォームアップ60秒なので不足9分はEmpty。表示を埋めるため履歴を捏造しない | P0,A04,A07,A08 |
| D17 | §53–54 Snapshot型・publish時刻・timer・履歴コピー・UI停止時のメモリ未定 | B: 共通型、ArcSwap、60Hz初期、bounded immutable chunk、UIは現行Arc1個のみ保持。無Tickでもstatus/右端UTCを更新 | A07,A08,P1 |
| D18 | §27–28,§75,§88 Spread差必須が計算節で明記不足。欠損価格・stale差分・無効値未定 | B: spread_diff=spread_A-spread_B追加。差分は各社最新有効quoteのas-of、age/sessionを付ける。NaN/Inf/不正quoteはraw維持、分析除外＋診断。Tick volumeは受理した実Tick数 | A05,A04,P1 |
| D19 | §47–49 状態列挙が直交性を型で表せていない。市場接続とEA/TCPの混同のおそれ | B: connection/data/heartbeat/warmup/overload/integrityの独立フィールド。未受信はUnknown、HeartbeatだけでLIVEにしない | P1,A08 |
| D20 | §66,§89,§95.1 送信→受信の一方向遅延を異なるEA/Rust epochから算出できない。性能閾値・試験時間も未定 | B: EA内時間、Rust内時間、ACK RTTを別測定。共通時計校正なしに一方向値を名乗らない。V: p50/p95/p99と時計誤差・条件を報告、forced ACKは改善後のみ | P3 |
| D21 | §72 fault通知自体が満杯Raw queueに詰まる可能性、終了drain/障害時の保持量が未定 | B: bounded独立health mailboxで最新状態＋累積counter保持。shutdown期限・pending上限・ログflush方針をP0設定schemaで固定。rawと診断の欠落を区別 | P0,P1,P2 |
| D22 | §22 境界横断型とmodule所有者、設定default・snapshot表示命令等の契約がない | B: 親所有`src/contracts/`とPort注入、親だけが結線。子は共通型・依存manifestを変更しない。UI操作はMVP既存設定内、製品Replay/取引/CSVは追加しない | 全担当 |

## ゲートの分類

- P0で確定できるもの: 型、列挙値、単位、初期設定、Port、マッチング手順、所有パス、Fixture期待値。上表と各契約文書の値を初期採用し、未定のまま担当へ渡さない。
- 実装中に検証するもの: bounded backpressure、短いwrite、same-ms回収アルゴリズム、遅着Candle、matching、snapshot一貫性。
- P3まで残るV: 各broker時刻基準、point/pipの実値、MT5同一ms順序・同期挙動、実Socket timeout、TCP/ACK効果、CopyRates照合、数時間〜数日の安定性。

Vで設計の成立条件が満たされなければ、その事実と影響を記録する。例えばsame-msブロックを走査上限内で回収できない場合は対応負荷範囲を狭めるか主仕様改訂が必要であり、Tickを飛ばして「成功」とする解決は認めない。
