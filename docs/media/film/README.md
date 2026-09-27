# The Adage film

`index5.html` is the film: a deterministic Three.js scene with a `window.seek(t)` function, rendered frame by frame.
Fonts are referenced from `../fonts/`. Render with headless Chromium (WebGL via SwiftShader) and encode with ffmpeg:

    node render.cjs preview 6 31 54          # stills at those seconds -> preview-<t>.png
    STAGE=index5.html node render.cjs full 30 film-silent.mp4
    python3 audio3.py soundtrack.wav         # the score, locked to the timeline constants in index5.html
    ffmpeg -i film-silent.mp4 -i soundtrack.wav -c:v libx264 -crf 24 -c:a aac -shortest adage-film-1080p.mp4

`render.cjs` expects Playwright and a Chromium binary; adjust the two paths at the top of the file.
The timeline constants (`const T`) in `index5.html` and `audio3.py` must match.
