| Embedder | Case store | Mode | Scope | Ŝ calls/session [90% CI] | Directives | Useful (Wilson90 LB) | Harmful |
| --- | --- | --- | --- | --- | ---: | --- | ---: |
| hashed | off | post-hoc | all 4 (82 sessions) | **+1.71** [+1.15, +2.32] | 66 | 66 (0.96) | 0 |
| hashed | off | post-hoc | vue-core (16) | **-0.04** [-0.04, -0.03] | 16 | 16 (0.86) | 0 |
| hashed | off | pre-emptive | all 4 (82 sessions) | **+2.29** [+1.70, +2.88] | 78 | 76 (0.93) | 0 |
| hashed | off | pre-emptive | vue-core (16) | **-0.04** [-0.04, -0.03] | 16 | 16 (0.86) | 0 |
| hashed | on | post-hoc | all 4 (82 sessions) | **+1.71** [+1.15, +2.32] | 66 | 66 (0.96) | 0 |
| hashed | on | post-hoc | vue-core (16) | **-0.04** [-0.04, -0.03] | 16 | 16 (0.86) | 0 |
| hashed | on | pre-emptive | all 4 (82 sessions) | **+2.29** [+1.70, +2.88] | 78 | 76 (0.93) | 0 |
| hashed | on | pre-emptive | vue-core (16) | **-0.04** [-0.04, -0.03] | 16 | 16 (0.86) | 0 |
| hashed | ungated | post-hoc | all 4 (82 sessions) | **+2.14** [+1.57, +2.73] | 82 | 77 (0.88) | 1 |
| hashed | ungated | post-hoc | vue-core (16) | **+2.15** [+1.14, +3.27] | 32 | 27 (0.71) | 1 |
| hashed | ungated | pre-emptive | all 4 (82 sessions) | **+2.75** [+2.21, +3.29] | 94 | 87 (0.87) | 1 |
| hashed | ungated | pre-emptive | vue-core (16) | **+2.34** [+1.32, +3.42] | 32 | 27 (0.71) | 1 |
| bundled | off | post-hoc | all 4 (82 sessions) | **+1.71** [+1.15, +2.32] | 66 | 66 (0.96) | 0 |
| bundled | off | post-hoc | vue-core (16) | **-0.04** [-0.04, -0.03] | 16 | 16 (0.86) | 0 |
| bundled | off | pre-emptive | all 4 (82 sessions) | **+2.29** [+1.70, +2.88] | 78 | 76 (0.93) | 0 |
| bundled | off | pre-emptive | vue-core (16) | **-0.04** [-0.04, -0.03] | 16 | 16 (0.86) | 0 |
| bundled | on | post-hoc | all 4 (82 sessions) | **+1.73** [+1.17, +2.33] | 70 | 70 (0.96) | 0 |
| bundled | on | post-hoc | vue-core (16) | **+0.03** [-0.04, +0.16] | 20 | 20 (0.88) | 0 |
| bundled | on | pre-emptive | all 4 (82 sessions) | **+2.31** [+1.73, +2.90] | 82 | 80 (0.93) | 0 |
| bundled | on | pre-emptive | vue-core (16) | **+0.10** [-0.04, +0.34] | 20 | 20 (0.88) | 0 |
| bundled | ungated | post-hoc | all 4 (82 sessions) | **+2.13** [+1.55, +2.71] | 80 | 76 (0.89) | 1 |
| bundled | ungated | post-hoc | vue-core (16) | **+2.09** [+1.02, +3.21] | 30 | 26 (0.73) | 1 |
| bundled | ungated | pre-emptive | all 4 (82 sessions) | **+2.74** [+2.19, +3.28] | 92 | 86 (0.88) | 1 |
| bundled | ungated | pre-emptive | vue-core (16) | **+2.28** [+1.27, +3.34] | 30 | 26 (0.73) | 1 |
| potion-code-16m-v2 | off | post-hoc | all 4 (82 sessions) | **+1.71** [+1.15, +2.32] | 66 | 66 (0.96) | 0 |
| potion-code-16m-v2 | off | post-hoc | vue-core (16) | **-0.04** [-0.04, -0.03] | 16 | 16 (0.86) | 0 |
| potion-code-16m-v2 | off | pre-emptive | all 4 (82 sessions) | **+2.29** [+1.70, +2.88] | 78 | 76 (0.93) | 0 |
| potion-code-16m-v2 | off | pre-emptive | vue-core (16) | **-0.04** [-0.04, -0.03] | 16 | 16 (0.86) | 0 |
| potion-code-16m-v2 | on | post-hoc | all 4 (82 sessions) | **+1.74** [+1.17, +2.34] | 70 | 70 (0.96) | 0 |
| potion-code-16m-v2 | on | post-hoc | vue-core (16) | **+0.10** [-0.03, +0.23] | 20 | 20 (0.88) | 0 |
| potion-code-16m-v2 | on | pre-emptive | all 4 (82 sessions) | **+2.32** [+1.75, +2.90] | 82 | 80 (0.93) | 0 |
| potion-code-16m-v2 | on | pre-emptive | vue-core (16) | **+0.16** [-0.03, +0.36] | 20 | 20 (0.88) | 0 |
| potion-code-16m-v2 | ungated | post-hoc | all 4 (82 sessions) | **+2.06** [+1.48, +2.66] | 79 | 75 (0.89) | 0 |
| potion-code-16m-v2 | ungated | post-hoc | vue-core (16) | **+1.72** [+0.60, +2.91] | 29 | 25 (0.73) | 0 |
| potion-code-16m-v2 | ungated | pre-emptive | all 4 (82 sessions) | **+2.66** [+2.12, +3.21] | 91 | 85 (0.88) | 0 |
| potion-code-16m-v2 | ungated | pre-emptive | vue-core (16) | **+1.90** [+0.84, +3.09] | 29 | 25 (0.73) | 0 |
