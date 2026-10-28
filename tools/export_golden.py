"""charm-pics の写真から、renkin-reader の charm-pics を使うテストとベンチ用のデータを tests/golden/ に書き出す.

写真から作ったデータなので、リポジトリには入れない (.gitignore で外している)。OpenCV の入った Python で動かす
(experimental-charm-reader の venv など)。書き出すもの:

- photos/<名前>.jpg: HEIC を JPEG (品質 95) にしたもの。Rust のテスト (tests/charm_pics.rs) は HEIC を開けないので使う
- photos/<名前>.png: --decoded で名前の一部を渡したときだけ。OpenCV で開いた画素 (PNG)。Rust の JPEG の開き方との
  違いを調べるとき用 (RENKIN_DECODED=1 で tests/charm_pics.rs がこちらを使う)
- bench/: 速さを測る写真 (BENCH) を RGBA の画素にしたもの (meta.json と *.rgba)。tests/profile.rs と tools/bench.mjs が使う

名前は charm-pics からの相対パスの / を __ にしたもの。

usage: python tools/export_golden.py [--decoded 名前の一部 ...] [charm-pics のディレクトリ]
"""
import json
import sys
from pathlib import Path

import cv2

ROOT = Path(__file__).resolve().parents[1]
EXPERIMENTAL = ROOT.parent / "experimental-charm-reader"
OUT = ROOT / "renkin-reader" / "tests" / "golden"
# 速さを測る写真: Pixel (近い)、iPad (HEIC)、Pixel (少し遠い)
BENCH = [
    "tests/001/PXL_20261010_101752055.jpg",
    "tests/004/IMG_0102.HEIC",
    "synthetic/photos/001/PXL_20261018_133822896.jpg",
]


def main(argv):
    decoded = []
    while argv and argv[0] == "--decoded":
        decoded.append(argv[1])
        argv = argv[2:]
    charm_pics = Path(argv[0]) if argv else ROOT.parent / "charm-pics"
    sys.path.insert(0, str(EXPERIMENTAL))
    from charm_reader.images import imread
    from charm_reader.truth import find_dirs, load_answers

    (OUT / "photos").mkdir(parents=True, exist_ok=True)
    for d in find_dirs([charm_pics / "tests", charm_pics / "synthetic"]):
        for a in load_answers(d):
            name = f"{d.relative_to(charm_pics)}/{a.path.name}"
            out = OUT / "photos" / name.replace("/", "__")
            if a.path.suffix.lower() in (".heic", ".heif"):
                cv2.imwrite(str(out) + ".jpg", imread(a.path), [cv2.IMWRITE_JPEG_QUALITY, 95])
            if any(k in name for k in decoded):
                cv2.imwrite(str(out) + ".png", imread(a.path))
                print(f"{name}: OpenCV で開いた画素を書いた")

    (OUT / "bench").mkdir(exist_ok=True)
    meta = []
    for i, name in enumerate(BENCH):
        rgba = cv2.cvtColor(imread(charm_pics / name), cv2.COLOR_BGR2RGBA)
        (OUT / "bench" / f"{i}.rgba").write_bytes(rgba.tobytes())
        meta.append({"name": name, "file": f"{i}.rgba", "width": rgba.shape[1], "height": rgba.shape[0]})
    (OUT / "bench" / "meta.json").write_text(json.dumps(meta, ensure_ascii=False) + "\n")
    print(f"{OUT} に書いた")


if __name__ == "__main__":
    main(sys.argv[1:])
