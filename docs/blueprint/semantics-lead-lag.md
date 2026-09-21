# Lead/Lagの意味論 C1

出典: [spec.md](../spec.md) §3.3,§13–14,§21,§42–44,§95.1–95.3。補足D08,D11–D13。

## 1. 観測対象

Lead/Lagは「本PCで有意な同方向Mid変化を観測したフレーム受信時刻の差」。市場で実際に発生した時刻や因果的先行を意味しない。broker_time_msc、normalized UTC、ea_elapsed_us、GUI描画時刻を差分時計にしない。

全イベントのevent_timeはRust共通runのrx_mono_ns。Frame内Tickは同時刻。Warmupを対象外とし、Liveの新規有効quoteだけを使う。UTC正規化の可否には依存しないが、point_sizeの検証は必要。

## 2. Significant Mid-move detector

状態はbrokerごとにanchorのMid/Bid/Ask/Spread、last_event_time、cooldown_until、latest quote、max excursion、segment。初回有効Live quoteでanchorを置き、イベントは出さない。

初期設定（可変）: trigger_move_points=2、matching_window_ms=100、event_cooldown_ms=20。thresholdの価格単位は`trigger_move_points * broker.point_size`。固定3pointや単なる次Tick一致へ変更しない。

各ordered Tickについて:

1. 現在のquote/最大乖離等は常に更新する。
2. `rx < cooldown_until`なら新規イベントを抑制し、anchorを変更しない。
3. `rx >= cooldown_until`で`abs(mid_now-anchor_mid) >= threshold`なら1件発火。
4. イベントには発火前anchorからのsigned deltaと方向を保存。
5. 発火直後にanchorを現在Mid/Bid/Ask/Spreadへ更新し、cooldown_until=rx+cooldown。

Tickが来ないcooldown終了時刻そのものでは発火しない。終了後最初の有効Tickで判定する。巨大1Tickジャンプでも1件だけ発行し、`mid_delta_points`の実値を残す。複数閾値ぶんの架空イベントへ分解しない。bid/ask/mid/spread deltaは全て発火前anchorのquoteとの差。

数値境界の丸め誤差はP0の価格精度契約に従う。B: 閾値比較のみ、入力価格スケールに対し`8 * f64::EPSILON * max(abs(mid_now),abs(anchor_mid),1)`以下の丸め誤差を境界同値として扱う。これがthresholdの1%を超える設定は精度不足としてrejectする。Raw価格や保存deltaを丸め直さず、意味のある1pointノイズを許容誤差で有意変動へ変えない。

## 3. 品質とノイズ

BID_ONLY / ASK_ONLY / BOTH_SIDES / SPREAD_DRIVEN等は診断分類。C1ではbidだけ非zero=BidOnly、askだけ非zero=AskOnly、両方非zero=BothSides。SpreadDrivenはspread_delta非zeroかつ片側変化、またはbid/ask逆方向の場合の追加flagとする。これは市場原因の推定ではない。

分類だけを理由にイベントを除外しない。Spread拡大がMidを閾値以上動かせば基本方式上イベントになり得る。Noise試験はその件数と未対応率を報告する。「Spread由来は必ずゼロ件」という原仕様にないフィルタを追加しない。

恒常的なA/B価格差は各自anchorとの差分なのでイベント発火に影響しない。Lead判定のための価格水準補正をしない。centered differenceやEMAがRaw判定を変えない。

## 4. 1対1マッチングの確定規則

B: 親Engineがwatermarkで確定したイベントを`(rx_mono_ns,broker_id,trigger_sequence)`順で渡す。Matcherはonline greedyで処理し、到着したeventに対して既存の相手brokerの未対応集合だけを調べる。

候補条件:

- 同じpair analysis segment/run。
- 方向一致。
- `0 < newer_time - older_time <= matching_window`。
- 双方ともまだ未対応。

候補が複数なら時間差最小を1件。tieは相手event_idのsequence最小（必要ならIDの辞書順）で決める。match成立時に双方を未対応集合から除き、再利用しない。未来により近いeventが来ても既存pairを組み替えない。全期間の最適割当て・未来候補待ちを要求する解釈は採用しない（D12の補足）。

候補なしならincomingを未対応集合へ追加。watermarkが`event_time + window`を厳密に越えた時点でexpireできる。窓ちょうどの候補は有効。同時刻A/Bは`delta=0`のためmatchしない。tie-breakで人工的なns差を与えない。

未対応集合は有限。上限超過はANALYSIS_OVERLOADを診断しsegment reset、Rawは保存継続する。隠れて古いeventをdropして正常なmatchingと見せない。

### 例（ms、全て同方向、独立したmatcher fixture）

| 既存未対応 | 新event | 結果 |
|---|---|---|
| A@0, A@8 | B@10 | A@8 ↔ B@10、A@0は未対応 |
| A@8 ↔ B@10が成立済み | B@11 | A@8を再利用しない |
| A@0 | B@100 | window=100なら成立 |
| A@0 | B@101 | 不成立 |
| A@10 | B@10 | delta=0で不成立 |
| A-up@0 | B-down@5 | 不成立 |

上のA@0/A@8はmatcher単体入力であり、cooldown=20msのdetectorがこの2件を作るという例ではない。

## 5. 符号・平滑・リセット

`signed_delta_ns = t_B - t_A`、正ならAがleader、負ならBがleader。`leader`と`abs_delta_ns`を別に持つ。mono u64の直接減算でunderflowせず、checkedな符号付き差へ変換する。

raw_delta_ms=signed_delta_ns/1e6。EMAは同じsigned値に対して`ema = alpha*raw + (1-alpha)*previous`、初回ema=raw、alpha初期0.1。成立matchだけで更新し、未対応event、Heartbeat、描画frameで更新しない。

片側session変化、sequence gap、Warmup→Live、接続断、run変更、重大分析faultでpair segmentを区切る。両者のpending/anchor/cooldown/EMAをresetし、異なる連続性を跨いでpairを作らない。last resultは履歴表示として残せるが「現在」の結果に再利用しない。

## 6. 表示と試験

UIは`Observed Lead: A +3.2 ms`等のPC観測である文言と、別にraw/EMA、sample age、品質・integrityを表示する。B leader時も内部signedは負のまま。表示の正負とleaderが矛盾しないことを試験する。

T-L01: broker/EA/wall時刻を変えてもrxと価格が同じなら結果不変。

T-L02: 固定anchor、連続trend、巨大jump、cooldownちょうど、1point往復、価格offset、Spread変化。

T-L03: 最小差1件、tie、同時刻、100ms境界、reverse方向、同一event再利用禁止、遅れてqueueに届く古いFrameをwatermarkで整列。

T-L04: signed leader、EMA原値維持、Warmup除外、session/gap reset、bounded pending過負荷。
