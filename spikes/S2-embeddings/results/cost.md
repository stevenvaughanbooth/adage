System load (start): uptime: 21:20  up 12 days, 20:11, 2 users, load averages: 9.11 10.47 8.51 | logical_cpus=11
System load (end): uptime: 21:20  up 12 days, 20:12, 2 users, load averages: 8.42 10.15 8.47 | logical_cpus=11

| Model | Cold start, 1st run (ms) | Cold start, median of runs 2-5 (ms) | Embed p50 @1 thread (ms) | Embed p95 @1 thread (ms) | RSS after load (MB) | Determinism (same config) | Determinism (1 vs default threads) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| potion-8m | 294.7 | 299.3 | 0.042 | 0.129 | 107.8 | 0.00e+00 | 0.00e+00 |
| bge-fp32 | 269.8 | 197.4 | 6.958 | 57.062 | 366.9 | 0.00e+00 | 0.00e+00 |
| bge-int8 | 228.6 | 209.7 | 5.253 | 49.652 | 157.0 | 0.00e+00 | 0.00e+00 |
| minilm-int8 | 179.4 | 161.9 | 2.437 | 23.245 | 118.7 | 0.00e+00 | 0.00e+00 |
