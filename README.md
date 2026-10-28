# mhxx-renkin-reader

MHXX の錬金結果の鑑定画面を写真から読み取って錬金時のフレームを推定するブラウザ向けのツール。

**公開ページ**: <https://skst328.github.io/mhxx-renkin-reader/>

- 画像の読み取りは Rust (WebAssembly) で、`renkin-reader/` にある。ビルドとテストは [renkin-reader/README.md](renkin-reader/README.md) を参照

## 開発

```sh
pnpm install
pnpm wasm   # renkin-reader/pkg/ (wasm と TypeScript の型) を作る。git 管理外なので、クローン直後と Rust を変えたあとに必ず実行する
pnpm dev
```

`wasm-pack` が要る。読み取りとフレーム探索は Web Worker (`src/workers/reader.worker.ts`) の中で wasm を動かす。
読み取りの準備 (埋め込んだ型紙の特徴点と見本の下ごしらえ) は最初に写真を読むときに 1 回だけ行い、2 枚目以降は使い回す。
ページが裏に回って 10 秒ほど何もしていなければ Worker を捨てて wasm のメモリを返し、次に頼んだときに作り直す。

## ライセンス

MIT License です。全文は [LICENSE](LICENSE) を参照してください。

`renkin-reader/assets/` の画像 (ビルド時に埋め込まれる見本) とそこから求めた型紙の特徴量 (`reference_features.bin`)、
`renkin-reader/tests/synthetic/` の画像 (テスト用の人工の写真) は MHXX のゲーム画面から作ったもので、MIT License の対象外です (著作権は株式会社カプコンに帰属します)。

### 使用しているソフトウェア

配布物に含まれるもの (と、移植したアルゴリズム) と、そのライセンスです。

| | ライセンス | 用途 |
|---|---|---|
| [React](https://react.dev/) | MIT | 画面 |
| [Base UI](https://base-ui.com/) | MIT | UI 部品の土台 |
| [shadcn/ui](https://ui.shadcn.com/) | MIT | UI 部品 |
| [Tailwind CSS](https://tailwindcss.com/) / tw-animate-css | MIT | スタイル |
| [class-variance-authority](https://cva.style/) | Apache-2.0 | スタイルの切り替え |
| [Lucide](https://lucide.dev/) | ISC | アイコン |
| [Geist](https://vercel.com/font) | **OFL-1.1** | フォント |
| [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen) / serde / [tsify](https://github.com/madonoharu/tsify) ほか | MIT または Apache-2.0 | Rust から WebAssembly への橋渡し |
| [OpenCV](https://github.com/opencv/opencv) | Apache-2.0 | 画像処理 (位置合わせ・読み取り) のアルゴリズム |
| [apmnnn/mhxx-rng](https://github.com/apmnnn/mhxx-rng) | MIT | 乱数と護石の決まり方 (フレームの探索) |

**Geist** は SIL Open Font License 1.1 です。ライセンス全文は
<https://github.com/vercel/geist-font> に含まれています。

**OpenCV** の処理を、結果が一致するように Rust で書き直したコード (`renkin-reader/src/vision/cv/` の各ファイル) を含みます。
該当するファイルには冒頭にその旨を記載しています。ライセンス全文と著作権表示は
[THIRD_PARTY_LICENSES/OpenCV](THIRD_PARTY_LICENSES/OpenCV) にあります。

**乱数と護石の決まり方** (`renkin-reader/src/rng.rs`、`renkin-reader/src/rng/`) は apmnnn 氏の mhxx-rng からの移植です。
MIT ライセンスの全文は `renkin-reader/src/rng/xorshift.rs` の冒頭に記載しています。
## 変更点（候補スキル追加版）

- `renkin-reader/assets/skills/` に候補スキル見本を追加（ID 200〜392、計193枚）
- `renkin-reader/src/skills.rs` に対応エントリを追加（表示名は仮の「候補XXX」）
- 見本は指定座標 `(655,165)-(800,191)` / `(655,194)-(800,218)` から切り出したグレー画像
- **注意**: 元の見本zipが鑑定結果画面ではなかったため、認識精度は保証されません。鑑定結果画面のキャプチャで差し替えることを推奨します。
