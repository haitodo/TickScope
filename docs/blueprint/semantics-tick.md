# Tickの意味論 C1

出典: [spec.md](../spec.md) §8–14,§17–21,§24–28,§47–52,§64–68。補足D04–D08,D14,D18–D21。

## 1. 何を1件と数えるか

Tickは`CopyTicks(COPY_TICKS_ALL)`が返す1つの記録の1つの出現である。同じbroker_time_msc、同じbid/ask/last/volume/flagsでも、正当な別出現なら別Tick。OnTick呼出、Heartbeat、TCP read、Snapshot公開の回数と一致しない。

EAが新規Tickを受理した時点でsequenceを1回だけ付与する。IDは(broker_id,session_id,sequence)、初期sequence=0。配信履歴境界の再読込には再付与しない。受信再送重複の判別はこのIDとpayload一致に限り、内容hashだけで異なるsequenceを潰さない。

`raw observation count`、`unique received tick count`、`valid quote tick count`、`significant event count`、`OnTick callback count`を別counterにする。

## 2. カーソルと有限回収

カーソルは`last_time_msc + そのms内で受理した出現数 + 境界block検証情報`。再読込時は、既処理prefixの内容・順序・重複出現数を照合し、そのprefixだけskipする。正当な同内容の新しい出現は送る。

`copy_batch_count=256`でも同一ms内既処理件数が256以上なら、`from=同じms,count=256`の繰返しでは進まない。B: explicit request countを「既処理境界件数+新規枠」へ拡張し、最大65536に制限する。回収の仕事量・1イベントの新規処理量も別に制限し、バッファは再利用する。返却配列の未処理分を保持する場合も有限pending予算に含める。

boundary prefixが一致しない、履歴同期で順序が変わる、同一ms全blockが走査上限を超える等で新規位置を証明できなければ、CURSOR_BLOCKED/TICK_BACKLOGと不完全性を報告する。境界を飛ばさない。count上限到達だけを根拠に欠落件数を断定しない。

cursorを進めるのは新規Tickを有限pendingに受理した後。pending不足時は原則回収を止め、回収済みTickを保持できず失う場合は付与済みsequence範囲とDATA_LOSSを記録する。sequenceは巻き戻さない。Cursor進行・seq付与・pending受理の一貫性をA02が所有する。

OnTickは有限catch-up、OnTimerはHeartbeat・Reply drain・再接続・残backlogの有限catch-upを行える。Heartbeatを新規Tick扱いしない。CopyTicks/Socket APIが1回でblockする時間は処理budgetから強制中断できないため、budget値を絶対応答保証とせず、超過を計測・報告する。

## 3. WarmupとLive

初期60秒の履歴はWARMUP flagで送り、raw保存・Candle再構成・履歴差分表示に使う。Warmupの古い記録を「今PCで先行したイベント」としてLead/Lagへ投入しない。live Tick rateやlast_live_tick_ageも更新しない。

EAはwarmup回収時点で固定した終端cursorまでを履歴とし、その後の出現をLiveとする。境界は同一msの出現数まで含め、二重送信・境界欠落を防ぐ。last WARMUP → PHASE(LIVE) → first Liveの順序を守る。履歴の初回同期待ちはWARMINGとして扱う。

両brokerのwarmupデータをEA時計やRust受信順で「歴史的Lead/Lag」にしない。履歴差分はnormalized UTC順でas-of結合し、同時刻tieをbroker/seqで決定、coverageと欠損を付ける。tick差分波形はwarmup/live境界を明示して結合し、受信mono系列へ過去時刻を偽造しない。

## 4. 受信時刻と解析順

`rx_mono_ns`は完全Frameの検証完了直後、Raw queue待ち前に共通Rust Instant originから採る。frame内全Tickで同値。ea_elapsed_usやbroker_time_mscから、Frame内の偽の個別受信nsを補間しない。

Engineは[architecture.md](architecture.md)のwatermarkでFrameを順序確定する。1つのbrokerではsession内sequence順、broker間は受信mono順。Candleはこの処理順に依存せずnormalized UTCを使用する。壁時計変更・UTC offset変更は受信monoを変更しない。

## 5. sequenceの整合性

| 入力 | 処理 |
|---|---|
| 新sessionで最初のseq=0 | 通常開始 |
| 観測開始・Rust再起動で最初のseq>0 | `PREFIX_UNOBSERVED`。以前の損失件数を推測しない |
| 同sessionでexpected seq | raw保存・新規分析 |
| 同sessionでexpectedより大 | 欠落範囲を記録、integrity低下、新しい分析segment。新規Tick自体は保存可能 |
| 既観測IDでpayload完全一致 | 再送重複を診断・raw observation保存、二重分析しない |
| 既観測IDでpayload不一致 | ID_COLLISION、fault、分析停止 |
| 未知の過去seq/照合履歴保持外 | `OUT_OF_ORDER_UNVERIFIED`、raw保存、分析除外。推測で新規扱いしない |

重複照合ledgerは有限。原Tickが同内容でもseqが異なれば双方分析対象。Receiver間の到着順をbroker内欠番と混同しない。TCP接続generationとEA sessionは別。

session/gap/phase/UTC設定変更ではbrokerごとのcontinuityを区切る。Lead/Lagのpair segmentはどちらかが切れたら両者の未対応集合とanchor/EMAをresetする。Closed Candleは過去segmentとして保持可能、同じSlotに履歴と新sessionが重なる場合は別segmentのcoverageとして示し、重複を時間hashで推測除去しない。

## 6. 有効値、保存、派生値

Raw Tickは原bitで保存。bid/askが非有限、0以下、ask<bidなら有効quoteではなく、Candle/価格差/Leadイベントには使用しない。last=0/volume=0はFXであり得るため、それだけでquoteを無効にしない。各分析除外は診断する。

有効quoteで`mid=(bid+ask)/2`、`spread=ask-bid`。overflow等で派生値が非有限なら分析不可。価格丸めは表示時のみ。spread/min/maxは指定された表示window内、差分はA-Bの符号。

Live差分は各brokerの最新有効quoteのas-of値。片側未取得ならNone。STALE側を含む値は最後の価格とageを添えて表示し、同時刻の価格と断定しない。Spread差も計算する。points/pipsはbroker別metadataに基づき表示し、異なるpoint_sizeのA/Bでは共通価格単位の差を基準にする。

## 7. 鮮度と生存

`last_live_tick_rx_mono_ns`、`last_heartbeat_rx_mono_ns`、`tcp_connection_state`は独立。初期未取得はUnknown。Tick age>=stale_after_msでSTALE、Heartbeat age>=timeoutでTIMEOUT。Heartbeatだけ受信してもTick ageは戻らない。受信中の過負荷でも過去Tickが新着として処理された時点に時計を付け直さない。

TCP切断はDISCONNECTED。Heartbeat timeoutはEA・通信・イベント処理の不調の可能性であり、確実な切断や市場停止とは表示しない。WARMING中でもHeartbeat timeoutという観測事実は保持し、原因候補として同期中を添える。

## 8. Rawの保証範囲

通常時はRawをdrop/coalesceしない。capacity fullでは所有権を返し、上流が保持・待機・backpressureする。GUIのframe落ちやSnapshot置換をRaw損失と混同しない。

断線・有限pending超過・disk障害ではDATA_LOSS、UNCONFIRMED、NOT_DURABLE等で保証範囲を明記する。known countとunknownを分ける。ACK受信、SocketSend完了、Logger queue受理、buffer flush、durable syncは別の到達点であり、1つで全てを保証したとしない。

適合試験: T-T01〜T-T05、T-H01、T-B01/T-B02。
