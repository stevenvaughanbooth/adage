# README header

The README's branded header is separate from the website's evidence sharing image.
Edit `index.html`, then render from the repository root:

```sh
pnpm --dir web/site install --frozen-lockfile
node docs/media/readme-header/render.mjs
```

The renderer uses the website's existing Playwright and font dependencies. It uses installed
Google Chrome on macOS, Playwright's Chromium elsewhere, or `CHROMIUM_EXECUTABLE` when supplied.
An optional output path is the first argument. The default is `docs/assets/readme-header.jpg`.

The typography and composition come from the original end-card source,
`docs/media/film/index7.html` at commit `64d75323e88ec69515faccabbeccad9014a93f3b`.
`background.jpg` freezes that source's rails and scene at 86.5 seconds, with the text overlay
hidden, at 1200×630: the 1920×1080 scene scaled to 1200×675 and centered vertically.
This keeps the original artwork without bundling its old film engine or video masters.
The visible words, lamps and typography remain editable HTML/CSS. The install command now uses
npm, and the old binary-size claim is replaced with “prebuilt binaries.”
