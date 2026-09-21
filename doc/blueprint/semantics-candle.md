# Candleの意味論 C1

出典: [spec.md](../spec.md) §13,§29–34,§38–40,§63,§95.1–95.2。補足D09,D10,D16,D18。

## 1. 時間軸とSlot

Candleの入力は検証された時刻設定で正規化したTick。Lead/Lagのrx_mono_nsを足の割当てに使わない。raw broker時刻を保持し、別フィールドに`normalized_time_utc_ms`を作る。

B: 実端末でbroker時刻がoffset付きepoch表現であると検証したprofileに対し、`utc_ms = broker_time_msc - broker_utc_offset_sec * 1000`をchecked算術で使う。既にUTCなら検証済みoffset=0。未知のまま数時間を推測して引かない。source/manual値・検証結果・設定epochを保存する。

periodは1,000 / 5,000 / 10,000 / 60,000ms。Slot開始は数学的floorで`floor(utc_ms / period_ms) * period_ms`、区間は`[start,start+period)`。負のepochでも0方向truncateではなくfloor。A/Bは同じ開始時刻のSlotを同じX位置に置く。

例: period=1000でutc=999はSlot0、1000はSlot1000。到着が遅れてもSlotの位置は変わらない。

## 2. OHLCと件数

デフォルトPriceMode=Bid、内部enumはBid/Ask/Mid。選択価格は有効quoteから得る。MQL5 CopyRatesのOHLCを本番Candle生成の入力にしない。

B: Slot内のopen/closeの順序キーは`(normalized_utc_ms, sequence)`。最小keyがopen、最大keyがclose、high/lowは全対象Tickの最大/最小。broker/session/normalization segmentは混ぜない。同一ms同一価格でも異なるseqなら件数を増やす。

Tick_countは「このCandleに寄与した有効な実Tickの出現数」。MqlTick.volumeの和、OnTick回数、価格変化回数ではない。quote無効で除外したTickは別counterで記録する。内部u64（原仕様の概念structのu32は上限保証ではない）でchecked加算する。

Warmupも実TickなのでCandleを生成できる。Warmupが60秒だけならM1×10本の残りを創作しない。部分取得Slotはcoverage=Partialで区別する。

## 3. Empty / Active / Closed

| 条件 | state | OHLC | 備考 |
|---|---|---|---|
| 対象Tickゼロ | Empty | None | 過去でもEmptyのまま。ゼロ価格の足を作らない |
| 対象Tickあり、Slot終端が共通UTC現在より後 | Active | Some | 現在形成中 |
| 対象Tickあり、Slot終端<=共通UTC現在 | Closed | Some | 時間区間が終了した状態 |

Closedは「未来永劫改訂されない」という保証ではない。保持範囲内へ遅着したTickは同じ順序規則でOHLCを再計算し、slot.revisionを増やす。時間順に先行するTickが遅着すればopenも変わり得る。保持範囲外ならraw保存を維持してLATE_OUTSIDE_RETENTIONを記録し、bounded履歴を無限に広げない。

Emptyへ遅着した実Tickが届けばActive/Closedに変わる。Heartbeat、時計timer、前のCloseだけではOHLCを作らない。表示補助線は分析データに含めない。将来の疑似表示も`synthetic_display_only`を別の描画primitiveにし、CandleSlotの実OHLCへ入れない。

## 4. 現在時刻、設定変更、欠損

共通UTC現在は親runtimeから供給する検証されたwall-clock軸であり、Rust monoからbroker時刻を推定することではない。無Tickでもtimerでadvanceし、右端とSlot閉鎖を更新する。巨大な時刻jumpでも表示範囲内の有限Slotだけ生成し、空白の年数分を列挙しない。

PC wall-clockの後退ではClosedをActiveへ無言で戻さない。CLOCK_DISCONTINUITYを出し、新しい表示clock segmentへ切替える（B）。UTC offset/DST設定更新は明示epoch境界とし、過去のrawや既存Candleを黙って再配置しない。保持済みrawからの明示再構築は将来対応、MVPはsegmentを分ける。

正規化未検証ならUTC CandleはUnavailable。raw価格やRust受信時計のLead分析まで誤ってUTCに依存させない。片側UTC未検証なら「共通軸で比較できる」と表示しない。

seq gap、session更新、cursor fault等はcoverage/integrityをSlotへ伝える。時間が同じだから新旧sessionの重複historyをHashで潰すことはしない。segmentを跨ぐ表示には境界を付け、同一分析足として加算しない。

## 5. 保持と公開

初期表示M1×10本。保持本数はS1=60、S5=24、S10=12、M1=10。SlotのA/B共通indexを含むCandleViewをSnapshotへ渡す。各periodの表示は同じUTC右端から作る。raw Tick表示ring60秒とCandle保持10分は別のbudget。

Snapshotはimmutableな履歴chunkを共有し、変更したchunkとActive Slotだけを更新可能。毎Tick全履歴をdeep-copyしない。表示段階でCandleを再集計しない。

## 6. 検証

T-C01: 境界999/1000ms、負のepoch、同一ms別seq、順不同到着で同じOHLC。

T-C02: AだけTick停止でもA/BのX軸は一致、停止SlotはEmpty/None。Heartbeatで件数が増えない。

T-C03: Closedへの遅着修正、保持外raw維持、部分warmup、session/offset切替。

T-C04: 各MT5のoffsetを複数sample、DST、Tick停止、OnTick/OnTimer、PC時刻変更で確認。単一TimeCurrent-TimeGMT値で正規化しない。

T-C05: MT5 CopyRatesと同symbol/価格モード/時間足で、確定OHLC、Tick Volume、現行足を別々に比較。乖離時は時刻→境界→seq/cursor→データソースの順に調べる。
