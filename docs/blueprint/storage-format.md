# 保存契約 L1（Blueprint補足）

主仕様[spec.md](../spec.md) §50–52はbinary追記と保存fieldを要求するが、disk byte配置は規定していない。以下はB/D15としてP0で固定する内部log契約。通信Wire C1とは独立versionとし、A06が単独でwriter/検証readerを実装できるようにする。製品Replay機能の追加を意味しない。

## 1. 所有と論理record

LogRecordは共通型のRawFrame / Metadata / Diagnostic。LoggerはRawFrameの元wire bytesと受信metadataを保存する。Rawの欠落・再送・分析除外を検証できるよう、同じTickIdの再観測も別観測recordとして保存する。原Tickのfloat bit・reserved・broker時刻を変更しない。

RawFrameのTickごとのsequence採否は0=New、1=DuplicateExact、2=IdentityConflict、3=OutOfOrderUnverified。Candle/Midに使えるquoteか、UTCが有効かは後続Diagnosticとconfig_epochで記録する。sequence採否を全分析の実行完了と同一視しない。

## 2. File header（全整数LE）

| offset | byte数 | field |
|---:|---:|---|
| 0 | 4 | literal bytes `54 4C 4F 47` (TLOG) |
| 4 | 2 | log_version:u16 = 1 |
| 6 | 2 | header_length:u16 = 56 |
| 8 | 16 | run_id: 16 opaque bytes、Rust起動単位 |
| 24 | 8 | file_id:u64、同run内一意 |
| 32 | 4 | broker_id:u32、global診断ファイルだけ0 |
| 36 | 4 | flags:u32 = 0 |
| 40 | 8 | created_unix_ns:i64 |
| 48 | 8 | reserved:u64 = 0 |

brokerごとにファイルを分け、日付directoryとrun/file識別子付きの名前でcreate-newする。既存ファイルへ上書きしない。Rust再起動時は新runの新ファイルを作る。日付変更時のrotateは新headerとMetadataから始める。

## 3. Record envelope

| offset | byte数 | field |
|---:|---:|---|
| 0 | 4 | total_length:u32、24 byte headerを含む |
| 4 | 2 | record_kind:u16、1=RawFrame/2=Metadata/3=Diagnostic |
| 6 | 2 | record_flags:u16、kindごとの定義 |
| 8 | 8 | record_index:u64、file内0開始連番 |
| 16 | 4 | crc32c:u32 |
| 20 | 4 | reserved:u32 = 0 |

CRC32Cは反転多項式0x82F63B78、初期0xFFFFFFFF、最終xor0xFFFFFFFF。checksum fieldを0としてenvelope全体＋payloadを対象とする。最大total_length=1,114,112 byte。Lengthのchecked検証後にのみallocateする。

## 4. RawFrame payload

固定部60 byteの後に原wire bytesと採否配列を連結する。

| offset | byte数 | field |
|---:|---:|---|
| 0 | 4 | broker_id:u32 |
| 4 | 8 | connection_generation:u64 |
| 12 | 8 | frame_index:u64 |
| 20 | 8 | rx_mono_ns:u64 |
| 28 | 8 | rx_unix_ns:i64、なければ0 |
| 36 | 8 | config_epoch:u64 |
| 44 | 8 | analysis_segment:u64 |
| 52 | 4 | wire_length:u32 |
| 56 | 4 | disposition_count:u32 |
| 60 | wire_length | 原wire Frameの全byte |
| 60+wire_length | disposition_count | sequence採否u8配列 |

record_flagsのbit0=HAS_RX_UNIX、それ以外0。disposition_countはTickBatchのtick_count、Controlでは0。broker/session/seq/phaseはwire Header/flagとSTATUSに保持される。ファイルheaderのrun_idとmonoを必ず組にする。analysis_segmentは当該run内u64 ID、詳細な切替理由はDiagnosticで参照できる。

## 5. Metadata / Diagnostic payload

Metadata固定部はconfig_epoch:u64、observed_mono_ns:u64、text_length:u32の20 byte、後続は最大65,536 byteのUTF-8 TOML構成文書。record_flags=0。読み込んだ設定、broker/symbol/point/pip、TimeProfileの検証状態・offset/source、分析設定・capacityを含める。元設定から不足するruntime検証結果を追加したものとし、秘密情報は元々設定へ入れない。起動/設定変更時の構成記録であり、Tickのテキストserializeではない。

Diagnostic固定部64 byte:

| offset | byte数 | field |
|---:|---:|---|
| 0 | 8 | mono_ns:u64 |
| 8 | 4 | broker_id:u32 |
| 12 | 2 | severity:u16、1=ERROR/2=WARN/3=INFO/4=DEBUG/5=TRACE |
| 14 | 2 | flags:u16、bit0=HAS_SESSION、bit1=HAS_RANGE、bit2=HAS_COUNT |
| 16 | 8 | session_id:u64 |
| 24 | 8 | sequence_first:u64 |
| 32 | 8 | sequence_last:u64 |
| 40 | 8 | known_count:u64 |
| 48 | 8 | detail_value:i64 |
| 56 | 4 | code_length:u32 |
| 60 | 4 | message_length:u32 |

後続はcode（UTF-8、最大64 byte）とmessage（UTF-8、最大4096 byte）。codeは`SEQUENCE_GAP`等の共通Diagnostic code、textは例外の説明で、通常Tickを文字列化しない。flagsで不在fieldを識別し、不在fieldは0。rangeはinclusive。envelope record_flags=0。

L1では全recordをversion/length/CRC/variant規則で検証する。未定義kind/flags/version、CRC不一致はCorrupt。末尾途中はTruncatedTail。readerが有効prefixを報告できることと、壊れた末尾を正常record扱いすることは別。製品運用で自動truncate/deleteしない。

## 6. 受理・flush・durability・終了

- Accepted: bounded queueが所有しただけ。disk完了ではない。
- Written: buffered writerへ渡った範囲。
- Flushed: ユーザー空間bufferをOSへ渡した範囲。
- Durable: 明示sync要求が成功した範囲。OS/filesystemの保証範囲を超えた電源断保証は主張しない。

定期flush初期1000msはFlushedまで。正常終了ではdrain→flush→syncを試みる。deadline初期5000ms超過・disk fault時はnot-durable/pending範囲をhealth/終了reportで示す。I/O呼出がOSで拘束されている間に強制中断できるとは保証せず、GUI終了操作とworker終了の状態を分離する。

`logging.enabled=false`は明示的な非保存状態であり、Raw dropの許可ではない。UIにLOGGING_OFF、記録の完全性なしを示す。logging=trueの故障を自動でfalseへ落として正常運用扱いしない。

writerとreaderはT-G01/T-G02で検証する。checksum期待値を同じwriter/reader実装だけで相互証明せず、P0がCRC32C既知vector `123456789 → E3069283`と独立fixtureを用意する。
