# Adage showcase: recurring work on five repositories

On 5 recurring-task families on 5 open-source repositories, with the product learning each family from 4 ordinary sessions by itself, Adage cut the cost per task by 28.2% (90% CI 25.0-31.2%). That covers 80 held-out pairs (160 sessions, 40 task variants, 5 families) with Claude Haiku 4.5. Pre-registered in [prereg.json](prereg.json) (sha256 `47125b1b2708`).

## Pooled (Claude Haiku 4.5)

| metric | control | adage | change [90% CI] | reading |
| --- | ---: | ---: | --- | --- |
| cost per task | $0.275 | $0.197 | -28.2% [-31.2, -25.0] | cut |
| tool calls | 38.5 | 28.9 | -25.1% [-28.4, -21.6] | cut |
| wall time (s) | 140 | 116 | -16.8% [-19.7, -13.7] | cut |
| hidden tests passed | 59/80 | 51/80 | -10.0 pts [-15.7, -4.3] | lower |

- **Card delivered** in 80 of 80 Adage sessions (100.0%, Wilson 90% [97, 100]%).
- **False fires:** 3 of 30 same-repository negative prompts got a card (upper bound 22.6%): 0 at the prompt, 3 at the first edit of a card file.
- **Censored at 60 turns:** 7 sessions. Without them: -25.1% [-28.5, -21.6].

## Per family (8 held-out variants x 2 repeats each)

| family | repository | ecosystem | cost per task [90% CI] | tool calls | passed (ctl / adage) | card delivered | false fires |
| --- | --- | --- | --- | --- | --- | --- | --- |
| click-type | pallets/click @ 06b2a67 (8.6.0 unreleased) | Python: uv venv, pytest | -19.4% [-24.1, -13.4] | -4.9% | 13/16 / 14/16 | 16/16 | 0/6 |
| mm-validator | marshmallow-code/marshmallow @ 7f0792bd (4.4.0 unreleased) | Python: uv venv, pytest | -30.5% [-35.5, -25.4] | -29.1% | 16/16 / 16/16 | 16/16 | 2/6 |
| go-flag | urfave/cli v3 @ 58ec406e | Go | -47.4% [-53.4, -40.6] | -39.8% | 1/16 / 1/16 | 16/16 | 0/6 |
| http-header | hyperium/http @ fdc6ab8 (1.5.0+) | Rust: cargo | -16.2% [-25.1, -5.8] | -29.3% | 14/16 / 4/16 | 16/16 | 1/6 |
| chi-middleware | go-chi/chi v5 @ 3d1777a | Go | -21.7% [-29.7, -13.4] | -20.9% | 15/16 / 16/16 | 16/16 | 0/6 |

Each family rests on 8 held-out variants; per-family intervals are about +/-10 points and cover about 88% of the time.

## A stronger model: Claude Sonnet 5 (directional)

On 12 pairs (mm-validator, chi-middleware), Adage cut the cost per task by 17.0% (90% CI 2.9-28.7%); tool calls -20.4% [-31.3, -8.9]; hidden tests passed 12/12 (control) vs 12/12 (adage).

Directional only: the Sonnet arm is reported as directional (12 pairs, about +/-12 points), with the note that it used the Haiku-learned index.

## Spend

$47.64 in total (training, held out, the Sonnet arm and any re-run included), against a pre-registered central estimate of $58.49 and a hard cap of $60.

## Caveats

- Families, prompts, hidden checks and gold patches were written by the Adage team; prompts follow one template per family, and the shipped detector recognises near-template prompts, not informal paraphrases (0/6 in the Go research family).
- The Adage arm's saving is intention-to-treat: sessions whose prompt the detector missed count, at no saving.
- Learning was frozen during held-out sessions; a user's index would keep learning from them.
- Each family rests on 8 held-out variants; per-family intervals are about +/-10 points and cover about 88% of the time.
- The pooled figure weights families by their cost; the interval is conditional on these 5 families.
- Wall time was measured on a shared machine.
- The Sonnet arm is small (12 pairs) and used cards learned from Haiku sessions.
- The go-flag family's CI requires an approval step (testdata/godoc-v3.x.txt) that no research training session performed; a card only repeats what earlier sessions did.
- 7 held-out sessions hit the 60-turn cap and are counted at their cost so far (sensitivity without them is reported).

## Data

- Pre-registration: [prereg.json](prereg.json); analysis: `analyze.py` -> `analysis.json`; this page: `render.py` -> `RESULTS.md`, `results.json`.
- Complete: {"haiku_pairs": [80, 80], "sonnet_pairs": [12, 12], "haiku_complete": true, "sonnet_complete": true}.
