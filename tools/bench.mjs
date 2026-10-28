// ブラウザ向けの wasm (pkg) を Node で動かし、準備と写真ごとの読み取りの時間を段階ごとの表にする。
// JS との境界 (wasm-bindgen、ReadResult の受け渡し) を実際の pkg で通す確認も兼ねる。
//
// 写真は tools/export_golden.py が書き出した renkin-reader/tests/golden/bench/ (meta.json と *.rgba) を使う
// (charm-pics の写真なので、charm-pics が無いと使えない)。pkg は Node 向けに作る:
//   (renkin-reader で) wasm-pack build --release --target nodejs --out-dir <pkg のディレクトリ>
//
// usage:
//   node tools/bench.mjs <pkg のディレクトリ> [--json 結果.json]   表を出す。--json なら読み取り結果と時間も書く
//   node tools/bench.mjs --compare 前.json 後.json                 2 つの結果で、読み取り結果が同じかと時間を並べる
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);

if (args[0] === '--compare') {
  const [a, b] = args.slice(1).map((p) => JSON.parse(readFileSync(p, 'utf8')));
  console.log(`準備 ${a.initMs.toFixed(0)} -> ${b.initMs.toFixed(0)} ms`);
  for (const [i, x] of a.reads.entries()) {
    const y = b.reads[i];
    const same = JSON.stringify(x.fields) === JSON.stringify(y.fields) && x.kind === y.kind;
    console.log(`${y.name}  ${x.ms.toFixed(0)} -> ${y.ms.toFixed(0)} ms  ${same ? '結果は同じ' : '結果が違う'}`);
  }
  process.exit(0);
}

const pkg = resolve(args[0]);
const jsonOut = args[1] === '--json' ? args[2] : undefined;
const benchDir = join(dirname(fileURLToPath(import.meta.url)), '../renkin-reader/tests/golden/bench');
const w = createRequire(import.meta.url)(join(pkg, 'renkin_reader.js'));
const meta = JSON.parse(readFileSync(join(benchDir, 'meta.json'), 'utf8'));

const table = (title, total, timings) => {
  console.log(`\n${title} (合計 ${total.toFixed(0)} ms)`);
  const sums = new Map();
  for (const { stage, ms } of timings) {
    const s = sums.get(stage) ?? { ms: 0, n: 0 };
    sums.set(stage, { ms: s.ms + ms, n: s.n + 1 });
  }
  for (const [stage, { ms, n }] of sums) {
    const times = n > 1 ? ` (${n} 回)` : '';
    console.log(`  ${stage.padEnd(20)} ${ms.toFixed(0).padStart(6)} ms ${((ms / total) * 100).toFixed(1).padStart(5)}%${times}`);
  }
};

let t = performance.now();
const reader = new w.Reader();
const out = { initMs: performance.now() - t, reads: [] };
table('準備 (new Reader)', out.initMs, reader.initTimings());
for (const m of meta) {
  const rgba = new Uint8Array(readFileSync(join(benchDir, m.file)));
  t = performance.now();
  const r = reader.read(rgba, m.width, m.height);
  const ms = performance.now() - t;
  table(m.name, ms, r.timings);
  const fields = r.fields && Object.fromEntries(Object.entries(r.fields).map(([k, v]) => [k, [v.value, v.confident, v.score, v.margin]]));
  out.reads.push({ name: m.name, ms, kind: r.kind, fields });
}
if (jsonOut) writeFileSync(jsonOut, JSON.stringify(out, null, 1) + '\n');
