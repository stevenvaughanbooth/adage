# Adage showcase: recurring work on five repositories

> **Status: SYNTHETIC.** SYNTHETIC DATA from the dry run: these numbers illustrate the pipeline, they are not results.

On 5 recurring-task families on 5 open-source repositories, with the product learning each family from 4 ordinary sessions by itself, Adage cut the cost per task by 14.7% (90% CI 9.6-19.8%). That covers 80 held-out pairs (160 sessions, 40 task variants, 5 families) with Claude Haiku 4.5. Pre-registered in [prereg.json](prereg.json) (sha256 `ed27e353aa00`).

## Pooled (Claude Haiku 4.5)

| metric | control | adage | change [90% CI] | reading |
| --- | ---: | ---: | --- | --- |
| cost per task | $0.351 | $0.300 | -14.7% [-19.8, -9.6] | cut |
| tool calls | 47.3 | 39.8 | -15.9% [-20.5, -11.1] | cut |
| wall time (s) | 197 | 172 | -12.8% [-16.4, -9.1] | cut |
| hidden tests passed | 44/80 | 41/80 | -3.8 pts [-10.0, +2.9] | no detectable change |

- **Card delivered** in 70 of 80 Adage sessions (87.5%, Wilson 90% [80, 92]%).
- **False fires:** 0 of 30 same-repository negative prompts got a card (upper bound 8.3%): 0 at the prompt, 0 at the first edit of a card file.
- **Censored at 60 turns:** 17 sessions. Without them: -8.8% [-13.1, -4.5].

## Per family (8 held-out variants x 2 repeats each)

| family | repository | ecosystem | cost per task [90% CI] | tool calls | passed (ctl / adage) | card delivered | false fires |
| --- | --- | --- | --- | --- | --- | --- | --- |
| click-type | pallets/click @ 06b2a67 (8.6.0 unreleased) | Python: uv venv, pytest | -17.3% [-29.8, -3.1] | -18.8% | 12/16 / 13/16 | 10/16 | 0/6 |
| mm-validator | marshmallow-code/marshmallow @ 7f0792bd (4.4.0 unreleased) | Python: uv venv, pytest | -13.9% [-26.7, -0.8] | -13.5% | 1/16 / 0/16 | 12/16 | 0/6 |
| go-flag | urfave/cli v3 @ 58ec406e | Go | -11.6% [-20.0, -1.7] | -12.4% | 16/16 / 14/16 | 16/16 | 0/6 |
| http-header | hyperium/http @ fdc6ab8 (1.5.0+) | Rust: cargo | -12.2% [-20.1, -5.6] | -18.1% | 11/16 / 13/16 | 16/16 | 0/6 |
| chi-middleware | go-chi/chi v5 @ 3d1777a | Go | -17.2% [-27.3, -6.4] | -15.8% | 4/16 / 1/16 | 16/16 | 0/6 |

Each family rests on 8 held-out variants; per-family intervals are about +/-10 points and cover about 88% of the time.

## A stronger model: Claude Sonnet 5 (directional)

On 12 pairs (mm-validator, chi-middleware), Adage cut the cost per task by 16.8% (90% CI 1.6-31.1%); tool calls -13.2% [-28.0, +2.3]; hidden tests passed 9/12 (control) vs 7/12 (adage).

Directional only: the Sonnet arm is reported as directional (12 pairs, about +/-12 points), with the note that it used the Haiku-learned index.

## Spend

$60.50 in total (training, held out, the Sonnet arm and any re-run included), against a pre-registered central estimate of $58.49 and a hard cap of $60.

## Caveats

- SYNTHETIC DATA from the dry run: these numbers illustrate the pipeline, they are not results.
- Families, prompts, hidden checks and gold patches were written by the Adage team; prompts follow one template per family, and the shipped detector recognises near-template prompts, not informal paraphrases (0/6 in the Go research family).
- The Adage arm's saving is intention-to-treat: sessions whose prompt the detector missed count, at no saving.
- Learning was frozen during held-out sessions; a user's index would keep learning from them.
- Each family rests on 8 held-out variants; per-family intervals are about +/-10 points and cover about 88% of the time.
- The pooled figure weights families by their cost; the interval is conditional on these 5 families.
- Wall time was measured on a shared machine.
- The Sonnet arm is small (12 pairs) and used cards learned from Haiku sessions.
- The go-flag family's CI requires an approval step (testdata/godoc-v3.x.txt) that no research training session performed; a card only repeats what earlier sessions did.
- 17 held-out sessions hit the 60-turn cap and are counted at their cost so far (sensitivity without them is reported).

## Data

- Pre-registration: [prereg.json](prereg.json); analysis: `analyze.py` -> `analysis.json`; this page: `render.py` -> `RESULTS.md`, `results.json`.
- Complete: {"haiku_pairs": [80, 80], "sonnet_pairs": [12, 12], "haiku_complete": true, "sonnet_complete": true}.
