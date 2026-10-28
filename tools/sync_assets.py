"""renkin-reader に埋め込む見本 (renkin-reader/assets/) を、experimental-charm-reader の見本からコピーする.

見本は charm-pics のキャプチャから experimental-charm-reader の make_assets が作る。見本を作り直したら、これを動かす。
OpenCV の入った Python で動かす (experimental-charm-reader の venv など)。

usage: python tools/sync_assets.py [experimental-charm-reader の charm_reader/assets のディレクトリ]

- skills/、digits/、signs/、slots/: そのままコピー (8 ビットの灰色の PNG)
- reference.png: 位置合わせの型紙 (ui_reference.png) を灰色にしたもの (cv2.cvtColor の BGR2GRAY)
- grid_dot.png: 鑑定ボックスの交点の見本 (そのままコピー)
- label.png: 型紙 (ui_reference.png) の色チャンネルの最大値のうち、パネルのラベル (LABEL_BLOCK) のまわりに
  LABEL_MARGIN px の余白を付けて切り出したもの。読み取りはこれにトップハット (25x25) をかけてから内側を使う。
  トップハット (収縮してから膨張) が届くのは 24px までなので、余白が 24px あれば型紙全体にかけたときと同じ値になる
"""
import shutil
import sys
from pathlib import Path

import cv2

LABEL_BLOCK = (476, 166, 570, 247)  # experimental-charm-reader の reader.LABEL_BLOCK と同じ
LABEL_MARGIN = 24                   # renkin-reader の vision::read::LABEL_MARGIN と同じ
DEST = Path(__file__).resolve().parents[1] / "renkin-reader" / "assets"
DEFAULT_SRC = Path(__file__).resolve().parents[2] / "experimental-charm-reader" / "charm_reader" / "assets"


def main(src):
    for sub in ("skills", "digits", "signs", "slots"):
        if (DEST / sub).exists():
            shutil.rmtree(DEST / sub)
        (DEST / sub).mkdir(parents=True)
        for p in sorted((src / sub).glob("*.png")):
            shutil.copy2(p, DEST / sub / p.name)
    ref = cv2.imread(str(src / "ui_reference.png"))
    x0, y0, x1, y1 = LABEL_BLOCK
    m = LABEL_MARGIN
    cv2.imwrite(str(DEST / "label.png"), ref.max(axis=2)[y0 - m:y1 + m, x0 - m:x1 + m])
    # 位置合わせの型紙 (灰色)。SIFT は灰色の画像で特徴点を取る
    cv2.imwrite(str(DEST / "reference.png"), cv2.cvtColor(ref, cv2.COLOR_BGR2GRAY))
    shutil.copy2(src / "grid_dot.png", DEST / "grid_dot.png")
    print(f"{DEST} に書き出した")


if __name__ == "__main__":
    main(Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_SRC)
