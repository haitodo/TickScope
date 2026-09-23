# TickScope Low-Latency Market Observatory

## Multi-Broker / Microstructure / Feed-Behavior Analysis

### RFC Beta 0.3

---

# 1. このRFCの目的

TickScopeは、売買シグナル生成システムではない。

発注システムでもない。

自動売買システムでもない。

TickScopeの役割は、

> **複数のFX Brokerから取得したリアルタイムQuoteを高速かつ正確に比較し、市場状態、Broker間の差異、Feed品質、Quote更新特性、観測上の先行・遅行、流動性状態の変化をユーザーが判断できるようにする観測システム**

である。

最終的なトレード判断は常にユーザーが行う。

---

# 2. 最上位原則

## O1. No Trading Decision

TickScopeは以下を生成しない。

* BUY
* SELL
* ENTRY
* EXIT
* LONG
* SHORT
* Trade Now
* Buy Confidence
* Sell Confidence
* Entry Score
* Signal Strength

また、これらと実質的に同等の意味を持つ単一スコアをUIへ表示しない。

---

## O2. Observation First

画面上の情報は次の3段階を明確に区別する。

### Observed

実際に受信・観測した事実。

例：

```text
Broker A Mid +2.0pt
Broker B Mid +1.8pt
Broker C Spread +1.5pt
```

### Derived

複数のObserved値から数学的に算出した値。

例：

```text
Median Mid
Mid Range
Breadth
Observed Lead/Lag
MAD
Spread Expansion
```

### Hypothesis

複数の観測から推察される可能性。

例：

```text
Possible Feed Filtering
Possible Quote Aggregation
Possible Delayed Repricing
Possible Internalisation Effect
```

Hypothesisは必ず「仮説」であることを明示する。

---

# 3. なぜこの分離が必要か

FXは単一の中央取引所ではなく、複数のdealer、ECN、single-dealer platform、PTF等が存在する分散・断片化された市場である。さらにdealerによるinternalisationも大きい。

したがって、

```text
Broker Aが先にQuote更新
```

から、

```text
LP Aが市場を動かした
```

とは判断できない。

同様に、

```text
Broker Aが長期間先にQuoteを更新
```

から、

```text
Broker Aは必ず市場価格形成に優位
```

とも言えない。

TickScopeでは、これらを統計的特徴として蓄積し、ユーザーが判断できる形にする。

---

# 4. システムの役割

TickScopeは以下を提供する。

## 4.1 Market State Observation

現在の市場状態。

## 4.2 Broker Feed Comparison

Broker間のQuote差。

## 4.3 Feed Quality Measurement

Freshness、staleness、update rate、spread、outlier等。

## 4.4 Observed Timing Analysis

PC上での受信順序とObserved Lead/Lag。

## 4.5 Quote Behavior Analysis

Quote更新のパターン。

## 4.6 Historical Behaviour

時間帯・市場状態別のBroker Feed特性。

---

# 5. Brokerの「優位性」の定義を変更する

単一の「Broker Score」は作らない。

Broker Feedの評価軸を分離する。

```text
Responsiveness
Freshness
Spread Stability
Quote Stability
Consensus Alignment
Repricing Behaviour
Outlier Frequency
Stale Frequency
Observed Lead Frequency
Observed Follow Frequency
```

つまり、

```text
Broker A = 83点
Broker B = 74点
```

のような総合順位にはしない。

代わりに、

```text
A
Responsiveness       High
Freshness            High
Spread Stability     Medium
Outlier Frequency    Low
Observed Lead        43%
Observed Follow      31%
```

とする。

これにより「優位性」を単純なランキングへ潰さない。

---

# 6. Execution Qualityとの区別

Quote Feed品質と実際のExecution Qualityは別物。

TickScopeが約定情報を持っていない場合、

```text
良いBroker
```

とは断定しない。

観測できるのは、

> Quote Feed / Market Data Behaviour

まで。

真のExecution Qualityには、

* fill price
* slippage
* rejection
* requote
* execution latency
* last-look behaviour
* order response

など追加データが必要。

将来Execution Feedを統合する場合は別Subsystemとして設計する。

---

# 7. Broker Model

Brokerは固定A/BではなくN Broker。

```text
BrokerState[BrokerId]
```

を基本とする。

各Broker:

```text
connection
session
generation

latest_bid
latest_ask
latest_mid
latest_spread

last_rx_mono_ns
last_mid_change_mono_ns

tick_rate
quote_update_rate
mid_change_rate

freshness
heartbeat
integrity

sequence_gap
overload
```

---

# 8. Pairwise Analysis

N Brokerを内部ではPairwise比較可能とする。

5 Brokerの場合：

```text
A-B
A-C
A-D
A-E
B-C
B-D
B-E
C-D
C-E
D-E
```

計10 Pair。

内部ではReference Brokerを必要としない。

---

# 9. Reference Broker

ReferenceはUI上の表示基準としてだけ存在してよい。

```text
reference_broker_id = A
```

ただし、

> Referenceだから市場の中心

とは扱わない。

Reference変更によって、

```text
内部Lead/Lag
Consensus
Broker Behaviour
Burst
```

の意味が変わってはならない。

---

# 10. Consensus

N Brokerの現在値を比較するため、

```text
consensus_mid
```

を計算する。

基本はFresh BrokerのMedian。

MeanよりMedianを基本候補とする。

ただしConsensusは、

> 「市場の真値」

ではない。

UI上は、

```text
Observed Broker Median
```

または

```text
Broker Median
```

という表現を使用する。

---

# 11. Dispersion

Consensusだけでは情報が足りない。

以下を保持する。

```text
mid_median
mid_min
mid_max
mid_range

bid_range
ask_range

median_abs_deviation
```

例えば、

```text
Median: 147.325
Range : 0.6 pt
```

ならBroker間の一致度が分かる。

---

# 12. Outlier

Broker単独の異常Quoteを検出する。

ただし「異常」と即断しない。

表示は、

```text
Deviation from Broker Median
```

を基本とする。

例：

```text
A +0.2pt
B +0.1pt
C +0.3pt
D +4.8pt
```

ならDは、

```text
Large Deviation
```

として表示する。

「間違い」「不正」「悪いBroker」とは表示しない。

---

# 13. Crossed Snapshot

複数Brokerで、

```text
Broker A Bid > Broker B Ask
```

などが観測された場合、

```text
Crossed Snapshot
```

として記録できる。

ただしこれは、

> Arbitrage Opportunity

とは表示しない。

Quoteの時刻差、staleness、latency、execution condition等を考慮しない単純比較だからである。

---

# 14. Freshness

各Brokerについて、

```text
Age = current_mono - last_live_quote_rx
```

を計算する。

例えば、

```text
A 8ms
B 12ms
C 741ms
D 13ms
```

ならCはFresh Broker集合から除外可能。

Consensus / Breadth / Burstでは、

```text
Fresh Count
```

を必ず保持する。

例：

```text
Fresh: 3/4
```

---

# 15. Stale Dataを「静止」と解釈しない

最重要Invariant。

Broker Cが300ms更新していない場合、

```text
C unchanged
```

とは言わない。

```text
C stale
```

とする。

これは、

> 「Cでは価格が動いていない」

とは意味しない。

---

# 16. Raw Quote Path

すべてのQuoteは可能な限り、

```text
receive
→ normalize
→ broker state
→ analysis
→ projection
```

を通る。

描画用のresamplingやdownsamplingを、

Raw分析より前に適用しない。

---

# 17. Fast Path / Analysis Path 分離

今回の最重要アーキテクチャ変更。

## Fast Path

```text
TCP
↓
Decode
↓
Receive timestamp
↓
Broker state
↓
minimal derived state
↓
publish
```

この経路は最小限の処理にする。

---

## Analysis Path

```text
Fast Path state
↓
Pairwise analysis
↓
Event clustering
↓
Historical statistics
↓
Behaviour inference
```

重い処理はFast Pathをブロックしない。

---

# 18. 「分析を追加したらFeed表示が遅くなる」を禁止

例えば、

```text
Historical statistics
LP behaviour inference
MAD
Burst clustering
Long-term statistics
```

などを追加しても、

```text
Tick arrival
→ Current Quote display
```

の経路に遅延を追加してはいけない。

---

# 19. Application-added latency

TickScope自身の遅延を測定する。

最低限、

```text
rx_timestamp
engine_timestamp
projection_timestamp
snapshot_timestamp
render_observation_timestamp
```

を記録可能にする。

そして、

```text
Tick → Engine
Engine → Projection
Projection → Snapshot
Snapshot → UI
```

のp50/p95/p99を測定する。

---

# 20. UIはLatency Budgetの対象

UIが綺麗でも、

```text
CPU 100%
frame queue
GC
allocations
lock contention
```

でFeed観測が遅れれば失敗。

したがって、

> UI性能 = 見た目のFPS

だけでは評価しない。

本当の評価は、

> **Tick受信からユーザーが観測可能になるまでの追加遅延**

とする。

---

# 21. Hot Path Allocation禁止

Tick処理中に、

```text
Vec allocation
String formatting
large clone
full-history copy
global sort
```

を極力発生させない。

現在の履歴Seriesを毎回Vecへコピーする方式などは再検討対象。

履歴は、

```text
ring buffer
immutable chunk
Arc-backed segment
```

等を使用可能とする。

---

# 22. RenderingとAnalysisを分離

UI Painterは、

```text
Snapshot
→ Draw
```

のみ。

以下は禁止。

```text
Draw callback
→ calculate Lead/Lag
→ calculate Consensus
→ access Raw Tick
→ disk I/O
```

---

# 23. Main Chart

M1 CandlestickをMain Chartの中心としない。

スキャルピング用途では、

> **Realtime Quote Path**

をMain Chartの中心とする。

---

# 24. Main Chart構成

推奨：

```text
Broker A Mid ─────────
Broker B Mid ─────────
Broker C Mid ─────────
Broker D Mid ─────────
Broker Median ────────
```

これに、

```text
Event Marker
Burst Marker
Spread Marker
```

を重ねる。

---

# 25. M1 Candlestick

M1 Candlestickは、

> Context View

として別領域またはtoggle表示。

M1 Candleを中心にしてしまうと、TickScopeが普通のFXチャートへ逆戻りする。

---

# 26. Relative Microstructure View

別表示モードとして、

```text
Broker Mid - Broker Median
```

を表示する。

例えば、

```text
A +0.2pt
B +0.1pt
C -0.1pt
D +3.4pt
```

のように表示する。

これはMarket Directionではなく、

> Broker dispersion

を見るためのView。

---

# 27. Y-Axis

デフォルトをMin-Max Auto Scaleにしない。

推奨：

```text
Fixed Follow Scale
```

---

# 28. Fixed Follow Scale

例：

```text
Span = 5 pips
```

なら常に約5pipの価格幅を表示する。

これにより、

```text
0.5 pip
```

と

```text
5 pip
```

の大きさが視覚的に同じ倍率へ固定されない。

---

# 29. Follow Hysteresis

最新価格を毎tick中央に置かない。

例えば、

```text
上部20%
下部20%
```

をDead Zoneにする。

その領域を超えた時だけY軸を再配置する。

これにより画面の揺れを減らす。

---

# 30. Scale変更自体もLatency対象

Scale計算に時間を使い過ぎない。

最新Quote表示は、

```text
Scale Calculation
```

に依存してはいけない。

つまり、

```text
new tick
↓
price immediately update
```

が先。

scale recalculationは後。

---

# 31. Auto Scale

Auto Scaleは残してよい。

しかし、

```text
Diagnostic / Shape Inspection
```

用とする。

通常モードではFixed Follow。

---

# 32. Scale表示

常時、

```text
Y-Span: 5.0 pip
Grid: 1.0 pip
```

を表示。

ユーザーがチャートの見た目だけでボラティリティを推測する必要をなくす。

---

# 33. Move Event

通常のTickと、

```text
Significant Move Event
```

を分ける。

Significant MoveはUI上の補助イベント。

Raw Tickの意味を変更してはいけない。

---

# 34. Market MoveとQuote Geometry

現在のMoveQualityを拡張し、

```text
TwoSideDirectional
BidOnly
AskOnly

SpreadExpansion
SpreadCompression

OppositeSideMove
MixedQuote
```

などのQuote Geometryを保持する。

---

# 35. 「Market Move」と断定しすぎない

例えば、

```text
Bid +2
Ask unchanged
```

なら、

```text
BidOnly Quote Change
```

と表示する。

「Market Buy」などとは表示しない。

同様に、

```text
Bid +3
Ask -1
```

なら、

```text
Mixed Quote Geometry
```

とする。

---

# 36. Observed Lead/Lag

Lead/Lagは、

```text
Observed Lead/Lag
```

という名称を使う。

意味：

> 同一PC上で観測されたQuote change eventの受信時刻差。

意味しないもの：

```text
True Market Causality
True LP Origin
True Exchange Timestamp
Guaranteed Execution Advantage
```

---

# 37. Time Domain

時間を4層に分ける。

```text
Broker Time
UTC
Rust Monotonic Receive Time
UI Display Time
```

Lead/Lagの主要時計：

```text
Rust Monotonic Receive Time
```

UTCは、

```text
session alignment
logging
replay
cross-day analysis
```

に使用する。

---

# 38. Source Timestampを捨てない

各Tickで可能なら、

```text
broker_timestamp
UTC_timestamp
EA_elapsed
rx_mono_ns
```

を分離保持。

どれがLead/Lagの時計なのかを明示する。

---

# 39. Pairwise Matcher

N Broker間のPairwise Matcherは維持する。

ただし、

```text
Pairwise Matcher
```

を市場全体の結論装置にしない。

---

# 40. Multi-Broker Burst

Pairwise Matcherの上位に、

```text
Event Cluster / Burst
```

を追加する。

例：

```text
A UP @ 0ms
B UP @ 4ms
C UP @ 7ms
D UP @ 10ms
```

↓

```text
UP Burst
Fresh: 4/4
Duration: 10ms
First observed: A
Last observed: D
```

とする。

---

# 41. BurstはSignalではない

Burst表示は禁止：

```text
BUY
SELL
ENTRY
MOMENTUM SIGNAL
```

表示例：

```text
UP Move Cluster
4/4 Fresh
10ms observed span
```

まで。

---

# 42. Breadth

方向を持つ情報として、

```text
UP: 4/4
DOWN: 1/4
```

などを計算可能とする。

ただし、

> UP 4/4 = Buy signal

とはしない。

UI上もSignal風のScore化をしない。

---

# 43. Broker Behaviour Fingerprint

長期・中期統計として、

```text
Observed Lead Frequency
Observed Follow Frequency
Median Delay
Spread Expansion Frequency
Stale Frequency
Outlier Frequency
Consensus Deviation
Repricing Persistence
Post-update Convergence
```

を保持。

これはBroker Feedの「特徴」を測定するためのもの。

---

# 44. Repricing Persistence

あるBrokerが他Brokerより先に価格を変えたあと、

```text
others follow
```

したのか、

```text
broker reverts
```

したのかを測定する。

重要なのは、

> 「先行した」

だけでなく、

> 「先行後、その価格がどの程度維持されたか」

を見ること。

---

# 45. Quote Persistence

Brokerごとに、

```text
quote duration
```

を測定。

例えば、

```text
new quote
→ 4ms
→ changed
```

と、

```text
new quote
→ 600ms
→ unchanged
```

ではFeed特性が異なる。

---

# 46. Spread Behaviour

Brokerごとに、

```text
median spread
p95 spread
spread expansion frequency
spread recovery time
spread instability
```

を測定する。

単純なCurrent Spreadだけでは不十分。

---

# 47. Staleness Behaviour

Brokerごとに、

```text
stale event count
stale duration
stale recovery time
```

を保持する。

これにより、

> 「通常このBrokerはFeed更新が遅いのか？」

を過去データから確認できる。

---

# 48. Feed Filtering Hypothesis

あるBrokerが、

```text
他Brokerで多数発生
↓
Broker Aには小さな変化しか来ない
```

という状態を繰り返す場合、

可能な仮説として、

```text
Possible Quote Filtering
Possible Aggregation
Possible Price Smoothing
Possible Feed-specific Policy
```

を表示可能とする。

ただし必ず、

```text
Hypothesis
```

と表示する。

---

# 49. LP推察

TickScopeはLP IDを知らない状態では、

> LP behavior

を直接表示してはいけない。

代わりに、

```text
Observed Feed Behaviour
```

として表示する。

例えば、

```text
Fast Repricing
Sticky Quote
Spread-first Response
Outlier Quote
Delayed Convergence
```

など。

---

# 50. LP推察のEvidence Chain

Hypothesisの下に、

```text
Evidence
```

を表示可能にする。

例：

```text
Possible Delayed Repricing

Evidence:
- 37 comparable events
- median observed lag 8.2ms
- 31/37 same direction
- fresh rate 99.2%
- low spread distortion
```

これにより「AIが勝手に判断した」という構造を避ける。

---

# 51. Confidence Scoreを作らない

以下は禁止：

```text
LP Confidence 92%
Broker Advantage 87%
Market Confidence 94%
```

こうした数値は実際の観測以上の確実性をユーザーへ与えやすい。

代わりに、

```text
Sample: 37
Fresh: 99.2%
Median lag: 8.2ms
Dispersion: 0.4pt
```

のようにEvidenceを表示する。

---

# 52. Statistical Sample Context

統計値には必ず、

```text
Sample Count
Observation Window
Freshness
```

を付与。

例えば、

```text
Median Lag: 3.2ms
n=7
window=10s
```

と、

```text
Median Lag: 3.2ms
n=14,820
window=24h
```

を同じ表示にしない。

---

# 53. Regime Awareness

Feed behaviourは市場環境によって変わる可能性がある。

したがって、

```text
Normal
High Volatility
News
Thin Liquidity
Session Transition
```

などのContextを、

必要に応じて後から分析軸へ追加できる構造にする。

ただしTickScope自身が「現在はニュース相場だから売買すべき」と判断してはいけない。

---

# 54. Human Factors

UIは、

> User's attentionを奪うより、User's situation awarenessを補助する

ことを目的とする。

---

# 55. Alert

基本はPassive UI。

以下のような大量の音・Popupはデフォルトで行わない。

```text
Broker A moved
Broker B moved
Spread changed
Lead changed
Breadth changed
```

こうした通知を大量に出すと、重要な状態とノイズの区別が難しくなる。

---

# 56. Event Marker

イベントはChart上の小さなMarkerへ集約する。

例えば、

```text
▲ A
▲ B
▲ C
```

のようにする。

---

# 57. One-Line State Ribbon

画面に一行だけ、

```text
Fresh 4/4 | UP Cluster 3/4 | Spread Stable | Dispersion 0.5pt
```

のようなState Summaryを置く。

これはSignalではない。

各項目をクリックすると元データを見ることができる。

---

# 58. No Hidden Decision

State Summaryをクリックすると、

```text
何を根拠にそう表示しているか
```

を確認できる。

例：

```text
UP Cluster 3/4

A +2.1pt
B +1.9pt
C +2.0pt
D stale
Window: 100ms
Threshold: 2pt
```

---

# 59. UIの情報階層

最上部：

```text
Symbol
Current Price
Spread
Freshness
Scale
```

中央：

```text
Realtime Broker Price Chart
```

直下：

```text
State Ribbon
```

下段：

```text
Microstructure View
```

さらに下：

```text
M1 Context
```

Debug：

```text
Latency / Queue / Sequence / Watermark
```

---

# 60. Main Chart

推奨構成：

```text
┌──────────────────────────────────────────────┐
│ USDJPY     Y-Span 5.0 pip      Fresh 4/4     │
├──────────────────────────────────────────────┤
│ A 147.325  147.327  0.2pip  Age 8ms         │
│ B 147.325  147.327  0.2pip  Age 10ms        │
│ C 147.326  147.328  0.2pip  Age 9ms         │
│ D 147.325  147.327  0.2pip  Age 12ms        │
├──────────────────────────────────────────────┤
│                                              │
│             REALTIME PRICE PATH              │
│                                              │
│    A ─────────────────────────               │
│    B ─────────────────────────               │
│    C ─────────────────────────               │
│    D ─────────────────────────               │
│    Median ─────────────────────              │
│                                              │
├──────────────────────────────────────────────┤
│ Fresh 4/4 | UP Cluster 3/4 | Range 0.5pt     │
├──────────────────────────────────────────────┤
│ 100ms | 250ms | 500ms | 1s | Spread | Lag    │
│              MICROSTRUCTURE                  │
└──────────────────────────────────────────────┘
```

---

# 61. Microstructure View

切替可能：

```text
Mid Dispersion
Bid/Ask Dispersion
Spread
Observed Lead/Lag
Move Breadth
Quote Persistence
```

ただし一度に表示するのは原則1～2種類。

---

# 62. 低遅延設計上の禁止事項

以下はHot Pathへの導入を禁止する。

```text
Heavy smoothing
Machine learning inference
Large historical query
Full chart autoscaling
Cross-broker global sort
Database query
Disk I/O
Network lookup
Complex JSON serialization
```

必要なら別workerで実行。

---

# 63. Smoothing

Current Quoteに平滑化を適用しない。

例えば、

```text
EMA
moving average
Kalman
```

などは分析Viewだけ。

Main Quote表示はRaw / latest observation。

---

# 64. Downsampling

画面へ描画する点数を削減することは許可。

ただし、

> Display Downsampling

と

> Analysis Data

を完全に分離する。

DownsampleされたデータをLead/Lag計算へ戻さない。

---

# 65. Clock

Hot Pathの経過時間計測はMonotonic clock。

```text
Instant / monotonic timestamp
```

を使用。

UTCはHuman-readable time / logging用。

---

# 66. Latency Dashboard

Debug UIに、

```text
Tick → Engine
p50
p95
p99

Engine → Projection
p50
p95
p99

Projection → Snapshot
p50
p95
p99
```

を表示。

さらに、

```text
Queue depth
Dropped snapshot count
Allocation statistics
CPU load
```

も測定可能にする。

---

# 67. Latency Regression Test

新しい機能を追加するたび、

```text
3 Broker
5 Broker
high tick rate
```

で、

```text
p95
p99
max
```

を比較する。

機能追加前よりtail latencyが悪化した場合、

> feature complete

とはしない。

---

# 68. Performance Acceptance Criteria

具体的なµs値はPC/Feed環境依存なので、最初から絶対値を固定しない。

代わりに、

```text
Baseline
```

を測定し、

```text
New build
```

との増分を見る。

Acceptanceの中心は、

> **Tick arrival → observable current state の追加遅延を最小化し、feature追加によるtail latency regressionを検出できること**

とする。

---

# 69. Statistical Review

Consensusには少なくとも、

```text
Median
Range
MAD / robust dispersion
Fresh count
```

を持たせる。

---

# 70. Low-N問題

3 Brokerしかない場合など、

```text
MAD = 0
```

などが起こり得る。

したがって、

```text
N
```

に応じて使用する統計量を変える。

3～5 Brokerでは、

```text
Median
Range
Pairwise deviation
```

を基本とし、複雑な分布推定を無理に行わない。

---

# 71. Statistical Inferenceの制約

TickScopeの解析は、

```text
Observation
```

と、

```text
Association
```

までは扱う。

しかし、

```text
Causality
```

は通常主張しない。

---

# 72. 例

観測：

```text
Broker A changed first
Broker B followed 3ms later
Broker C followed 4ms later
```

表示：

```text
Observed order:
A → B → C

Observed median lag:
A→B 3ms
A→C 4ms
```

表示してよい。

---

しかし、

```text
A caused B and C
```

とは表示しない。

---

# 73. Broker Behaviour Hypothesis

長期データから、

```text
A tends to reprice earlier
```

というstatistical descriptionは可能。

ただし、

```text
A predicts the market
```

とは表示しない。

---

# 74. 「優位性」の適切な意味

TickScopeでいうBrokerの優位性は、

> **特定の観測目的に対して有用なFeed特性**

と定義する。

例：

```text
low freshness delay
stable spread
fast quote update
low stale frequency
high agreement with other feeds
```

これらは別々のdimension。

---

# 75. Broker Profile

Broker Profile画面を設けてもよい。

```text
Broker A

Freshness
p50 8ms
p95 21ms

Spread
median 0.7pip
p95 1.4pip

Stale
0.12%

Observed Lead
43%

Observed Follow
21%

Outlier
1.7%
```

これを、

```text
Overall Score
```

にまとめない。

---

# 76. Market-State Conditional Profile

さらに、

```text
Normal
High Volatility
News
Low Activity
```

などに分けてProfileを保存できる。

同じBrokerでも状況によって特徴が変化する可能性を残す。

---

# 77. LP Behaviour Inference

Brokerの背後に複数LPがいる場合、

```text
Broker Feed ≠ LP single stream
```

である可能性がある。

したがってTickScopeは、

```text
LP #1
LP #2
LP #3
```

のような架空の識別を行わない。

---

# 78. 推察カテゴリ

将来の分析では、

```text
Possible Aggregated Feed
Possible Sticky Pricing
Possible Filtering
Possible Delayed Repricing
Possible Spread-first Adjustment
Possible Volatility-dependent Widening
```

などの仮説を作れる。

各仮説にはEvidenceを付与する。

---

# 79. Hypothesis UI

例えば、

```text
Possible Sticky Pricing

Evidence
n = 2,481 comparable moves
median quote persistence = 38ms
follow frequency = 72%
reversion frequency = 4%
```

のようにする。

---

# 80. Hypothesisに「確率」を付けない

```text
87% Sticky LP
```

などは表示しない。

観測数と測定値を表示して、ユーザーが判断する。

---

# 81. Regression Test

機能追加による市場状態判断の変質をテストする。

### Test A

1社だけQuote jump。

期待：

```text
Broker Deviation
```

であり、

```text
Market-wide Move
```

と自動解釈しない。

---

### Test B

5社ほぼ同時。

期待：

```text
High Breadth
```

ただしSignal化しない。

---

### Test C

1社Stale。

期待：

```text
Fresh 4/5
```

---

### Test D

Spreadだけ拡大。

期待：

```text
Spread Expansion
```

---

### Test E

Askだけ変化。

期待：

```text
Ask-only Quote Change
```

---

### Test F

Bid/Ask両方同方向。

期待：

```text
Two-sided directional quote change
```

---

# 82. Latency Regression Test

同じReplay Datasetで、

```text
Version N
Version N+1
```

を比較。

測定：

```text
Input rate
p50
p95
p99
max engine latency
queue depth
CPU
allocations
snapshot age
```

---

# 83. UI Regression

同じ市場データを使い、

```text
Fixed Scale
Auto Scale
Reference Change
Broker count
```

を変えても、

> Raw measurement result

が変わらないこと。

UI設定は分析結果へ逆流させない。

---

# 84. Replay-first Architecture

可能な限り、

```text
Live Feed
Replay Feed
```

が同じEngineを通る構造にする。

これにより、

```text
市場データ
→ 同じ分析
→ 同じ結果
```

を再現可能にする。

---

# 85. Reproducibility

各分析結果には、

```text
RunId
AnalysisSegmentId
Broker configuration
Threshold configuration
Clock policy
```

を紐付ける。

これにより、

> なぜこの表示になったのか

を再現可能にする。

---

# 86. 最上位UIポリシー

UIは、

```text
Fact
↓
Measurement
↓
Context
```

の順。

```text
Recommendation
↓
Action
```

は行わない。

---

# 87. 色設計

色を、

```text
green = buy
red = sell
```

のような意味に固定しない。

色はBroker識別・状態識別に使う。

状態：

```text
Live
Stale
Disconnected
```

も、色だけで判断させない。

必ず文字を併記する。

---

# 88. Flashing

頻繁な点滅は禁止。

Tickごとの色変更も原則避ける。

重要状態だけ短時間の弱い視覚変化を許可する。

---

# 89. Sound

基本Off。

このアプリは通知装置ではなく観測装置。

---

# 90. 最終アーキテクチャ

```text
                    ┌──────── Broker A
                    ├──────── Broker B
                    ├──────── Broker C
MT5/Feed ───────────┼──────── Broker D
                    └──────── Broker E
                             │
                             ▼
                    ┌─────────────────┐
                    │ Fast Ingestion  │
                    └─────────────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │ Broker State    │
                    └─────────────────┘
                             │
                 ┌───────────┴────────────┐
                 ▼                        ▼
         Pairwise Analysis        Multi-Broker Analysis
                 │                        │
         Lead/Lag / Diff          Consensus / Breadth
         Spread / Quote           Burst / Dispersion
                 │                        │
                 └───────────┬────────────┘
                             ▼
                    ┌─────────────────┐
                    │ Projection      │
                    └─────────────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │ Snapshot        │
                    └─────────────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │ egui UI         │
                    └─────────────────┘
```

InferenceやHistorical AnalysisはこのFast Pathから分離する。

---

# 91. Core Invariants

### I1

Tick受信時刻とUI描画時刻を混同しない。

### I2

Lead/LagはObserved Lead/Lag。

### I3

Staleを「価格停止」と解釈しない。

### I4

Reference Brokerは市場真値ではない。

### I5

ConsensusはMarket Truthではない。

### I6

Broker Feed差からLP identityを推定しない。

### I7

HypothesisとObservationをUI上で区別する。

### I8

Current Quoteは平滑化しない。

### I9

Display DownsamplingはAnalysisへ逆流しない。

### I10

UI設定はAnalysis結果を変更しない。

### I11

分析追加によってFast Path latencyを悪化させない。

### I12

単一ScoreでBrokerを順位付けしない。

### I13

Signal / Recommendationを生成しない。

### I14

Raw observationはDerived resultと別に保持する。

### I15

分析の不確実性を隠さない。

---

# 92. 実装Priority

## Phase 0 — Measurement Foundation

最優先。

```text
Latency instrumentation
Clock separation
Freshness
Fast-path profiling
Allocation profiling
```

---

## Phase 1 — Multi Broker Core

```text
N Broker
Pairwise engine
Broker table
Fixed Follow scale
Realtime quote path
```

---

## Phase 2 — Microstructure

```text
Consensus
Dispersion
Breadth
Quote Geometry
Observed Lead/Lag
Burst
```

---

## Phase 3 — Behaviour Analysis

```text
Persistence
Repricing characteristics
Spread behaviour
Stale statistics
Observed lead frequency
Feed fingerprint
```

---

## Phase 4 — Hypothesis Layer

```text
Possible Filtering
Possible Aggregation
Possible Delayed Repricing
Possible Sticky Pricing
```

すべてEvidence-based。

---

# 93. 実装上の最重要評価基準

機能数ではない。

評価順序は、

```text
1. Correctness
2. Measurement validity
3. Added latency
4. Tail latency
5. Cognitive load
6. Observability
7. UI aesthetics
```

とする。

---

# 94. 最終的な成功条件

TickScopeが成功した状態とは、

> ユーザーがチャートを見て「上がったから買い」という単純な判断をすること

ではない。

むしろ、

```text
「Aだけ動いたのか？」
「複数Feedで一致しているのか？」
「Spreadはどうなっている？」
「CはStaleではないか？」
「このLead/Lagは今回だけか？」
「過去にもこのBrokerは同じ挙動をしているか？」
「これは市場全体の変化なのか、Quote更新の癖なのか？」
「この観測は何ms遅れて表示されているのか？」
```

をユーザー自身が高速に確認できること。

---

# 95. 最終コンセプト

TickScopeは、

> **「何を売買すべきか」を教えるアプリではない。**

TickScopeは、

> **「現在の市場とBroker Feedで何が起きているように観測されているか」を、余計な遅延や自動判断を加えず、人間が高速に理解できるようにするアプリ**

である。

最も重要なのは、

```text
Fast
Accurate
Observable
Neutral
Reproducible
```

である。

ユーザーの判断を置き換えるのではなく、

> **ユーザーが自分の判断を行うための観測能力を最大化する**

ことを最終目標とする。
