# renkin-reader

MHXX の錬金結果の鑑定画面を写真から読む Rust のクレート。ブラウザでは wasm (wasm-pack) として使う。

## テスト

テストは 2 種類に分かれる。

- **リポジトリだけで動くもの**: `cargo test` で毎回流す
- **charm-pics (実際の写真。公開しないリポジトリ) を使うもの**: `--ignored` を付けたときだけ動く。charm-pics やそこから書き出したデータが無ければ、何もせずに通る

どちらも wasm の中でも流せる (下の「wasm で流す」)。

### リポジトリだけで動くもの (`cargo test --release`)

| 何を確かめるか | どこ | 正解 |
|---|---|---|
| 画像処理の部品 (色、ぼかし、膨張・収縮、縮小・拡大、照合、連結成分、射影変換、行列など) が OpenCV / NumPy と同じ | `src/vision/cv/*.rs` | `tests/fixtures/cv/` |
| PNG の読み込み | `src/vision/cv/image.rs` | `tests/fixtures/load/` |
| 見本の下ごしらえが Python 版と同じ | `src/vision/read.rs` | `tests/fixtures/read/` |
| 型紙の SIFT が NumPy 版と bit-exact、OpenCV と特徴点が全部・記述子が 1402 / 1415 同じ | `src/vision/cv/sift.rs` | `tests/fixtures/sift/` |
| RANSAC と最小二乗がインライアの判定まで NumPy 版と同じ | `src/vision/cv/ransac.rs` | `tests/fixtures/cv/ransac.json` |
| 錬金で出る護石とフレームの探索が mhxx-rng (ノートブック) と同じ (2 種類の表の全スキルを最低 1 回通す)。探索は 1 フレームずつ素直に作ったものとも同じ。mhxx-rng の「有名なお守り」6 個を 0〜10 億フレームで探して、Python 版と同じで当たりフレームの数も一覧と同じ (これだけで 20 秒ほど) | `src/rng.rs` | `tests/fixtures/rng/` |
| 人工の写真 12 枚を写真から最後まで読んで、期待どおり (読める 10 枚、撮り直し 2 枚) | `tests/synthetic.rs` | `tests/synthetic/answers.csv` |
| 読み取りからフレーム探索まで: 読める 10 枚を読み、読んだ護石と何個目でフレーム (0〜100 万) を探して、候補が期待どおり。素材の 4 枚は 1 回のマカフシギ錬金の 1〜4 個目なので、全部の候補に共通するのはその錬金のフレーム 383346 だけ (mhxx-rng で錬金した結果がキャプチャと並びまで一致する) | `tests/synthetic.rs` | `tests/synthetic/frames.json` |
| そのほか (埋め込んだ見本、スキル表、入力の長さ、ぼかしの足し算の順番、記述子の距離の SIMD 版) | 各ファイル | — |

正解の作り方 (OpenCV の入った Python で動かす。experimental-charm-reader の venv など):

- `tests/fixtures/`: `python tools/make_fixtures.py`。入力は乱数の小さな画像と見本 (`assets/`) と人工の写真で、写真は使わない。
  `tests/fixtures/rng/` は mhxx-rng のノートブック (リポジトリの隣に clone した mhxx-rng。experimental-charm-reader の
  charm_reader/mhxx_rng.py を参照) と numba も要る
- `tests/synthetic/`: `python tools/make_synthetic.py`。charm-pics のキャプチャ (鑑定画面のゲーム画面だけ) を部屋の写真の中に斜めに置き、ボケ・ノイズ・JPEG・反射・左右反転などを加えたもの。何度作っても同じ画像になる。作ったら Python 版でも期待どおりか確かめる (`python -m charm_reader.evaluate <このディレクトリ>/tests/synthetic`)

### charm-pics を使うもの (`--ignored`)

| 何を確かめるか / 測るか | 動かし方 |
|---|---|
| charm-pics の写真 288 枚を写真から最後まで読んで、期待どおり (`tests/charm_pics.rs`) | `cargo test --release --test charm_pics -- --ignored --nocapture` (3 スレッドで 100 秒ほど) |
| 段階ごとの時間 (ネイティブ、`tests/profile.rs`) | `cargo test --release --test profile -- --ignored --nocapture` |
| 段階ごとの時間 (wasm。JS との境界も通る) | `wasm-pack build --release --target nodejs --out-dir <pkg>` してから `node ../tools/bench.mjs <pkg> [--json 結果.json]`。2 つの結果は `node ../tools/bench.mjs --compare 前.json 後.json` で比べる (読み取り結果が同じかと時間) |

- charm-pics の場所は環境変数 `CHARM_PICS` (無ければ、このリポジトリの隣の `charm-pics`)
- 写真から作ったデータは `tests/golden/` (gitignore) に `python tools/export_golden.py` で書き出す: HEIC を JPEG にしたもの (Rust で HEIC を開けないため) と、時間を測る写真を RGBA にしたもの
- 期待と違うことがわかっていて許しているものは、`tests/charm_pics.rs` の `KNOWN` に理由と一緒に書く (今は JPEG の開き方の違いで読めない 1 枚)

### 写真から最後まで読むテストの道具

`tests/support/e2e.rs` (両方のテストで共通)。answers.csv の書き方と判定は experimental-charm-reader の evaluate と同じ。
調べるときに使える環境変数:

- `RENKIN_ONLY=<文字列>`: 名前にその文字列を含む画像だけ
- `RENKIN_LIMIT=<n>`: 先頭の n 枚だけ
- `RENKIN_DUMP=<ディレクトリ>`: 位置合わせ後の画面を PNG で書き出す
- `RENKIN_DECODED=1` (charm_pics だけ): OpenCV で開いた画素 (`export_golden.py --decoded <名前の一部>` で書き出す) を使う

### wasm で流す

ブラウザと同じ simd128 のコードを、wasm32-wasip1 向けに作って Node の WASI で動かす (`tests/wasi-run.mjs`、
`.cargo/config.toml` の runner)。最初に `rustup target add wasm32-wasip1` が要る。スレッドは使えないので 1 本で読む。

```sh
cargo test --release --target wasm32-wasip1                                    # リポジトリだけで動くもの
cargo test --release --target wasm32-wasip1 --test charm_pics -- --ignored --nocapture   # charm-pics (数分)
```

JS との境界 (wasm-bindgen、TypeScript の型) はこれでは通らない。そこは `tools/bench.mjs` が実際の pkg で通す。
