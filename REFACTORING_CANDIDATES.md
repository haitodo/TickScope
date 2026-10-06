# TickScope リファクタリング候補（簡単 × 大量 × 修正方針が明確）

- 調査対象: コミット `7687480`（作業ツリーはクリーン）
- 使用ツール: cargo / rustc 1.98.1、`cargo clippy --features replay`、`cargo fmt --check`
- この文書は LLM エージェントへの作業指示としてそのまま渡せる形で書いている。
- 件数はすべて実測値。検証コマンドは本ドキュメント作成時に実行済み。

---

## 0. 前提（現状の把握）

- `cargo clippy`（デフォルト lint）の本番コード警告は **13 件のみ**。既存コードはほぼ lint クリーン。
  → 探すべきは「壊れている箇所」ではなく **「方針を決めて機械的に一括適用できる箇所」**。
- `-W clippy::pedantic -W clippy::unwrap_used -W clippy::expect_used` を足すと
  **本番 1,033 件 / 全ターゲット 2,643 件**。
- うち **本番 601 件 / 全ターゲット 1,246 件は Clippy が `MachineApplicable`（機械適用可能）とマーク**しており、
  `cargo clippy --fix` の 1 コマンドで消せる。
- ベースラインの `cargo test --features replay` は緑（回帰判定の基準として使える）。

### ★重要（対応済み）: リファクタリングではなく「実バグ」が 1 件見つかっていた

`src/config/timezone.rs:102` の `days_to_ymd` は **Hinnant の civil-date アルゴリズムの定数が間違っている**。

```rust
// src/config/timezone.rs:106（誤り・修正前）
let yoe = (doe - doe / 1024 + doe / 1461 - doe / 36524) / 365;
// 正しい式（現在は src/core/civil_date.rs:17 が唯一の実装）
let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
```

- `doe / 1024` → `doe / 1460`、`doe / 1461` → `doe / 146096` が正しい。
- 実測（1900〜2200 の 109,573 日を検証）: **月が誤る日が 6,974 日**、うち 1,384 日は年も誤る。
- 生成される値が不正な日付になる（例: 2026-02-28 → `2026-3-0`、2026-02-26 → `2026-3--2`）。
  毎年 2 月下旬の 2〜5 日が該当する。
- **`is_us_dst` の判定結果は変わらない**（全 6,974 日の誤りで DST の月バケットは反転せず、
  唯一月が 3→2 にずれる 2003-03-01 も両経路とも `false`）。したがって**現在の運用には影響していない**が、
  `pub fn days_to_ymd` / `pub fn unix_sec_to_ymd` は公開 API として明確に誤り。
- 既存テスト `test_calendar_roundtrip`（:155-167）は 1970-01-01 と 2026-09-30 の 2 点しか見ておらず、
  しかも `ymd_to_days`（正しい実装）が誤った出力をたまたま逆変換できてしまうため**素通りする**。

**対応状況**: 修正済み。暦計算は現在 `src/core/civil_date.rs` に集約され、`civil_from_days` を含む
暦ヘルパー（`ymd_to_days` / `day_of_week` / `nth_sunday_of_month` / `unix_sec_to_ymd` /
`ymd_hms_to_unix_sec`）が**1 実装だけ**になっている。
`config::timezone` / `storage::tlog_writer` / `ui::chart` / `logging` はすべてそこへ委譲し、
`config/timezone.rs` は 242 行 → 120 行の DST ルール専用モジュールになった。
リグレッションテストは 4 層（`core::civil_date` の既知日付・暦の連続性、`config::timezone` の DST 遷移、
`ui::chart::candlestick` のタイムスタンプ整形、`logging` の 2026-02-28 整形）にあり、
共通実装を一時的に壊すと **4 層すべてが落ちる**ことを確認済み。
`cargo test` 197 passed / `cargo test --features replay` 220 passed / clippy 警告 13 件（いずれも変化なし）。

**残タスク**: なし。暦変換の 4 コピー（`logging.rs` / `config/timezone.rs` /
`storage/tlog_writer.rs` / `ui/chart/candlestick.rs`）は `core::civil_date` に統合済み（§8 参照）。
§3 のバイト列デコード共通化も対応済みで、本番コードの `unwrap()` が 68 件減っている。

### 推奨する実施順序

| # | 作業 | 規模 | 章 | 単独コミット必須 |
| :-- | :--- | :-- | :-- | :-- |
| 0 | ~~**`days_to_ymd` のバグ修正**~~（**対応済み**・下記 §0 参照） | 1 関数（定数 2 箇所） | §0 | ○ |
| 1 | ~~`cargo clippy --fix` 一括自動修正~~（**対応済み**） | 73 ファイル / 685 箇所 | §1 | ○ |
| 2 | テストフィクスチャの共通化 | 80 箇所以上 | §4 | ○ |
| 3 | ~~バイト列デコードのヘルパー化~~（**対応済み**） | 68 箇所 | §3 | ○ |
| 4 | UI 色リテラルの定数化 | 84 箇所 | §5 | ○ |
| 5 | ~~`std::sync` → `parking_lot` 統一~~（**対応済み**） | 14 箇所 | §6 | ○ |
| 6 | 未使用の公開関数 26 件の整理 | 26 箇所 | §7 | ○ |
| 7 | `cargo fmt` 一括適用 | 77 ファイル / 492 ハンク | §2 | ○（必ず最後） |

理由: 1〜6 は意味を変えない機械的変更なので、**最後に一度だけ `cargo fmt` をかける**と差分が 1 回で済む。
各章は前の章に依存しないので、並行・別担当でもよい。

---

## 1. ★最有力: `cargo clippy --fix` による一括自動修正（本番 601 / 全体 1,246 箇所）★対応済み

**対応状況**: 完了。**73 ファイル / +732 −827 行**、警告は 1,444 件 → **763 件**、
機械適用可能な修正は **685 件 → 0 件**。ついでにデフォルト Clippy の警告も **13 件 → 5 件**に減った。

実行したコマンド（除外理由は下記）:

```powershell
cargo clippy --fix --all-targets --features replay --allow-dirty --allow-staged `
  -- -W clippy::pedantic -A clippy::must_use_candidate -A clippy::return_self_not_must_use `
     -A clippy::manual_midpoint -A clippy::wildcard_imports
```

除外した 4 つと理由:

| 除外した lint | 理由 |
| :--- | :--- |
| `must_use_candidate` (240) / `return_self_not_must_use` (64) | `#[must_use]` を付けると戻り値を捨てている呼び出し側に `unused_must_use` が新規発生しうる。別コミットで扱うのが安全 |
| `manual_midpoint` (36) | `f64::midpoint` は Rust 1.85+。MSRV の決定が必要（付録 B） |
| `wildcard_imports` (35) | ~~glob import の展開は可読性を下げる場合がある。方針判断が必要~~ → **対応済み**（35 → 0）。`--fix` で展開したが、**テストコードでのみ使う名前が展開から漏れて 3 ファイルがコンパイルエラーになった**ため、`#[cfg(test)] mod tests` の中に個別 import を補った（モジュール先頭に置くと非テスト時に未使用警告になる） |

**残っているデフォルト Clippy 警告 5 件**（いずれも自動修正不可・要判断）:

| lint | 場所 | 内容 |
| :--- | :--- | :--- |
| `if_same_then_else` | `src/tick/engine.rs:832` | `if` の両ブロックが同一（片方が書き間違いの可能性） |
| `missing_transmute_annotations` | `src/ui/dpi.rs:178, :203` | `transmute` に型注釈がない |
| `too_many_arguments` | `src/replay/driver.rs:200`, `src/replay/rebuilder.rs:20` | 引数 9 個 |

以下は実施時に使った調査内容（記録として残す）。

### 修正方針

「pedantic レベルの lint を有効化し、Clippy が機械適用可能と判定した suggestion をそのまま適用する」。
個別判断は不要で、**コマンド 1 回 + テストで完了**する。

| lint | 本番 | 全体 | 修正内容 | リスク |
| :--- | ---: | ---: | :--- | :--- |
| `clippy::must_use_candidate` | 240 | 480 | 値を返す関数に `#[must_use]` を付与 | ⚠ 呼び出し側に `unused_must_use` が新規発生しうる（下記注意） |
| `clippy::uninlined_format_args` | 108 | 238 | `format!("{}", x)` → `format!("{x}")` | なし |
| `clippy::doc_markdown` | 54 | 128 | doc コメント内の識別子をバッククォートで囲む | なし |
| `clippy::map_unwrap_or` | 50 | 101 | `.map(f).unwrap_or(d)` → `.map_or(d, f)` | なし |
| `clippy::wildcard_imports` | 35 | 35 | `use foo::*;` を使う名前の明示列挙に展開 | なし（差分は大きい） |
| `clippy::cast_lossless` | 27 | 73 | `x as i64` → `i64::from(x)`（無損失と証明済みの箇所のみ） | なし |
| `clippy::redundant_closure_for_method_calls` | 15 | 30 | `.map(\|x\| x.f())` → `.map(T::f)` | なし |
| `clippy::ignored_unit_patterns` | 14 | 28 | `\|_\|` → `\|()\|` | なし |
| `clippy::manual_midpoint` | 15 | 36 | `(a + b) / 2.0` → `a.midpoint(b)` | ⚠ **Rust 1.85+ 必須**（付録 B 参照） |
| `clippy::borrow_as_ptr` | 10 | 20 | `&x as *mut T` → `&raw mut x` | ⚠ Rust 1.82+（既に 1.82 API 使用済みなので実質問題なし） |

上記以外の小口（`single_match_else` 7、`if_not_else` 6、`bool_to_int_with_if` 3、`redundant_else` 3、
`collapsible_if` 2、`unchecked_time_subtraction` 2 ほか）も同時に消える。

### 手順

```powershell
# 0) 事前にコミットしておく（--fix はソースを直接書き換える）
git add -A; git commit -m "chore: snapshot before clippy --fix"

# 1) 自動修正（内部的に最大 4 パスまで再適用される）
cargo clippy --fix --all-targets --features replay --allow-dirty --allow-staged `
  -- -W clippy::pedantic -W clippy::unwrap_used -W clippy::expect_used

# 2) 残件確認（機械適用できない lint だけが残る）
cargo clippy --all-targets --features replay `
  -- -W clippy::pedantic -W clippy::unwrap_used -W clippy::expect_used

# 3) 回帰確認
cargo test --features replay
```

### 検証（完了条件）

1. `cargo test --features replay` が緑のまま。
2. `cargo clippy --all-targets --features replay -- -W clippy::pedantic` の警告数が減っている
   （機械適用不可の `unwrap_used` / `cast_*` / `too_many_lines` などは残る。§9 参照）。
3. 最後に `cargo fmt --all -- --check` を通す（§2）。

### 注意

- **`must_use_candidate` が唯一の要注意項目**。240 箇所に `#[must_use]` を足すと、戻り値を捨てている呼び出し側で
  `unused_must_use` が新規に出る可能性がある。出た場合は
  (a) 呼び出し側を直す、または (b) `must_use_candidate` だけ外す（`-A clippy::must_use_candidate`）。
  他の 9 割は無風なので、**先に `-A clippy::must_use_candidate` で適用し、must_use は別コミットで扱う**のが安全。
- `manual_midpoint` は `f64::midpoint`（Rust 1.85+）を使う。MSRV を 1.80 のまま維持したい場合は
  `clippy.toml` に `msrv = "1.80"` を書くか、この lint だけ除外する。
- `wildcard_imports` は 1 ファイルあたり十数行の import 展開になる。
  「元の glob に無い名前を書き足す」ミスが起きやすいので、差分は 1 ファイルずつ確認する。

---

## 2. `cargo fmt` の一括適用（77 ファイル / 492 ハンク）

### 修正方針

`rustfmt.toml` は存在せず、コードは rustfmt 未適用。既定設定で `cargo fmt --all` をかける。

### 手順・検証

```powershell
cargo fmt --all                                  # 適用
cargo fmt --all -- --check                       # 検証（無出力・exit 0 が完了条件）
cargo test --features replay
```

### 注意

- 実測: 既定設定で **492 ハンク / 77 ファイル**（`.rs` は全 111 ファイル）。
- `max_width = 120` にすると逆に差分が増える（558 ハンク）、`140` でも 577 ハンク。
  → **既定のままが最小差分**。設定ファイルは追加しない方がよい。
- 差分が巨大なので **他の変更と絶対に同じコミットに混ぜない**。
- 適用は必ず最後（§1・§3〜§7 の後）に行う。

---

## 3. バイト列デコードのヘルパー化（68 箇所 → 0）★対応済み

**対応状況**: 完了。`src/protocol/bytes.rs` に `le_u16` / `le_u32` / `le_u64` / `le_i32` /
`le_i64` / `le_f64` を追加し、`packet.rs` 34・`tlog_reader.rs` 32・`codec.rs` 1・`router.rs` 1 の
計 68 箇所を置換した（置換前に全 68 箇所のスライス幅が型幅と一致することを機械的に検証）。
`Vec<u8>` / 固定長配列を渡す箇所は参照 (`&`) が必要な点にだけ注意。
本番コードの `unwrap()` は 68 件減り、`src/` に残る `try_into().unwrap()` は
`src/metrics/latency.rs:228` の 1 件（配列変換であり対象外）のみになった。

以下は実施時に使った調査内容（記録として残す）。

### 修正方針

`u16::from_le_bytes(buf[a..b].try_into().unwrap())` を全廃し、
`src/protocol/bytes.rs`（新規）に置いた読み出しヘルパーへ置き換える。

```rust
#[inline]
#[must_use]
pub fn le_u32(buf: &[u8], offset: usize) -> u32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&buf[offset..offset + 4]);
    u32::from_le_bytes(b)
}
// le_u16 / le_u64 / le_i64 / le_f64 も同様に用意する
```

### 対象（実測 68 箇所）

| ファイル | 箇所数 | 直前の長さチェック |
| :--- | ---: | :--- |
| `src/protocol/packet.rs` | 34 | 10 箇所は関数内で実施、**24 箇所は呼び出し元依存** |
| `src/storage/tlog_reader.rs` | 32 | 32 箇所すべて関数内で実施 |
| `src/protocol/codec.rs` | 1 | あり（`if i + 4 <= self.buffer_len()`） |
| `src/transport/router.rs` | 1 | あり（`read_exact` で固定長配列へ） |
| **合計** | **68** | 44 箇所がローカルに防御、24 箇所は呼び出し元不変条件のみ |

`src/metrics/latency.rs:228` の `try_into().unwrap()` は boxed slice → 配列の変換であり、
バイト列デコードではない。**この章の対象外**（検証コマンドをディレクトリ限定にする理由）。

### なぜ安全か

- 68 箇所中 **44 箇所は同じ関数内で長さチェック済み**（`payload.len() < N` や固定長配列への `read_exact`）。panic は到達不能。
- 残り **24 箇所は `packet.rs` の `decode_tick_record`(:179) / `decode_heartbeat`(:230) /
  `decode_batch_ack`(:254) / `decode_status`(:263) の本体**で、関数内に長さチェックが無い。
  長さは `decode_header` が `payload_length == tick_count * TICK_RECORD_LENGTH` 等で保証し、
  呼び出し元 `codec.rs:107/116/120/124` が過不足のないスライスを渡している（他の呼び出し元は存在しない）。
  → 到達不能であることは変わらない。
- **ヘルパーは panic するバージョンのままにする**（`buf[offset..offset + N]` + `copy_from_slice`）。
  panic 条件は完全に同一（`offset + N > len`）。`Result` / `Option` を返す形にすると
  panic → エラーに**挙動が変わり**、公開関数 4 つのシグネチャと 5 箇所の呼び出し元も変わる。同じコミットでやらない。
- 境界チェックをここで「ついでに足す」のも禁止（別の改修）。panic 位置がヘルパーに移るだけで挙動は同じ。
- **エンコード側（書き込み）には触らない**。`encode_frame`（`codec.rs:171`）は payload enum から
  バッファ長を決めるのに `frame.header.payload_length` をそのまま書いており（:187）、
  ここを「計算値に直す」とバイト列が変わる。リファクタではなく仕様変更になる。

### 副次効果

本番コードの `unwrap()` 78 件のうち **68 件がこれで消える**（`packet.rs` 34 + `tlog_reader.rs` 32 +
`codec.rs` 1 + `router.rs` 1）。

### 検証（完了条件）

```powershell
# 0 になること（対象 3 ディレクトリのみ。latency.rs は対象外なので含めない）
(Get-ChildItem src\protocol,src\storage,src\transport -Recurse -Filter *.rs |
  ForEach-Object { ([regex]::Matches((Get-Content $_.FullName -Raw), 'try_into\(\)\.unwrap\(\)')).Count } |
  Measure-Object -Sum).Sum
```

### 発展（今回はやらない）

- 同じ規則が**書き込み側にも 72 箇所**ある（`packet.rs` 34、`tlog_writer.rs` 37、`tlog_reader.rs` 1）。
  読み込み 68 + 書き込み 72 = **140 箇所**が 1 つの規則で説明できる。ただし書き込み側は
  `copy_from_slice`（固定オフセット）と `extend_from_slice`（追記）の 2 形態があり、
  ヘルパーを 2 種類用意するか決めてから着手する（差分を小さくしたいなら別コミット）。
- ヘルパーを `Result` 返しに変えれば「不正入力での panic を型で防ぐ」改修が **1 箇所の変更**で済む。
  今は同じ判断が 68 箇所に散っているので、まずこの章を済ませてから別タスクにする。

---

## 4. ★テストフィクスチャの共通化（80 箇所以上）

### 修正方針

テストコード内でコピペされている構造体リテラル・ハーネス構築を、
既存の `tests/support/`（`FakeClock` / `FakeIngressSink` / `FakeLogSink` を提供済み）に集約する。

### 対象（実測）

| コピペされているもの | 箇所数 | 出現ファイル |
| :--- | ---: | :--- |
| `BrokerConfig { …16 フィールド… }` リテラル | 35 | `tests/integration_test.rs` 11、`src/ui/settings.rs`（テスト領域）7、`tests/replay_test.rs` 5、`tests/transport_test.rs` 5、`tests/ui_test.rs` 5、他 2 |
| `Header { magic: MAGIC_TICK, protocol_version: …, message_type: … }` | 17 | `tests/transport_test.rs` 6、`tests/integration_test.rs` 5、`tests/protocol_test.rs` 3、他 3 |
| `TickId { … }` / `TickRecord` の末尾 4 フィールド | 19 / 15 | 11 ファイル / 6 ファイル |
| egui ハーネス `SnapshotExchange::new(…)` + `DashboardApp::new(…)` | 21 / 26 | `src/ui/dashboard/mod.rs`、`tests/ui_test.rs`、`src/ui/settings.rs` ほか |
| `UiSnapshot::default()` の手組み | 20 | 同上 |

- 既存の `tests/support/` を使っているテストファイルは **20 ファイル中 2 つだけ**。事実上未活用。
- `tests/replay_test.rs` の `make_test_config()`（1 定義 / 9 呼び出し）だけが唯一の成功例。これを横展開する。

### 追加する関数（例）

```rust
// tests/support/mod.rs
pub fn make_broker_config(id: u32, port: u16) -> BrokerConfig
pub fn make_header(message_type: u16) -> Header
pub fn make_tick(broker_id: u32, bid: f64, ask: f64) -> TickRecord
pub fn headless_ui() -> (Arc<SnapshotExchange>, DashboardApp)
```

### 注意（技術的制約）

- `tests/` の統合テストと `src/**` の `#[cfg(test)]` ユニットテストは**別クレート**なので、
  `tests/support` をユニットテストからは参照できない。
  → 統合テスト用は `tests/support/`、ユニットテスト用は lib 内の `#[cfg(test)] pub(crate) mod test_util` に置く
  （もしくは `src` 側はこの章の対象外にして放置する。**どちらかに統一**すること）。
- **テスト専用の変更なので、本体の挙動リスクはゼロ**。`cargo test --features replay` が緑であることだけ確認する。

### 検証

```powershell
cargo test --features replay        # テスト件数（214 件）と結果が変わらないこと
```

---

## 5. UI の色リテラルを名前付き定数へ（84 箇所 / 15 ファイル / 44 色）★対応済み

**対応状況**: 完了。`src/ui/style.rs` に 26 個、`src/ui/chart/theme.rs` に 18 個の計 **44 定数**を追加し、
パレット外の色リテラルを **0** にした（検証コマンドは下記のとおり 0 を返す）。
値は元のリテラルと完全に同一で、置換は文字列の機械置換のみ。色は 1 つも変えていない。

- 内訳: テキスト階調 10 / 行背景 2 / 見出し・操作系 8 / ステータス 6（style.rs）、
  チャート系列 6 / トレードオーバーレイ 12（chart/theme.rs）
- 近いが異なる色（`style::ERROR` と `STATUS_ALERT`、`STATUS_OK` と `FRESH_ALL` など）は
  **統合せず**、その方針をパレットのコメントに明記した
- `Color32::from_rgba_unmultiplied` は egui 0.29 で `const fn` ではないため、
  半透明の 6 色（行背景 2・トレード背景 4）だけは定数ではなく関数にした
- 置換で不要になった `Color32` の import を 4 ファイルから削除

以下は実施時に使った調査内容（記録として残す）。

### 修正方針

`src/ui/style.rs` と `src/ui/chart/theme.rs` 以外では
`Color32::from_rgb` / `from_rgba_unmultiplied` / `from_gray` を **書かない**。
出現する色に名前を付けて両モジュールへ追加し、呼び出し側は定数を参照する。

### 対象（実測 84 箇所）

| ファイル | 箇所数 |
| :--- | ---: |
| `src/ui/chart/candlestick.rs` | 18 |
| `src/ui/dashboard/charts_view.rs` | 13 |
| `src/ui/dashboard/header.rs` | 11 |
| `src/ui/dashboard/quick_settings.rs` | 10 |
| `src/ui/chart/bottom/breadth.rs` | 6 |
| `src/ui/chart/difference.rs` | 5 |
| `src/ui/chart/bottom/dispersion.rs` | 4 |
| `src/ui/chart/bottom/lead_lag.rs` | 4 |
| `src/ui/chart/quote_path.rs` | 3 |
| その他 6 ファイル | 10 |

同一値の重複は **18 色 / 58 箇所**。最大の塊は **グレー 10 段階 / 30 箇所**：
`from_gray(180)`×8、`140`×6、`160`×5、`120`×2、`150`×2、`190`×2、`185`×2、`130`×1、`170`×1、`60`×1。

### 注意（意味論は触らない）

- 既存の `ui::style` / `chart::theme` の定数と **RGB が一致する色は 1 つも無い**。つまり
  「既存定数に置換」ではなく「新規定数を追加」する作業になる。
- `style::ERROR (239,139,147)` と `(255,140,140)`、`COLOR_OANDA (0,211,126)` と `(0,210,130)` のように
  **近いが違う色が 7 組**ある（差分 Δ4〜24）。**勝手に寄せない**。定数化のみ行い、統合は別タスクとしてユーザー判断に委ねる。
- グレー 10 段階は意図的な濃淡かドリフトか判別できない。定数名は `from_gray(180)` → `TEXT_DIM` のように
  **用途ベースで命名**し、値の統合はしない。

### 検証（完了条件）

```powershell
# 0 になること
(Get-ChildItem src\ui -Recurse -Filter *.rs |
  Where-Object { $_.Name -ne 'style.rs' -and $_.FullName -notmatch 'chart\\theme\.rs$' } |
  ForEach-Object { (Select-String -Path $_.FullName -Pattern 'Color32::from_(rgb|rgba_unmultiplied|gray)' -AllMatches).Matches.Count } |
  Measure-Object -Sum).Sum
```

---

## 6. `std::sync` のロックを `parking_lot` に統一（14 箇所）★対応済み

**対応状況**: 完了。`logging.rs` / `storage/tlog_writer.rs` / `state/snapshot.rs` /
`transport/tcp.rs`（テスト）/ `tests/cli_and_logging_test.rs` を `parking_lot` へ移し、
poison 処理（`.expect("…mutex poisoned")` 8 箇所、`.unwrap_or_else(|e| e.into_inner())` 2 箇所、
`.lock().unwrap()` 3 箇所、`if let Ok(guard)` 2 箇所）を全廃した。
`src/` と `tests/` に `std::sync::Mutex` / `RwLock` は 1 つも残っていない。

`tlog_writer.rs` の fault mutex は「poison したら panic」から「そのまま続行」に変わるが、
ガードしているのは `Option<StorageError>` 1 つだけで、壊れうる不変条件がないため安全と判断した。
`logging.rs` / `snapshot.rs` / `tcp.rs`(テスト) は元から poison を無視していたので挙動は完全に同じ。

以下は実施時に使った調査内容（記録として残す）。

### 修正方針

`parking_lot` は既に依存にあり **72 箇所で使用済み**。残る `std::sync` の 14 箇所を `parking_lot` へ移し、
poison 処理（`.expect("…mutex poisoned")` ×7、`.unwrap_or_else(|e| e.into_inner())` ×2、
`.lock().unwrap()` ×3、`if let Ok(guard)` ×2）を**すべて削除**する。

| ファイル | 箇所数 | 内容 |
| :--- | ---: | :--- |
| `src/storage/tlog_writer.rs`（:13, :352〜:467） | 7 | `Mutex` + `.expect("logger fault mutex poisoned")` |
| `src/logging.rs`（:9, :115, :123） | 2 | `Mutex` + `.unwrap_or_else(\|e\| e.into_inner())` |
| `src/state/snapshot.rs`（:71, :78, :94, :106） | 2 | `std::sync::RwLock` |
| `src/transport/tcp.rs`（:412, :426） | 2 | テスト用 `struct Ingress(Mutex<_>)` + `.lock().unwrap()` |
| `tests/cli_and_logging_test.rs` | 1 | `.lock().unwrap()` |

### 注意

- `tlog_writer.rs` の 7 箇所は意味が変わる：`parking_lot` は poison しないため、
  「poison したら panic」→「poison という概念が無く、そのまま続行」になる。
  このファイルはログ永続化の障害通知を持つので、**障害時に panic させたいのか継続したいのか**を先に決める。
  現状は panic 側なので、継続に変えるなら `log::error!` を 1 行足して明示する。
- `logging.rs` / `snapshot.rs` / `tcp.rs`(テスト) は既に poison を無視しているので **挙動は完全に同じ**。

### 検証（完了条件）

```powershell
# 0 行になること
Get-ChildItem src -Recurse -Filter *.rs |
  Select-String -Pattern 'into_inner\(\)\)|mutex poisoned|\.lock\(\)\.unwrap\(\)|std::sync::(Mutex|RwLock)'
```

---

## 7. 未使用の公開関数 26 件の整理（＋再発防止）

### 修正方針

`src/` の `pub fn` のうち、`src/` と `tests/` のどこからも参照されていないものが **26 件**ある。
名前がリポジトリ内で 1 回（定義時のみ）しか出現しないことを機械的に判定済み。

```powershell
# 再検出コマンド（各関数名について refs=1 なら未使用）
Get-ChildItem src,tests -Recurse -Filter *.rs | Select-String -Pattern '\bdraw_difference_chart\b' -AllMatches
```

代表例:
`src/deploy/mod.rs:31 deploy_mt5_files` / `src/core/types.rs:132 points_to_price` /
`src/metrics/hypothesis.rs:239 evaluate_fingerprint` / `src/metrics/persistence.rs:236 record_reversion` /
`src/ui/dashboard/mod.rs:321 mark_dirty` / `:651 set_x_axis_mode` /
`src/ui/chart/difference.rs:10 draw_difference_chart` / `src/ui/chart/candlestick.rs:14 draw_candlestick_chart` /
`src/ui/chart/candlestick.rs:630 draw_trade_overlays` / `src/ui/dashboard/latency.rs:5 draw_latency_dashboard` /
`src/ui/dpi.rs:20 get_dpi_scale_at_point` ほか計 26 件。

### 注意（削除前にユーザー確認が必要）

- **`mark_dirty` / `set_x_axis_mode` / `draw_*_chart` 系が死んでいるのは「機能が呼ばれていない」サインの可能性がある。**
  過去のリファクタで呼び出し元が消えただけなのか、機能退行なのかはコードだけでは判断できない。
  → 削除の前に「これは不要か、復活させるべきか」をユーザーに確認する。
- 削除ではなく「残す」判断をする場合は、`///` に用途と理由を 1 行書いて意図を明示する。

### 再発防止（同じ作業単位で実施）

`lib` クレートの `pub` 項目は dead_code 警告が出ないため、未使用 API が静かに蓄積する。
各モジュールの `pub` を `pub(crate)` に下げ、**コンパイルエラーになったものだけ `pub` に戻す**手順で
「本当に外部公開が必要な API」だけを残せる（`src/main.rs` と `src/bin/replay.rs` から使うものは `pub` のまま）。

### 検証

```powershell
cargo build --release --features replay   # ビルドが通ること
cargo clippy --all-targets -- -D warnings # 警告ゼロ
cargo test --features replay
```

---

## 8. その他の機械的 lint ★一部対応済み

**対応状況**: `missing_const_for_fn` (164) と `use_self` (91) は適用済み
（**38 ファイル / +245 −245 行**、純粋な置換のみ）。
`str_to_string` (108) は **見送り** — `"x".to_string()` → `"x".to_owned()` は
どちらも等価で慣用的であり、108 箇所を書き換えても読みやすさが上がらないため。

適用したコマンド:

```powershell
cargo clippy --fix --all-targets --features replay --allow-dirty `
  -- -W clippy::missing_const_for_fn -W clippy::use_self
```

| lint | 本番 | 状態 |
| :--- | ---: | :--- |
| `clippy::missing_const_for_fn` | 164 | **適用済み**（`fn` → `const fn`） |
| `clippy::use_self` | 91 | **適用済み**（`impl Foo { fn new() -> Foo }` → `-> Self`） |
| `clippy::str_to_string` | 108 | 見送り（上記理由） |

### 小口 lint 13 種（71 箇所）★対応済み

自動修正できない小さな lint を 1 種類ずつ潰した。**71 箇所 → 0**（32 ファイル / +150 −150 行）。

| lint | 箇所 | 対応 |
| :--- | ---: | :--- |
| `clippy::unreadable_literal` | 33 | 数値リテラルに `_` 区切りを追加（値は不変） |
| `clippy::manual_let_else` | 11 | `let x = if let ... { } else { return }` を let-else に書き換え |
| `clippy::field_reassign_with_default` | 8 | `Default::default()` + フィールド代入を構造体更新記法に |
| `clippy::match_same_arms` | 4 | 同一本体の arm を or-pattern に統合 |
| `clippy::assigning_clones` | 3 | `a = b.clone()` → `a.clone_from(&b)` |
| `clippy::items_after_statements` | 3 | 関数内の `use` を関数先頭へ移動（`#[cfg(windows)]` は維持） |
| `clippy::format_push_string` | 2 | `push_str(&format!(..))` → `writeln!`（`write_with_newline` も同時に解消） |
| `clippy::unnecessary_wraps` | 2 | **serde の `default = "..."` 関数はフィールド型を返す必要がある**ため誤検知。理由コメント付き `#[allow]` |
| `clippy::comparison_chain` | 1 | `if a > b / else if a < b` を `match a.cmp(&b)` に |
| `clippy::needless_continue` | 1 | ループ末尾の冗長な `continue` を削除 |
| `clippy::struct_field_names` | 1 | `SyncArbiter` の private フィールドから `last_observed_` 接頭辞を削除 |
| `clippy::default_trait_access` | 1 | `Default::default()` → `egui::RawInput::default()` |
| `clippy::should_panic_without_expect` | 1 | `#[should_panic(expected = "range end index")]` |

Clipy の suggestion を機械適用する際、`match_same_arms` は**複数 span の同時適用が前提**で、
片方だけ適用すると壊れる（実際に一度壊して手で直した）。それ以外の 12 種は単一 span で安全。

### ドキュメント補完（33 関数）★対応済み

**対応状況**: 完了。`missing_errors_doc` 30 件・`missing_panics_doc` 5 件（関数としては 33 件。
うち 2 件は両方に該当）をすべて解消し、**21 ファイル / +198 行**（doc のみ、コード変更なし）。

- 実装を読んでから書いた。エラー節は実際に返す variant と条件を列挙し、
  存在しない variant は書いていない。
- `# Panics` は Clippy が指摘した実際の panic 箇所（`try_into().unwrap()`、
  `checked_sub(..).unwrap()`、`get_mut(..).unwrap()`、`remove(..).unwrap()`）を確認し、
  いずれも「不変条件により到達しない」ことを根拠とともに明記した。
- 例外として、実際には失敗しない関数（`logging::init_logging`、
  `storage::tlog_writer::encode_record`、`BrokerParquetSource::new`）は
  「現在は失敗しない。Result は将来の失敗に備えて残している」と正直に書いた。

検証: `cargo doc --no-deps --features replay` が警告なし /
      doc 2 lint が 0 件 / cargo test 41 スイート 417 passed 0 failed

| lint | 対応前 | 対応後 |
| :--- | ---: | ---: |
| `clippy::missing_errors_doc` | 30 | 0 |
| `clippy::missing_panics_doc` | 5 | 0 |

### 重複している小関数の統合（2 件対応済み・1 件は見送り）

1. ~~**`civil_from_days` の重複**~~ → **対応済み**。実装は `src/core/civil_date.rs` の 1 つだけになり、
   `src/config/timezone.rs`、`src/storage/tlog_writer.rs`、`src/ui/chart/candlestick.rs` の
   `format_utc_timestamp`、`src/logging.rs` の `format_utc_timestamp` はすべてそこへ委譲する。
   当初 3 コピーと見ていたが、**UI 層（`candlestick.rs`）にもう 1 コピーあり、実際は 4 コピー**だった。
   置き場所は `logging` では不自然なので `core` へ移し、依存の向きを素直にしてある。
   回帰検知は 4 層すべてが反応することを、共通実装を一時的に壊して確認済み。
   あわせて暦ヘルパー（`ymd_to_days` / `day_of_week` / `nth_sunday_of_month` /
   `unix_sec_to_ymd` / `ymd_hms_to_unix_sec`）も `core::civil_date` へ移し、
   `config/timezone.rs`（242 行 → 120 行）は DST ルール専用になった。
2. ~~**IO エラーの文脈付与が 10 箇所**~~ → **見送り（実装しない判断）**。
   `src/storage/tlog_writer.rs` 7 箇所と `src/transport/router.rs` 3 箇所の
   `.map_err(|error| format!("…: {error}"))` を共通ヘルパーに畳む案だったが、実際に読むと
   **10 箇所のうち 4 箇所はパスを埋め込む**（`format!("failed to create '{}': {error}", path.display())`）。
   ヘルパーに寄せるとその 4 箇所は「エラー時のみ評価される」利点を失って毎回文字列を組み立てるうえ、
   残り 6 箇所も `map_err` + クロージャという Rust の標準的な書き方で十分短い。
   変更しても読みやすさが上がらないため、ここは触らない。
3. ~~**`utc_date_now`（`tlog_writer.rs:222`）**~~ → **対応済み**。
   `logging::format_utc_date(SystemTime)` を追加し、`format_utc_timestamp` と共通の
   `utc_split` を使うようにした。`tlog_writer::utc_date_now` は 1 行の委譲になり、
   日付組み立ての重複は消えた。

---

## 9. これはやらない方がよい（機械的に直せない）

| lint | 本番件数 | 理由 |
| :--- | ---: | :--- |
| `clippy::cast_precision_loss` | 62 | `as f64` が正当な箇所と危険な箇所を個別に判断する必要がある |
| `clippy::cast_possible_truncation` | 57 | 同上（`try_into()?` 化はエラー型の設計変更を伴う） |
| `clippy::cast_sign_loss` | 34 | 同上 |
| `clippy::too_many_lines` | 24 関数 | 関数分割は設計判断（付録 A 参照） |
| `clippy::needless_pass_by_value` | 21 | シグネチャ変更＋全呼び出し側の修正。簡単ではない |
| `clippy::float_cmp` | 68 | 大半がテストの `assert_eq!`。実害なし |

その他、**調査の結果「該当なし」だったもの**（作業指示に含めないこと）:

- `TODO` / `FIXME` / `XXX` / `HACK` / `todo!` / `unimplemented!` … **0 件**。
- print 系の規約違反 … **0 件**。`log::` マクロ 82 箇所はすべて `log::` 接頭辞付き、`println!` 系 15 箇所は
  2 つのバイナリの CLI 表示のみで規約どおり。
- `#[allow(dead_code)]` … `src/` には **0 件**（`tests/` に 5 件）。
- `#[must_use]` 属性 … リポジトリ全体で **0 件**（§1 で 480 件追加される）。

---

## 付録 A. 大きめの構造リファクタ（「簡単」ではないが効果大）

§1〜§7 を終えた後の第 2 弾として。UI の非テスト 6,554 行のうち **約 450 行**が削減できる見込み。

| 対象 | 内容 | 削減 |
| :--- | :--- | ---: |
| `src/ui/chart/candlestick.rs` | 6 つの公開ラッパーが 10〜16 引数を再宣言して転送している（同一引数ブロックが 6 回反復）。`CandleDrawRequest` 構造体 1 つに集約でき、`src/ui/chart/mod.rs:3` の `#![allow(clippy::too_many_arguments)]` も削除できる | −190 行 |
| `src/ui/chart/difference.rs` | 176〜471 行が 3 チャート分のコピー（描画ループが 4 箇所でほぼ同一）。`draw_diff_chart(series, value_of, …)` に汎用化 | −146 行 |
| `src/ui/chart/quote_path.rs` ↔ `candlestick.rs` | 凡例の折り返し・非表示・オーバーフロー表示が 2 ファイルで重複 | −53 行 |
| `src/ui/chart/candlestick.rs` ↔ `quote_path.rs` | 価格グリッド＋軸ラベルの描画が 20 行 × 2 で重複 | −16 行 |
| `src/ui/chart/bottom/dispersion.rs` ↔ `persistence.rs` | ブローカー行の可視判定とラベル描画が重複（`is_none_or` の行は完全一致） | −14 行 |
| 全体 | 中央寄せプレースホルダ文言（8 箇所）、背景 `rect_filled`（9 箇所）の共通化 | −34 行 |

小口だが独立した作業単位:

- ~~**broker 名引き当ての共通関数化**~~ → **対応済み**。`src/ui/shared.rs` に
  `broker_name(&[BrokerOverview], BrokerId, fallback) -> &str` と
  `broker_index(...) -> usize` を追加し、UI の 7 箇所（candlestick / breadth / header /
  dashboard/mod）を置換。`src/tick/engine.rs` の 4 箇所は `BrokerConfig` を引く別型なので、
  同ファイル内の private ヘルパー `broker_name(&[BrokerConfig], BrokerId)` に集約した。
  なお candlestick.rs ではループ変数 `broker_index` が関数名を隠すため完全修飾で呼んでいる。
- ~~**`selectable_label(...).clicked() { app.set_* }` の共通化（19 箇所）**~~ → **見送り**。
  実際に読むと 26 箇所の形状が揃っていない：`.on_hover_text()` を鎖す箇所、
  `b.broker_id != X && ui.selectable_label(..).clicked()` のように複合条件の一部になっている箇所、
  `let resp = ui.selectable_label(..)` と応答を先に束縛する箇所が混在する。
  1 つの `choice_row` では最も単純な数箇所しか覆えず、変種を増やすほど間接参照が増えるため、
  egui の標準的な書き方のままにした。
- ~~**精度ラダーの共通化**~~ → **対応済み**。`src/ui/shared.rs` に
  `format_pips(pips, unit) -> String` を追加し、`settings.rs`（`" pips"`）と
  `header.rs` 2 箇所・`quick_settings.rs`（`"p"`）の計 4 箇所を置換。
  3 段階のラダーを持つ `candlestick.rs:468` は規則が異なる（0/1/2 桁）ため対象外とした。
- **`main.rs` と `bin/replay.rs` の共通化**: 引数解析 → `load_startup_config` → deploy → coordinator →
  eframe 起動という 117 行が両者でほぼ同一。共有ブートストラップ関数 1 つに抽出できる。**未着手**。
- ~~**ソケット設定の `.ok()` 握り潰し（8 箇所）**~~ → **対応済み**。
  `tcp.rs` 6 箇所と `router.rs` 2 箇所を `if let Err(error) = .. { log::warn!("…: {error}"); }` に変更。
  失敗しても続行する挙動は変えず、記録だけが残るようにした。`src/transport` に `.ok();` は残っていない。

---

## 付録 B. 環境メモ（作業前に確認）

1. **MSRV が README と実態でずれている**
   README は「Rust 1.80+」と書いているが、`Option::is_none_or`（Rust 1.82 で安定化）を既に 3 箇所で使用
   （`src/storage/tlog_writer.rs:317`、`src/ui/chart/bottom/persistence.rs:33`、`src/ui/chart/bottom/dispersion.rs:79`）。
   実質 1.82+ が必要。`f64::midpoint`（1.85+）を使うなら README の更新か `clippy.toml` の `msrv` 指定が必要。

2. **ビルド用ディレクトリの権限**
   `D:\dev\TickScope\target` は所有者が別ユーザー（`DESK\CodexSandboxOffline`）になっており、
   現在のユーザーでは書き込めず `cargo` が失敗する。`CARGO_TARGET_DIR` を別ディレクトリに向けるか、
   そのフォルダーのアクセス権を修復してから作業する。

3. **テストのベースライン**
   `cargo test` と `cargo test --features replay` はどちらも緑（`0 failed`、MT5 実機が要る 1 件のみ `ignored`）。
   作業の前後で必ず両方を実行し、結果が変わったらその変更を切り戻す。
