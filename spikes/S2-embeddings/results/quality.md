| Model | Norm | dim | Family top-1 (LOO) | Verb-contrast (pairwise) | Verb-contrast (multiclass) | Open-set P@90R | ARI | Purity | Noise frac |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| static:potion-base-8m | raw | 256 | 0.716 | 0.973 | 0.927 | 0.965 | 0.144 | 0.454 | 0.464 |
| static:potion-base-8m | normalized | 256 | 0.852 | 0.993 | 0.967 | 0.972 | 0.150 | 0.503 | 0.503 |
| bge-small-en-v1.5:fp32 | raw | 384 | 0.886 | 0.993 | 0.967 | 1.000 | 0.063 | 0.450 | 0.599 |
| bge-small-en-v1.5:fp32 | normalized | 384 | 0.930 | 0.993 | 0.953 | 0.998 | 0.149 | 0.573 | 0.501 |
| bge-small-en-v1.5:int8-dynamic | raw | 384 | 0.900 | 0.993 | 0.973 | 1.000 | 0.047 | 0.322 | 0.226 |
| bge-small-en-v1.5:int8-dynamic | normalized | 384 | 0.928 | 0.993 | 0.960 | 1.000 | 0.121 | 0.549 | 0.548 |
| all-MiniLM-L6-v2:int8-arm64 | raw | 384 | 0.812 | 0.973 | 0.940 | 0.989 | 0.021 | 0.316 | 0.175 |
| all-MiniLM-L6-v2:int8-arm64 | normalized | 384 | 0.880 | 0.980 | 0.920 | 0.992 | 0.159 | 0.517 | 0.475 |
