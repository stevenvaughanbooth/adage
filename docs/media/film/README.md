# The Adage film

`index7.html` is the film: a deterministic Three.js scene with a `window.seek(t)` function, rendered frame by frame.
It carries the reflexes-and-instincts rebrand (see `docs/rebrand-reflexes-and-instincts.md`) and the Workshop type set
(Bricolage Grotesque, Hanken Grotesk, Red Hat Mono, referenced from `../fonts/`). Two grades live in the same file:
night (default) and day (`#day` in the URL, `HASH=day` for the renderer).

Render with headless Chromium (WebGL via SwiftShader) and encode with ffmpeg:

    STAGE=index7.html node render.cjs preview 6 31 54          # stills at those seconds -> preview-<t>.png
    STAGE=index7.html node render.cjs full 30 film-silent.mp4  # night
    STAGE=index7.html HASH=day node render.cjs full 30 film-day-silent.mp4
    python3 audio6.py soundtrack.wav                           # the stereo score (needs numpy + scipy), locked to the timeline constants in index7.html
    ffmpeg -i film-silent.mp4 -i soundtrack.wav -c:v libx264 -crf 25 -c:a aac -b:a 192k -shortest adage-film-1080p.mp4

`render.cjs` expects Playwright and a Chromium binary; adjust the two paths at the top of the file.
The timeline constants (`const T`) in `index7.html` and `audio6.py` must match (87.857 s, 112 BPM).
`index6.html` / `audio5.py` are the previous cut, kept for reference.
