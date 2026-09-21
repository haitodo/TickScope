# Snapshotの意味論 C1

出典: [spec.md](../spec.md) §4.3,§24–25,§34–41,§47–54,§69,§74。補足D16–D19,D21。

## 1. 境界

SnapshotはGUIが描くためのimmutableな最新状態。Raw Tick配送、取引履歴、監査ログ、Candle生成入力、Lead/Lagイベントqueueではない。中間Snapshotが表示されず置換されても正常。Raw Tickまで同じdrop規則にしない。

Engineは全新規Tickを処理し、描画レートに独立したProjectionを作る。Publisherはrepaint_hz上限（初期60Hz、目標60〜120Hz）でSnapshotを公開する。1000tick/sを1000Snapshot/sにしない。UIはディスプレイ更新に合わせて最新1個をloadする。

## 2. 一貫性と鮮度

1つのSnapshotは1つのEngineProjection revisionからA/B価格、Candle、差分、Lead/Lagを取得する。A価格だけ新revision、差分だけ旧revisionという混在をしない。これらはas-of観測なので、A/Bの元Tick時刻そのものが同時であるとは限らない。

snapshot_revision、projection_revision、built_mono_ns、processed_watermark、display_now_utc、各値のsource time/ageを持つ。新しいSnapshotが公開されても、停止した分析結果の年齢を0に戻さない。

独立health mailboxから新しいtransport/logger faultをoverlayできる。その場合health_observed_mono_nsを持ち、価格のsource revisionは維持する。EngineがLogger待ちでも、UIはOVERLOAD/LOGGER_FAULTと古い価格を表示できる。

## 3. 直交状態

| 軸 | 値の例 | 根拠 |
|---|---|---|
| connection | Connecting / Connected / Disconnected | Socket lifecycle |
| phase | Warming / Live | EA STATUSと受理境界 |
| data | Unknown / Live / Stale | 最後のLive Tickのrx age |
| heartbeat | Unknown / Ok / Timeout | 最後のHeartbeatのrx age |
| overload | receiver / engine / logger / analysis flags | 各queue・workerの状態 |
| integrity | CompleteObservedRange / PrefixUnobserved / Gap / Unconfirmed / DataLoss | sequence・EA診断・保存結果 |
| normalization | Unverified / Verified / Discontinuity | UTC設定・検証epoch |

`CONNECTED + DATA_STALE + HEARTBEAT_OK`を1つのDisconnected enumへ潰さない。最初のHeartbeatだけで価格LIVEにしない。値が未取得なら`—`等、0で埋めない。断線後の最終価格を表示する場合はSTALEとageを付ける。

## 4. 有限メモリと共有

第一選択ArcSwapによるatomic Arc交換。GUIは描画frame中同一Arcを保持し、次frameへ無制限に蓄積しない。publisherは最新1個、UIは現行1個、履歴は固定本数・期間・最大件数。古い参照のretentionを性能試験に含める。

Candle・line系列はimmutable Arc chunks。毎公開で全Tickや全履歴をdeep-copyしない。変化した末尾chunkや必要viewだけを生成する。UIの長いmutex保持、Raw大配列の直接参照は禁止。

Tick lineや差分波形の描画点削減は表示だけの操作。分析Tick・ログ・Candle件数へ逆流させない。最大画面点数を超えた場合は同pixel区間のmin/max等の表示集約を使えるが、時間・価格の意味を変える補間値を分析へ戻さない。

## 5. チャートと表示範囲

初期はM1×10、A/B共通UTC右端・共通Slot位置。最初の60秒しか実データがなければ他のSlotはEmpty。UIはEmptyに足を描かず、任意の薄い前Close基準線を分析から分離する。

右端時刻更新、age更新、status更新は無Tick時にも進む。Tick line/差分60秒の表示保持とM1 Candle10分の保持を混同しない。時間軸やCandleはEngine/Projectionの意味論を使い、UIで時刻正規化をやり直さない。

UIはBid/Ask/Mid/Spread、Bid/Ask/Mid/Spread差、Lead/Lag、Tick rate、接続状態を表示。価格はdigits、points/pipsはSymbolMeta由来。STALE側を含む差分にはageと品質を付ける。通常非表示のDebug overlayでqueue depth、seq gap、p50/p95/p99、Snapshot age等を表示できる。

ズーム・パン・自由スクロール・インジケータ・発注・broker切替はMVP範囲外。価格計算やLogger I/Oを描画callbackへ持ち込まない。

## 6. 適合性

T-S01: 2000tick/s相当でSnapshotは設定Hz以下、UI pause中は最新値置換、Raw count不変、resume時は最新を表示。

T-S02: 1描画のrevision一貫性、未取得・WARMING・STALE+Heartbeat OK・断線・OVERLOAD、Empty Slot、符号、原値/EMAを表示。最小化・移動・終了が応答する。

T-S03: 毎公開のallocation/copy量、長時間Arc保持数、表示ring上限。Full history deep-copyと無制限snapshot queueがないことを確認。
