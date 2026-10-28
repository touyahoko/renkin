"""renkin-reader のテストで使う人工の写真 (tests/synthetic/) を作る.

charm-pics の Switch のキャプチャ (ゲーム画面だけ。templates/ のうち鑑定画面のもの) を、テレビの画面として部屋の写真の中に斜めに置き、ボケ・ノイズ・
JPEG などの劣化を加えて、スマホで撮った写真の代わりにする。写真そのものは使わないので、できた画像はリポジトリに入れる
(リポジトリ単体で写真から最後まで読むテストができる)。答え (answers.csv) はキャプチャの答えから写し、
スロットと何個目かは、劣化させる前のキャプチャを Python 版で読んだ値にする。
読める画像ごとに、錬金したフレームの候補 (frames.json) も書く (読み取りからフレーム探索までのテスト用)。

作るときには charm-pics と、OpenCV の入った Python (experimental-charm-reader の venv など) が要る。
作ったあと、Python 版 (python -m charm_reader.evaluate renkin-reader/tests/synthetic) でも期待どおりか確かめる。

usage: python tools/make_synthetic.py [charm-pics のディレクトリ]
"""
import csv
import json
import sys
from pathlib import Path

import cv2
import numpy as np

ROOT = Path(__file__).resolve().parents[1]
EXPERIMENTAL = ROOT.parent / "experimental-charm-reader"
OUT = ROOT / "renkin-reader" / "tests" / "synthetic"
sys.path.insert(0, str(EXPERIMENTAL))
from charm_reader import reader as R  # noqa: E402
from charm_reader.align import Aligner  # noqa: E402

BEZEL = 40  # 画面のまわりの黒いふち (キャプチャの px)

# (ファイル名, 素材のキャプチャ (None = 画面を写さない), 置き方と劣化, 期待 ("" か "retake"), 説明)。
# 置き方: canvas (写真の幅, 高さ)、width (テレビの幅 / 写真の幅)、center (写真の中の位置の割合)、rot (傾き、度)、
# yaw (左右の辺の長さの差の割合。正なら右が近い)、pitch (上下の辺の長さの差の割合。正なら下が近い)。
# 劣化: blur (σ、写真 px)、motion (横のぶれ幅、写真 px)、noise (σ)、gamma、glare (パネルの近くの白いにじみの明るさ)、
# mirror (左右反転)、jpeg (品質)
SCENES = [
    ("01_basic.jpg", "20261010192445-01M4JN7AY3RQCRADWB5M98SGTQ.jpg",
     dict(width=0.8, rot=2, yaw=0.05, blur=1.0, noise=3), "", "正面から少し斜め"),
    ("02_oblique.jpg", "20261010192446-01M4JN7H79MXMNMND83A7YPD69.jpg",
     dict(width=0.75, rot=-4, yaw=0.35, pitch=0.08, blur=1.0, noise=3), "", "横から強く斜め"),
    ("03_portrait_tilted.jpg", "20261010192447-01M4JN806T3TJVK9MCCVD57JYH.jpg",
     dict(canvas=(2100, 2800), width=0.92, rot=12, blur=1.0, noise=3), "", "縦長の写真で画面が傾いている"),
    ("04_far.jpg", "20261010192448-01M4JN87PQ92KAMGZ65KRA0RYD.jpg",
     dict(width=0.3, center=(0.4, 0.55), blur=0.6, noise=3), "", "遠くから (画面が小さく写る)"),
    ("05_noisy_jpeg.jpg", "20261010192447-01M4JN806T3TJVK9MCCVD57JYH.jpg",
     dict(width=0.8, yaw=-0.1, blur=1.0, noise=18, jpeg=40), "", "ノイズと強い JPEG"),
    ("06_dark.jpg", "20261010192448-01M4JN87PQ92KAMGZ65KRA0RYD.jpg",
     dict(width=0.8, rot=-2, blur=1.0, noise=5, gamma=1.6), "", "暗い"),
    ("07_glare.jpg", "20261010192445-01M4JN7AY3RQCRADWB5M98SGTQ.jpg",
     dict(width=0.8, yaw=0.1, blur=1.0, noise=3, glare=130), "", "パネルの近くに反射"),
    ("08_mirrored.jpg", "20261010192446-01M4JN7H79MXMNMND83A7YPD69.jpg",
     dict(width=0.8, rot=3, yaw=0.1, blur=1.0, noise=3, mirror=True), "", "左右反転して保存された写真"),
    ("09_small_image.jpg", "20261010192447-01M4JN806T3TJVK9MCCVD57JYH.jpg",
     dict(canvas=(1600, 1200), width=0.88, rot=-1, blur=0.6, noise=3), "", "長辺 2000px 未満の写真 (拡大して使う)"),
    ("10_motion.jpg", "20261010192448-01M4JN87PQ92KAMGZ65KRA0RYD.jpg",
     dict(width=0.8, rot=1, blur=0.8, motion=7, noise=3), "", "手ぶれ"),
    ("11_too_small.jpg", "20261010192445-01M4JN7AY3RQCRADWB5M98SGTQ.jpg",
     dict(width=0.1, center=(0.6, 0.45), blur=0.6, noise=3), "retake", "画面が小さすぎる"),
    ("12_no_screen.jpg", None, dict(noise=3), "retake", "画面が写っていない"),
]
DEFAULTS = dict(canvas=(2800, 2100), width=0.8, center=(0.5, 0.5), rot=0, yaw=0.0, pitch=0.0, blur=0.0, motion=0,
                noise=0, gamma=1.0, glare=0, mirror=False, jpeg=88)


def background(w, h, rng):
    """部屋らしい背景: ゆるやかな明るさのむら + 家具のような四角 + 細かいざらつき."""
    low = rng.uniform(40, 150, (6, 8, 3)).astype(np.float32)
    img = cv2.resize(low, (w, h), interpolation=cv2.INTER_CUBIC)
    for _ in range(6):
        x0, y0 = int(rng.uniform(0, w)), int(rng.uniform(0, h))
        x1, y1 = x0 + int(rng.uniform(0.1, 0.5) * w), y0 + int(rng.uniform(0.1, 0.5) * h)
        cv2.rectangle(img, (x0, y0), (x1, y1), rng.uniform(30, 200, 3).tolist(), -1)
    img = cv2.GaussianBlur(img, (0, 0), 6)
    return img + rng.normal(0, 4, img.shape).astype(np.float32)


def tv_quad(w, h, p, aspect):
    """テレビ (ふち込み) の 4 隅 (左上・右上・右下・左下) の、写真の中の位置."""
    tw = p["width"] * w
    th = tw / aspect
    hw, hh = tw / 2, th / 2
    yaw, pitch = p["yaw"], p["pitch"]
    pts = np.float32([
        [-hw * (1 - pitch), -hh * (1 - yaw)],
        [hw * (1 - pitch), -hh * (1 + yaw)],
        [hw * (1 + pitch), hh * (1 + yaw)],
        [-hw * (1 + pitch), hh * (1 - yaw)],
    ])
    a = np.deg2rad(p["rot"])
    rot = np.float32([[np.cos(a), -np.sin(a)], [np.sin(a), np.cos(a)]])
    return pts @ rot.T + np.float32([p["center"][0] * w, p["center"][1] * h])


def render(capture, p, rng):
    w, h = p["canvas"]
    img = background(w, h, rng)
    if capture is not None:
        tv = cv2.copyMakeBorder(capture, BEZEL, BEZEL, BEZEL, BEZEL, cv2.BORDER_CONSTANT, value=(18, 18, 18))
        th, tw = tv.shape[:2]
        H = cv2.getPerspectiveTransform(np.float32([[0, 0], [tw, 0], [tw, th], [0, th]]), tv_quad(w, h, p, tw / th))
        warped = cv2.warpPerspective(tv.astype(np.float32), H, (w, h), flags=cv2.INTER_LINEAR)
        mask = cv2.warpPerspective(np.ones((th, tw), np.float32), H, (w, h), flags=cv2.INTER_LINEAR)[..., None]
        img = img * (1 - mask) + warped * mask
        if p["glare"]:
            # パネルのあたり (画面の (660, 205)) を中心に、画面で半径 90px ほどの白いにじみ
            cx, cy = cv2.perspectiveTransform(np.float32([[[660 + BEZEL, 205 + BEZEL]]]), H)[0, 0]
            r = 90 * p["width"] * w / tw
            yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
            img = img + (p["glare"] * np.exp(-((xx - cx) ** 2 + (yy - cy) ** 2) / (2 * r * r)))[..., None]
    img = np.clip(img, 0, 255)
    if p["gamma"] != 1.0:
        img = ((img / 255) ** p["gamma"]) * 255
    if p["blur"]:
        img = cv2.GaussianBlur(img, (0, 0), p["blur"])
    if p["motion"]:
        n = int(p["motion"]) | 1
        k = np.zeros((n, n), np.float32)
        k[n // 2, :] = 1 / n
        img = cv2.filter2D(img, -1, k)
    if p["noise"]:
        img = img + rng.normal(0, p["noise"], img.shape)
    img = np.clip(img, 0, 255).round().astype(np.uint8)
    if p["mirror"]:
        img = img[:, ::-1]
    return np.ascontiguousarray(img)


# 読み取りからフレーム探索までのテストで、フレームを探す範囲 [開始, 終了)
FRAME_RANGE = (0, 1_000_000)


def frames(rows):
    """読める画像ごとに、正解の護石と何個目から、錬金したフレームの候補 (Python 版の frames.py) を frames.json に書く.

    素材の 4 枚は 1 回のマカフシギ錬金の 1〜4 個目なので、4 個の候補に共通するフレームがその錬金のフレーム
    (383346。mhxx-rng で錬金した結果がキャプチャと並びまで一致する)。それも書いておく。
    素材を変えてクエスト報酬などの護石を使うと、候補が無くなることがあるので注意。
    """
    import re
    from charm_reader import frames as F
    skills = {name: int(i) for i, name in re.findall(r'\((\d+), "([^"]+)"\)',
                                                     (ROOT / "renkin-reader" / "src" / "skills.rs").read_text())}
    out = {"start": FRAME_RANGE[0], "end": FRAME_RANGE[1], "images": {}}
    for r in rows:
        if r["expect"] == "retake":
            continue
        charm = (skills[r["skill1"]], int(r["pts1"]), skills[r["skill2"]] if r["skill2"] else -1,
                 int(r["pts2"] or 0), int(r["slot"]))
        hits = F.candidate_frames(charm, int(r["index"]), FRAME_RANGE)
        out["images"][r["file"]] = sorted({f for f, _ in hits})
    common = set.intersection(*(set(v) for v in out["images"].values()))
    assert len(common) == 1, common
    out["melding"] = common.pop()
    (OUT / "frames.json").write_text(json.dumps(out, indent=1) + "\n")
    print(f"錬金したフレーム {out['melding']}、候補の数 {[len(v) for v in out['images'].values()]}")


def main(charm_pics):
    templates = Path(charm_pics) / "templates"
    with open(templates / "answers.csv", newline="") as f:
        answers = {r["file"]: r for r in csv.DictReader(f)}
    aligner = Aligner()
    OUT.mkdir(parents=True, exist_ok=True)
    rows = []
    for i, (name, src, params, expect, note) in enumerate(SCENES):
        p = {**DEFAULTS, **params}
        rng = np.random.default_rng(i)
        capture = None if src is None else cv2.imread(str(templates / src))
        cv2.imwrite(str(OUT / name), render(capture, p, rng), [cv2.IMWRITE_JPEG_QUALITY, p["jpeg"]])
        if src is None:
            # 画面が写っていないので、中身は読めない。答えの欄は形だけ (判定では使わない)
            rows.append(dict(file=name, index="", skill1="攻撃", pts1="1", skill2="", pts2="", slot="",
                             expect=expect, note=note))
            continue
        a = answers[src]
        # スロットと何個目かは、劣化させる前のキャプチャを Python 版で読んだ値 (自信ありのときだけ)
        screen, _, _ = aligner.warp(capture)
        res = R.read_screen(screen)
        assert not R.unsure_reasons(res), (src, R.unsure_reasons(res))
        index = "-" if res["index"][0] is None else res["index"][0]
        if expect == "retake":
            index = ""  # 撮り直しになるのが正しいので、何個目かは数えない
        rows.append(dict(file=name, index=index, skill1=a["skill1"], pts1=a["pts1"],
                         skill2=a["skill2"], pts2=a["pts2"], slot=res["slot"][0], expect=expect,
                         note=f"{note} (素材 {src})"))
    with open(OUT / "answers.csv", "w", newline="") as f:
        wr = csv.DictWriter(f, fieldnames=["file", "index", "skill1", "pts1", "skill2", "pts2", "slot", "expect", "note"])
        wr.writeheader()
        wr.writerows(rows)
    frames(rows)
    total = sum((OUT / r["file"]).stat().st_size for r in rows)
    print(f"{len(rows)} 枚を {OUT} に書いた (合計 {total / 1e6:.1f} MB)")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else ROOT.parent / "charm-pics")
