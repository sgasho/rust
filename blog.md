# Rustコンパイラに"MIR最適化の二分探索"を実装した話 ── `-Zmir-opt-bisect-limit` の設計と課題

## 前提となる技術要素

### MIR (Mid-level Intermediate Representation) とは何か

Rustコンパイラの内部表現の一つ。ソースコード → HIR → **MIR** → LLVM IR → 機械語という変換パイプラインの中間に位置する。

HIR（高レベル）はRustの構文に近い表現、LLVM IR（低レベル）は機械語に近い表現。MIRはその中間で、**所有権チェック（借用チェッカー）** や **最適化** を行うために設計された表現である。

MIRは関数ごとに生成され、基本ブロック（BasicBlock）の制御フローグラフ（CFG）として表現される。

```
ソースコード → HIR → MIR → LLVM IR → 機械語
                       ↑
              借用チェック・最適化はここで行われる
```

MIRには複数の「最適化パス」が順番に適用される。例：`SimplifyCfg`（制御フローの簡略化）、`CopyProp`（コピー伝播）、`RemoveZsts`（ゼロサイズ型の除去）など。

これらのパスは `compiler/rustc_mir_transform/src/pass_manager.rs` で管理されている。

### bisect（二分探索デバッグ）とは何か

コンパイラの最適化パスは通常数十個が順番に適用される。ある最適化パスにバグがある場合、**どのパスが原因か**を特定する必要がある。

- 素朴なアプローチ：パスを1つずつ無効化して試す → パスが50個あれば50回試行が必要
- bisect（二分探索）アプローチ：「最初のN個だけ実行」というリミットを設定し、二分探索で問題のパスを絞り込む → **O(log N)** で特定可能

これはLLVMの `-opt-bisect-limit` として既に存在する機能。本PRはこの概念をRustのMIR最適化パスに導入したもの。

```
パス1 → パス2 → ... → パス25 → パス26 → ... → パス50
                        ↑ limit=25なら、ここまで実行
                                   ↑ ここからスキップ

「limit=25で正常、limit=26でバグ」→ パス26が原因
```

## PRの概要

- Issue: [#150910](https://github.com/rust-lang/rust/issues/150910) — `-Zmir-enable-passes=+SimplifyCfg`が機能しない＆警告も出ない
- 元々はパス名のダンプ手段がないという問題から出発し、Zulipでのレビュアー（saethlin）との議論を経て「bisect機能自体を実装しよう」という方向に発展
- `-Zmir-opt-bisect-limit=N` というコンパイラフラグを新設。Nまでの最適化パスだけを実行し、残りをスキップする
- 変更は計5ファイル、+198行 / -1行

## 使用例

```rust
fn abs(num: isize) -> usize {
    if num < 0 { -num as usize } else { num as usize }
}

fn main() {
    println!("{}", abs(-10));
}
```

```shell
$ rustc -Zmir-opt-bisect-limit=30 src/main.rs

BISECT: running pass (1) CheckAlignment on main::main
BISECT: running pass (2) CheckNull on main::main
...
BISECT: running pass (30) InstSimplify-before-inline on main::abs
BISECT: NOT running pass (31) ForceInline on main::abs
BISECT: NOT running pass (32) RemoveStorageMarkers on main::abs
...
```

- `running pass` = 実行されたパス、`NOT running pass` = スキップされたパス
- limit=30の場合、30番目までが実行され、31番目以降はスキップ
- ポイント：**関数ごとではなく、すべての関数のパスがグローバルに通し番号で管理される**（`main`関数のパスが終わった後に`abs`関数のパスが続く）
- これにより、「limit=30では正常だがlimit=31ではクラッシュする」という状況から、31番目のパス（`ForceInline on abs`）が原因だと即座に特定できる

### 実際の動作確認

`./x setup` および `./x build --stage 1 compiler library` でstage1コンパイラをビルドし、`rustup toolchain link stage1` で登録した上で実行した結果：

```shell
$ rustc +stage1 -Zmir-opt-bisect-limit=30 src/main.rs
BISECT: running pass (1) CheckAlignment on main[acb8]::main
BISECT: running pass (2) CheckNull on main[acb8]::main
BISECT: running pass (3) CheckEnums on main[acb8]::main
BISECT: running pass (4) LowerSliceLenCalls on main[acb8]::main
BISECT: running pass (5) InstSimplify-before-inline on main[acb8]::main
BISECT: running pass (6) ForceInline on main[acb8]::main
BISECT: running pass (7) RemoveStorageMarkers on main[acb8]::main
BISECT: running pass (8) RemoveZsts on main[acb8]::main
BISECT: running pass (9) RemoveUnneededDrops on main[acb8]::main
BISECT: running pass (10) UnreachableEnumBranching on main[acb8]::main
BISECT: running pass (11) SimplifyCfg-after-unreachable-enum-branching on main[acb8]::main
BISECT: running pass (12) InstSimplify-after-simplifycfg on main[acb8]::main
BISECT: running pass (13) SimplifyConstCondition-after-inst-simplify on main[acb8]::main
BISECT: running pass (14) SimplifyLocals-before-const-prop on main[acb8]::main
BISECT: running pass (15) SimplifyLocals-after-value-numbering on main[acb8]::main
BISECT: running pass (16) SingleUseConsts on main[acb8]::main
BISECT: running pass (17) SimplifyConstCondition-after-const-prop on main[acb8]::main
BISECT: running pass (18) SimplifyConstCondition-final on main[acb8]::main
BISECT: running pass (19) RemoveNoopLandingPads on main[acb8]::main
BISECT: running pass (20) SimplifyCfg-final on main[acb8]::main
BISECT: running pass (21) CopyProp on main[acb8]::main
BISECT: running pass (22) SimplifyLocals-final on main[acb8]::main
BISECT: running pass (23) AddCallGuards on main[acb8]::main
BISECT: running pass (24) PreCodegen on main[acb8]::main
BISECT: running pass (25) CheckAlignment on main[acb8]::abs
BISECT: running pass (26) CheckNull on main[acb8]::abs
BISECT: running pass (27) CheckEnums on main[acb8]::abs
BISECT: running pass (28) LowerSliceLenCalls on main[acb8]::abs
BISECT: running pass (29) InstSimplify-before-inline on main[acb8]::abs
BISECT: running pass (30) ForceInline on main[acb8]::abs
BISECT: NOT running pass (31) RemoveStorageMarkers on main[acb8]::abs
BISECT: NOT running pass (32) RemoveZsts on main[acb8]::abs
BISECT: NOT running pass (33) RemoveUnneededDrops on main[acb8]::abs
BISECT: NOT running pass (34) UnreachableEnumBranching on main[acb8]::abs
BISECT: NOT running pass (35) SimplifyCfg-after-unreachable-enum-branching on main[acb8]::abs
BISECT: NOT running pass (36) InstSimplify-after-simplifycfg on main[acb8]::abs
BISECT: NOT running pass (37) SimplifyConstCondition-after-inst-simplify on main[acb8]::abs
BISECT: NOT running pass (38) SimplifyLocals-before-const-prop on main[acb8]::abs
BISECT: NOT running pass (39) SimplifyLocals-after-value-numbering on main[acb8]::abs
BISECT: NOT running pass (40) SingleUseConsts on main[acb8]::abs
BISECT: NOT running pass (41) SimplifyConstCondition-after-const-prop on main[acb8]::abs
BISECT: NOT running pass (42) SimplifyConstCondition-final on main[acb8]::abs
BISECT: NOT running pass (43) RemoveNoopLandingPads on main[acb8]::abs
BISECT: NOT running pass (44) SimplifyCfg-final on main[acb8]::abs
BISECT: NOT running pass (45) CopyProp on main[acb8]::abs
BISECT: NOT running pass (46) SimplifyLocals-final on main[acb8]::abs
BISECT: NOT running pass (47) AddCallGuards on main[acb8]::abs
BISECT: NOT running pass (48) PreCodegen on main[acb8]::abs
```

`main`関数に24パス、`abs`関数に24パスの合計48パスが存在する。limit=30の場合、30番目（`ForceInline on abs`）までが実行され、31番目以降がスキップされていることが確認できる。

### ケーススタディ：bisectでバグの原因パスを特定する

bisectの真価を示すため、実際にCopyPropパスにバグを埋め込み、それをbisectで特定する実験を行った。

#### バグの内容

`compiler/rustc_mir_transform/src/copy_prop.rs` の `run_pass` 末尾に、以下のコードを追加した。

```rust
// Neg（符号反転）操作を含む関数でのみ発動する条件付きバグ。
// 現実のコンパイラバグでも、特定のMIR構造が揃ったときだけ
// 発現するケースは珍しくない。
let has_neg = body.basic_blocks.iter().any(|bb| {
    bb.statements.iter().any(|stmt| {
        matches!(
            &stmt.kind,
            StatementKind::Assign(box (_, Rvalue::UnaryOp(UnOp::Neg, _)))
        )
    })
});

if has_neg {
    for block in body.basic_blocks_mut() {
        if let TerminatorKind::SwitchInt { ref mut targets, .. } =
            block.terminator_mut().kind
        {
            let all_targets = targets.all_targets_mut();
            if all_targets.len() >= 2 {
                // switchIntのジャンプ先を入れ替える。
                // これにより if の then と else が逆転する。
                all_targets.swap(0, all_targets.len() - 1);
            }
        }
    }
}
```

このバグは、Neg（`-x` のような符号反転）を含む関数に対してのみ `switchInt` の分岐先を反転させる。`abs()` は `-num` を含むのでバグの対象となるが、標準ライブラリの大半の関数には影響しない。

#### バグの発現

バグ入りコンパイラで `abs(-10)` をコンパイル・実行すると：

```shell
$ rustc +stage1 src/main.rs -o abs_buggy && ./abs_buggy
18446744073709551606
```

期待値の `10` ではなく `18446744073709551606` が出力された。これは `-10` を符号なし64bit整数（`usize`）にそのままキャストした値であり、if文の分岐が逆転していることを示している。

#### bisectによる原因特定

bisect-limitを0から順に上げていき、各limit値でプログラムの出力を確認する。

```shell
limit=0  → 10 (正常)
limit=1  → 10 (正常)
...
limit=44 → 10 (正常)
limit=45 → 18446744073709551606 (バグ!)
limit=46 → 18446744073709551606 (バグ!)
...
limit=48 → 18446744073709551606 (バグ!)
```

**limit=44で正常、limit=45でバグが発現**する。45番目のパスを確認すると：

```shell
$ rustc +stage1 -Zmir-opt-bisect-limit=48 src/main.rs -o /tmp/test 2>&1 | grep "pass (4[3-6])"
BISECT: running pass (43) RemoveNoopLandingPads on main[acb8]::abs
BISECT: running pass (44) SimplifyCfg-final on main[acb8]::abs
BISECT: running pass (45) CopyProp on main[acb8]::abs        ← 犯人
BISECT: running pass (46) SimplifyLocals-final on main[acb8]::abs
```

**パス(45) `CopyProp on abs` が原因であると即座に特定できた。**

実際の運用では、このように全数探索する必要はなく、二分探索（`limit=24 → OK, limit=36 → OK, limit=42 → OK, limit=45 → NG, limit=44 → OK`）を行えば、48パスの中から **わずか5〜6回の試行** で原因パスを絞り込める。

## 実装の解説 ── Atomicityの設計

### カウンタの仕組み

```rust
// compiler/rustc_session/src/session.rs
pub struct Session {
    // ...
    pub mir_opt_bisect_eval_count: AtomicUsize,
}
```

```rust
// compiler/rustc_mir_transform/src/pass_manager.rs
fn limited_by_opt_bisect<'tcx, P>(
    tcx: TyCtxt<'tcx>,
    def_path: String,
    limit: usize,
    pass: &P,
) -> bool {
    let current_opt_bisect_count =
        tcx.sess.mir_opt_bisect_eval_count.fetch_add(1, Ordering::Relaxed);

    let can_run = current_opt_bisect_count < limit;
    // ...
    !can_run
}
```

- `AtomicUsize`を使って、パスが実行されるたびにグローバルカウンタを1ずつインクリメント
- `fetch_add(1, Ordering::Relaxed)` で、現在の値を取得しつつアトミックに+1する

### なぜAtomicが必要なのか

Rustコンパイラは `-Zthreads=N` で並列コンパイルが可能。複数スレッドが同時にMIR最適化パスを実行しうる。

もし通常の `usize` を使った場合：

```rust
// ❌ データ競合が発生する例
let count = self.counter;      // スレッドAが読む: 5
                                // スレッドBも読む: 5（まだAが書き込んでない）
self.counter = count + 1;      // スレッドAが書く: 6
                                // スレッドBも書く: 6（本来は7であるべき）
```

2つのスレッドが同時にカウンタを読み取ると、同じ値を読んでしまい、インクリメントが1回分消失する（lost update）。

Rustでは、そもそも `usize` を複数スレッドで共有しようとするとコンパイルエラーになる（`Sync` トレイトを実装していないため）。Rustの型システムがデータ競合を**コンパイル時に**防いでくれる。

`AtomicUsize` は CPU のアトミック命令（x86の`lock xadd`等）を使い、読み取り→加算→書き込みを**一つの不可分な操作**として実行する。

### 実験：AtomicUsizeをCell\<usize\>に変えてみる

実際に `AtomicUsize` を `Cell<usize>` に変えてコンパイラをビルドしてみた。`Cell<usize>` は単一スレッドでの内部可変性（共有参照 `&self` を通じた値の変更）を提供するが、スレッド安全ではない。

変更箇所：

```rust
// compiler/rustc_session/src/session.rs
// Before:
pub mir_opt_bisect_eval_count: AtomicUsize,
// After:
pub mir_opt_bisect_eval_count: Cell<usize>,

// compiler/rustc_mir_transform/src/pass_manager.rs
// Before:
let current_opt_bisect_count =
    tcx.sess.mir_opt_bisect_eval_count.fetch_add(1, Ordering::Relaxed);
// After:
let current_opt_bisect_count = tcx.sess.mir_opt_bisect_eval_count.get();
tcx.sess.mir_opt_bisect_eval_count.set(current_opt_bisect_count + 1);
```

結果：

```
error[E0277]: `Cell<usize>` doesn't implement `DynSync`.

   --> compiler/rustc_middle/src/ty/context.rs:754:29
    |
754 |     sync::assert_dyn_sync::<&'_ GlobalCtxt<'_>>();
    |                             ^^^^^^^^^^^^^^^^^^ within `GlobalCtxt<'_>`,
    |                             the trait `DynSync` is not implemented for `Cell<usize>`
    |
note: required because it appears within the type `Session`
   --> compiler/rustc_session/src/session.rs:98:12
    |
 98 | pub struct Session {
    |            ^^^^^^^
```

`Cell<usize>` は `Sync`（`DynSync`）を実装していないため、スレッド間で共有される `Session` 構造体のフィールドとして使用できない。`Session` → `GlobalCtxt` → スレッド間共有、という依存の連鎖を型システムが追跡し、**コンパイル時に**安全でない共有を検出している。

つまり、Rustでは「うっかりスレッドセーフでない型を使う」こと自体が不可能であり、`AtomicUsize` の使用はコンパイラによって強制されている。

### `Ordering::Relaxed` の選択

Atomicには複数のメモリオーダリングがある：`Relaxed`, `Acquire`, `Release`, `SeqCst` など。

- `Relaxed` は最も緩い保証で、「アトミック操作自体の不可分性」だけを保証し、他のメモリ操作との順序関係は保証しない
- ここでは「カウンタ値が正確にインクリメントされること」だけが重要で、パス間の実行順序の可視性は問題にならない（もともと並列実行時のパス順序は非決定的）ため、`Relaxed` で十分
- `SeqCst`（最も強い保証）を使えばスレッド間でカウンタの進行が厳密に見えるが、パフォーマンスコストがある割にこの用途では恩恵がない

## このPRが抱える問題

### 1. 並列コンパイル（`-Zthreads`）との非決定性

レビュアーsaethlinのコメント：

> This might have weird interactions with -Zthreads, but I don't see a coherent way to resolve that

- `-Zthreads=4` のように並列度を上げると、関数の最適化がどの順序で実行されるかが実行ごとに変わる
- つまり、同じ `limit=30` でも実行ごとに「30番目のパス」が異なる関数の異なるパスを指す可能性がある
- bisectの再現性が損なわれるため、テストでは `-Zthreads=1` を明示的に指定して回避している

```rust
// tests/run-make/mir-opt-bisect-limit/rmake.rs
cmd.arg("-Zthreads=1")  // 並列性を排除して再現性を確保
```

なお、実際に `-Zthreads=8` で100関数のプログラムを5回コンパイルする実験を行ったところ、パスの割り当て順序は毎回一致した。現状ではクエリシステムが関数を決定的な順序で処理するため、小〜中規模のプログラムでは非決定性が顕在化しにくい。ただし、大規模なプロジェクトやスレッドスケジューリングの揺らぎにより理論上は発生しうるため、テストでは `-Zthreads=1` が明示的に指定されている。

### 2. インクリメンタルコンパイルとの非互換

レビュアーbjorn3のコメント：

> We used to have optimization fuel in the session, but removed it in [#115293](https://github.com/rust-lang/rust/pull/115293)

過去にも似た機能（`-Zfuel`）が存在したが、**インクリメンタルコンパイルと非互換**であるとして[削除された](https://github.com/rust-lang/rust/pull/115293)。
本PRも同じ問題を抱えている。実際に再現してみた。

#### 再現実験

まず、2つの関数（`abs` + `main`）をインクリメンタルコンパイルで初回ビルドし、CopyPropのパス番号を確認する。

```shell
# 1回目: 初回コンパイル
$ rustc +stage1 -Zmir-opt-bisect-limit=999 -Cincremental=./incr src/main.rs
BISECT: running pass (21) CopyProp on main::main
BISECT: running pass (45) CopyProp on main::abs    ← absのCopyPropはパス45
```

次に、コードを変更せず再コンパイルする。

```shell
# 2回目: 変更なしで再コンパイル
$ rustc +stage1 -Zmir-opt-bisect-limit=999 -Cincremental=./incr src/main.rs
（BISECT出力なし — すべてキャッシュから復元され、MIR最適化は実行されない）
```

最後に、`abs` の前に新しい関数 `double` を追加して再コンパイルする。

```rust
fn double(x: usize) -> usize { x * 2 }   // ← 追加

fn abs(num: isize) -> usize {
    if num < 0 { -num as usize } else { num as usize }
}

fn main() {
    println!("{}", double(5));
    println!("{}", abs(-10));
}
```

```shell
# 3回目: double関数を追加して再コンパイル
$ rustc +stage1 -Zmir-opt-bisect-limit=999 -Cincremental=./incr src/main.rs
BISECT: running pass (21) CopyProp on main::main
BISECT: running pass (45) CopyProp on main::double  ← パス45がdoubleに!
```

`abs` はコード変更がないため、インクリメンタルキャッシュからMIRが復元され、最適化パスが実行されなかった。代わりに新しい `double` 関数がパス(45)を取った。

| コンパイル | パス(45)の対象 |
|---|---|
| 1回目 | `CopyProp on abs` |
| 3回目 | `CopyProp on double` |

**同じ `limit=45` が、1回目は `abs` のCopyProp、3回目は `double` のCopyPropを指す。** bisectの前提である「同じlimit値は常に同じパスを指す」が完全に崩壊しており、インクリメンタルコンパイルとbisectを併用すると、原因パスの特定が不可能になる。

bjorn3は「`Session`にカウンタを持つとインクリメンタルコンパイルが壊れるが、簡単な回避策はない」と述べている。

### 3. 複数セッション間の状態共有問題

bjorn3のコメント：

> This is actually worse. This now not only breaks incr comp, but also breaks running multiple rustc sessions in the same process.

- Rustコンパイラは一部のユースケース（rust-analyzerなど）で、同一プロセス内で複数のコンパイルセッションを実行することがある
- `Session` にカウンタを持つこと自体は各セッション独立だが、かつての `-Zfuel` は `static` 変数だったため複数セッションで共有されてしまっていた
- 本PRでは `Session` のフィールドとして持つ設計にしたため、セッション間の干渉は避けられているが、インクリメンタルコンパイルの問題は残る

### 4. 代替手法：Hash-based bisect

コントリビューターhanna-kruppeが提案した、根本的に異なるアプローチ：

> an interesting alternative that avoids this problem is [hash based bisect](https://research.swtch.com/bisect)

- Goコンパイラが採用している手法で、各最適化対象を**ハッシュ値**で識別する
- カウンタ方式（本PR）の代わりに、ハッシュの末尾ビットパターンでパスの有効/無効を制御する
- **利点**：実行順序・スレッド数に依存しない。並列コンパイルでも完全に再現可能
- **欠点**：実装が複雑。手動での二分探索が困難で、専用の自動化ツールが必要
- 本PRは「デバッグツールとしてはgood enough」という判断でマージされたが、将来的にはhash-based方式への移行が検討されうる
