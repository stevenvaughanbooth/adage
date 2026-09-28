// Render film/index.html frame by frame with headless Chromium (WebGL via SwiftShader) and encode with ffmpeg.
//   node render.cjs preview 2 9 ...        -> preview-<t>.png
//   node render.cjs full [fps] [out.mp4]   -> silent video
const { chromium } = require('/opt/node22/lib/node_modules/playwright');
const { spawn } = require('child_process');
const path = require('path');
const HERE = __dirname;
const FFMPEG = '/usr/local/lib/python3.11/dist-packages/imageio_ffmpeg/binaries/ffmpeg-linux-x86_64-v7.0.2';

(async () => {
  const mode = process.argv[2] || 'preview';
  const b = await chromium.launch({ executablePath: '/opt/pw-browsers/chromium', args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist', '--allow-file-access-from-files', '--font-render-hinting=none', '--disable-lcd-text'] });
  const p = await b.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  const errs = [];
  p.on('pageerror', (e) => errs.push(e.message));
  p.on('console', (m) => { if (m.type() === 'error') errs.push(m.text()); });
  await p.goto('file://' + path.join(HERE, process.env.STAGE || 'index.html') + (process.env.HASH ? '#' + process.env.HASH : ''));
  await p.waitForFunction(() => window.__ready === true, null, { timeout: 120000 });
  const info = await p.evaluate(() => window.__info || '');
  console.log('ready:', info);
  const duration = await p.evaluate(() => window.DURATION);

  if (mode === 'preview') {
    for (const t of process.argv.slice(3).map(Number)) {
      await p.evaluate((t) => window.seek(t), t);
      await p.screenshot({ path: path.join(HERE, `preview-${String(t).replace('.', '_')}.png`), type: 'png' });
      process.stdout.write(`t=${t} `);
    }
    console.log('\nerrors:', errs.length ? errs.join('\n') : 'none');
    await b.close();
    return;
  }

  const fps = Number(process.argv[3] || 30);
  const out = path.join(HERE, process.argv[4] || 'film-silent.mp4');
  const total = Math.round(duration * fps);
  const ff = spawn(FFMPEG, ['-y', '-hide_banner', '-loglevel', 'error', '-f', 'image2pipe', '-vcodec', 'png', '-framerate', String(fps), '-i', '-',
    '-c:v', 'libx264', '-preset', 'medium', '-crf', '17', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', '-r', String(fps), out]);
  ff.stderr.on('data', (d) => process.stderr.write(d));
  const t0 = Date.now();
  const from = Number(process.env.FROM || 0), to = Math.min(total, Number(process.env.TO || total));
  for (let i = from; i < to; i++) {
    await p.evaluate((t) => window.seek(t), i / fps);
    const png = await p.screenshot({ type: 'png' });
    if (!ff.stdin.write(png)) await new Promise((r) => ff.stdin.once('drain', r));
    if (i % 100 === 0) console.log(`frame ${i}/${total} · ${((Date.now() - t0) / 1000).toFixed(0)}s`);
  }
  ff.stdin.end();
  await new Promise((r) => ff.on('close', r));
  console.log('done', out, `${((Date.now() - t0) / 1000).toFixed(0)}s`, 'errors:', errs.length ? errs.join('\n') : 'none');
  await b.close();
})();
