# TickScope 研究者パネル

TickScopeは、複数のFX Brokerが配信する価格を同時に観測し、その差異、鮮度、更新特性、同時的な価格変化を記録するためのアプリケーションである。売買判断、真の市場価格、Brokerの優劣を断定する機能は扱わない。

この目的に合わせ、実装とレビューを次の5人の研究者ロールで行う。

| 研究者ロール | 専門領域 | TickScopeで確認すること | 成果物の基準 |
|---|---|---|---|
| 市場マイクロ構造研究者 | Quote更新、Spread、価格分散、イベント連鎖 | Broker間の一致と乖離を観測値として扱えているか | Median、Range、Dispersion、Burstを「観測」として表示する |
| 時刻・分散計測研究者 | 単調時計、受信時刻、遅延、順序保証 | 「先行」をサーバー時刻の因果と取り違えていないか | Lead/Lagは「このPC上で先に観測」と明記し、MonoNsで算出する |
| 堅牢統計研究者 | Median、MAD、外れ値、低標本数 | 古いQuoteや少数Brokerで集計が過大解釈されないか | Fresh Quoteだけを集計し、低NのMADは計算不能として扱う |
| 可観測性・信頼性研究者 | 生データ保存、障害状態、再接続、監査可能性 | 分析結果を原データまで遡れるか | Broker別Raw Frame保存、Run識別、状態と鮮度の明示 |
| 情報可視化研究者 | リアルタイム状況把握、説明可能性、認知負荷 | 画面が2社比較へ偏らず、全Brokerの状態を伝えるか | Broker Overviewを主画面にし、Focus Pairは補助分析にする |

## 共通の研究手順

すべての表示と記録は、次の3層を混同しない。

| 層 | 例 |
|---|---|
| Observed | Broker AのMid、Quote age、受信時刻、Raw Frame |
| Derived | Observed Broker Median、Range、Fresh 4/5、Event Cluster |
| Hypothesis | 複数Feedで同時に価格更新が観測された |

HypothesisはRaw FrameやDerived値から追跡可能にし、価格の正誤、将来の方向、売買可能性を断定しない。

## 現在の実装判断

- Raw FrameはUTC日付、Broker ID、Run IDで分離して保存する。Brokerごとの記録を取り出して検証でき、再起動や別Brokerの書込みと混在しない。
- Broker OverviewではBid、Ask、Spread、Quote age、Feed状態、Tick rateを並べ、STALEとDISCONNECTEDを価格差と区別する。
- Observed Broker MedianとRangeはFresh Quoteだけを用いる。Focus Pairの価格差とLead/Lagは補助表示とする。

## 次の評価基準

1. 2、3、5 Brokerで、切断・再接続・STALE・外れ値・Burstの観測値が同じルールで出ること。
2. すべてのDerived値が、該当Brokerの保存Raw Frameと受信時刻から説明できること。
3. 1 Brokerの停止が、他のFresh BrokerのMedian、Range、Burst表示を止めないこと。
4. 受信からSnapshotまでのp50、p95、p99、maxをBroker数別に記録し、変更による尾部遅延を比較できること。
