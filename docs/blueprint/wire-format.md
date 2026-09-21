# Wire Format 実装契約 C1

独立してMQL5 writer / Rust decoderへ渡す文書。主要仕様は[spec.md](../spec.md) §18–21, §80, §95.2。S=原仕様、B=不足を埋めるBlueprint補足（[D01–D05](decisions.md)）。補足の数値を原仕様の確定値と取り違えない。P0がC1として一括凍結し、片側だけ変更しない。

## 1. 共通規則

- S: binary length-prefixed TCP stream。`[40-byte Header][payload_length bytes]`。テキスト通信、JSON/CSV、構造体丸ごと送信は禁止。
- S: 整数は固定幅Little Endian。B: 符号付き整数は2の補数、f64はIEEE 754 binary64のbit列をLittle Endianで送る。
- S: `repr(C)`、`repr(packed)`、transmute、生pointer cast、compiler paddingに依存しない。unaligned fieldもbyte単位で明示読書き。
- S: Header=40 byte、TickRecord=72 byte。B: version=1では他のheader_lengthを受理しない。将来拡張はversion契約更新で行う。
- B: frame payload最大=1,048,576 byte。`40 + payload_length`、`tick_count * 72`、`sequence_start + tick_count - 1`はchecked算術。
- S: 送信とreadの呼出回数は一致しない。short writeを成功完了とせず、同じ接続で未送信suffixだけ送る。

## 2. Header（S: offset/type、B: 値・用途の詳細）

| offset | byte数 | field | type | C1の値／検証 |
|---:|---:|---|---|---|
| 0 | 4 | magic | u32 | `0x5449434B`。wire=`4B 43 49 54` |
| 4 | 2 | protocol_version | u16 | 1 |
| 6 | 2 | message_type | u16 | 下表 |
| 8 | 2 | header_length | u16 | 40 |
| 10 | 2 | header_flags | u16 | message別の許可bitのみ |
| 12 | 4 | broker_id | u32 | 接続portの設定IDと一致 |
| 16 | 8 | session_id | u64 | EA起動単位。0はC1で禁止 |
| 24 | 8 | sequence_start | u64 | TICK_BATCHの先頭seq。それ以外は0 |
| 32 | 4 | tick_count | u32 | TICK_BATCHのみ1以上。それ以外は0 |
| 36 | 4 | payload_length | u32 | typeと件数に一致 |

magicの主仕様コメント`"TICK"`をASCII文字順の指定として解釈しない。数値＋Little Endianを採用した補足判断である。両側とFixtureを同じbyte列に固定する。

| message_type (B) | 方向 | payload byte数 | 許可header_flags |
|---|---|---:|---|
| 1: TICK_BATCH | EA → Rust | tick_count × 72 | 0、またはbit0=WARMUP |
| 2: HEARTBEAT | EA → Rust | 36 | bit1=HAS_LAST_TICK、bit2=HAS_OFFSET_SAMPLE |
| 3: BATCH_ACK | Rust → EA | 8 | 0 |
| 4: STATUS | EA → Rust | 48 | 0 |

未定義type、方向違反、未定義flagはC1ではprotocol error。将来拡張を勝手に読み飛ばさない。TickRecord.reservedのWARN規則とは区別する。

## 3. TickRecord（S）

| offset | byte数 | field | type |
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

S: reservedは送信時0、受信未知値はWARN（§95.2）。MqlTick.flagsのbit列はそのまま保存し、protocol header_flagsと混同しない。

B: TICK_BATCHは連続sequenceだけで構成し、record[i].sequence = sequence_start+i。欠落を含む別範囲は別Frameにする。broker_time_mscが等しいTick、priceが等しいTickも別record。1つのCopyTicks結果の新規分を即送信し、packet上限で分割可能。待ち時間でBatchを貯めない。

B: NaN/Infやbid>ask等のquote異常はstructural framing errorではない。bit列をdecode・保存し、Engineが分析除外と診断を行う。無効quoteを0に変換しない。length/sequence整合性など構造異常はfatal。

## 4. HEARTBEAT（B: offset/type、S: 必須意味）

| offset | byte数 | field | type |
|---:|---:|---|---|
| 0 | 8 | session_id | u64 |
| 8 | 8 | last_sequence | u64 |
| 16 | 8 | last_tick_time_msc | i64 |
| 24 | 4 | server_utc_offset_sec | i32 |
| 28 | 8 | heartbeat_elapsed_us | u64 |

- payload.session_idはHeaderと一致。sequence_start=0、tick_count=0。
- HAS_LAST_TICK=0ならlast_sequence/last_tick_time_mscは0を送るが、意味は「未取得」。seq=0の最初のTickを処理済みならflag=1で区別。
- last_sequenceはEAがsequenceを付与した最後のTickであり、Rust受信済み・disk保存済みの保証ではない。
- HAS_OFFSET_SAMPLE=0ならoffsetを0送信。flag=1でも診断sampleであり、Candle変換係数へ自動採用しない。
- `heartbeat_elapsed_us`はEAプログラム開始からの値。§49.1の`ea_elapsed_us`と同じclock domain。wire名を上表に統一。
- §49.1のoffset_ms表記と矛盾するため、C1では§19.4のsecを採用。ミリ秒を誤ってそのまま格納しない。
- Heartbeat受信はTick count、sequence進行、Candle、Lead/Lag、Tick鮮度を更新しない。

## 5. BATCH_ACK（B）

payload offset0、8 byte、`sequence_end: u64`。Headerは元Batchと同broker/session、sequence_start=0、tick_count=0、flags=0。

ACKは「そのsequence_endを末尾に持つ完全Frameを構造検証し、bounded Raw ingressへ受理した」ことだけを示す。累積ACKでも、disk durability保証でも、MT5配信完全性保証でもない。EAはACKを利用して独自にrawを削除・再送する別の信頼性protocolを追加しない。

`ack_mode=off`は送らない。`diagnostic`は設定された実験区間でon/off比較、`forced`は効果確認後だけ使用。ACKをTick送信の必須待ち条件にしない。EAのReply decoderもpartial/coalesced frameを扱い、OnTimer/OnTickの有限budgetで排出する。

## 6. STATUS（B、原仕様の異常通知/WARMINGを伝える補足）

| offset | byte数 | field | type |
|---:|---:|---|---|
| 0 | 2 | status_code | u16 |
| 2 | 2 | phase | u16 |
| 4 | 4 | detail_flags | u32 |
| 8 | 8 | sequence_first | u64 |
| 16 | 8 | sequence_last | u64 |
| 24 | 8 | affected_count | u64 |
| 32 | 8 | ea_elapsed_us | u64 |
| 40 | 8 | detail_value | i64 |

phase: 1=WARMING、2=LIVE。status_code: 1=PHASE、2=TICK_BACKLOG、3=CURSOR_BLOCKED、4=TRANSPORT_FAULT、5=DATA_LOSS、6=UNCONFIRMED、7=RECOVERY。

detail_flags: bit0=HAS_SEQUENCE_RANGE、bit1=HAS_EXACT_COUNT。他bit禁止。rangeは両端inclusiveでfirst<=last。未知range/countはfield=0かつflag=0、未知件数を損失0件としない。detail_valueはcode別に、PHASE=0、BACKLOG=推定pending Tick数、CURSOR_BLOCKED=境界既処理件数、TRANSPORT_FAULT=EA error番号、DATA_LOSS=0、UNCONFIRMED=未送信byte数、RECOVERY=0。コード不明時はmalformed。

PHASE(LIVE)は最後のWARMUP Batchより後、最初のLive Batchより前に送る。全BatchのWARMUP flagとphaseを整合させる。再接続直後は現phaseを先頭STATUSで通知する。障害通知をその障害で送れない場合はEA診断へ残し、復旧接続で有限の集約通知を送る。送れたことを仮定しない。

## 7. 接続とsession

初期listenは127.0.0.1:39001/39002。portからbrokerを固定し、異なるbroker_idはreject。一つのportにつき現行接続は1本、新接続で既存接続を無言置換せず、新接続を閉じて診断する（B）。

最初のSTATUSでsessionとphaseを確立。sessionは接続中に変えない。EA再起動は新sessionと新TCP接続、TCPだけの再接続ではsession/seqを維持する。Rust再起動でsessionの先頭seq=0を要求しない。

部分write後の接続切断では残suffixを新TCP接続の先頭へ継ぎ足さない。部分Frameの範囲をUNCONFIRMEDとして記録し、C1初期方針では自動再送しない。まだ一切送っていない完全Frameは順序を保ち再接続後に送れる。そのpendingが有限容量を超えた場合はDATA_LOSSを明示する。Exactly-once通信を保証する仕様ではない。

## 8. Decoder状態機械と異常処理

1. 40 byte未満ならNeedMore。EOFなら空bufferは正常、残byteありはTruncatedFrame。
2. Headerを検証。version/header_length/type/flags/broker/lengthのchecked算術を先に行う。
3. 宣言長まで有限bufferで待つ。length由来の無制限reserve禁止。
4. payloadが全て揃ったらtype別decode/検証。WARNとfatalを分離。
5. そのFrameだけを排出し、そのFrame長だけconsume。後続Frameを同様に処理。
6. 完全Frame検証直後にTransportが1回採時。全recordが同じrx_mono_nsを継承。

単なるpartial readは異常ではない。切断、EOF、設定された通信監視条件で初めて未完了として分類する。Header内magic/version/length不正、payload算術不整合、session不整合等はProductionで診断して接続破棄。正常なprefix Frameを、後続不正Frameがあるという理由で巻き戻さない。

Debug/Fuzz限定で最大65,536 byteのresync scanを許す。有効magicだけでなくHeader全検証を通過する必要がある。無制限scanやProduction自動復旧は禁止。

検証責任を重複させないため、Codecはbyte構造・長さ・type/flags・payload内整合性・reserved警告を担当する。Transportは方向・port/broker対応・最初のSTATUS・接続中session一定を担当する。EngineはFrame間のphase/sequence・再送・gapを担当し、重大違反はTransportControlへgeneration付きcloseを要求する。Engineが受理しない分析データでも、観測済みの有効raw Frameは診断とともに保存する。Codec単体のGolden Tick fixtureに接続handshakeがなくても、byte decode試験として有効。

## 9. Golden vectorと適合性

B: C1最小TickBatch fixture（broker=1、session=1、seq=0、Tick数1、全flag=0）のHeader40 byte:

```text
4B 43 49 54  01 00  01 00  28 00  00 00  01 00 00 00
01 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00
01 00 00 00  48 00 00 00
```

続くTick72 byte（time_msc=1000、ea_elapsed_us=10、bid=1.0、ask=2.0、last=0、volume=1、volume_real=1.0、flags=0、reserved=0）:

```text
00 00 00 00 00 00 00 00
E8 03 00 00 00 00 00 00
0A 00 00 00 00 00 00 00
00 00 00 00 00 00 F0 3F
00 00 00 00 00 00 00 40
00 00 00 00 00 00 00 00
01 00 00 00 00 00 00 00
00 00 00 00 00 00 F0 3F
00 00 00 00  00 00 00 00
```

これはwire試験用の値であり、USDJPY価格の例ではない。P0はこの期待byte列を独立したFixtureへ変換し、A01/A02の同じserializerから期待値を生成しない。

追加必須vectors: 負のbroker_time/offset、最初のseq=0を含むHeartbeatと未取得Heartbeat、ACK、全STATUS、非zero reserved(WARN)、NaN原bit保存、長さ/数overflow、同一ms同一内容の複数seq。双方のencoder bytesとdecoder fieldを比較する。全read分割位置、1byteずつ、複数Frame結合で結果不変。詳細はT-W01〜T-W04。
