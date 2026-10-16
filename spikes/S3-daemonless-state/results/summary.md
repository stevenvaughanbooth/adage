## Latency (µs; 8 parallel PostToolUse per batch; batch decision overlaps them)

### Daemonless, in-process driver — paced (state access = lock + load + store)

| run | sessions | decide n | decide p50 / p95 / p99 (µs) | state access p95 | lock wait p95 | observe p50 / p95 (µs) | spawn-incl. decide p95 | reconstruct | load (1m before→after) |
|---|---:|---:|---|---:|---:|---|---:|---|---|
| file-slots-1-paced | 1 | 225 | 183.1 / 339.1 / 402.9 | 161.8 | 121.8 | 111.2 / 216.3 | – | 2/2 | 8.81→8.42 |
| file-slots-4-paced | 4 | 828 | 227.9 / 761.5 / 1206.0 | 282.4 | 207.6 | 117.8 / 350.6 | – | 8/8 | 9.03→8.47 |
| file-slots-16-paced | 16 | 3322 | 290.1 / 829.7 / 2577.9 | 211.8 | 141.3 | 90.6 / 312.1 | – | 32/32 | 15.85→23.43 |
| file-rename-1-paced | 1 | 225 | 1203.0 / 2373.1 / 2701.8 | 1759.5 | 309.2 | 262.1 / 673.0 | – | 2/2 | 8.18→7.61 |
| file-rename-4-paced | 4 | 828 | 529.6 / 1738.9 / 2429.0 | 1313.6 | 194.9 | 106.5 / 391.7 | – | 8/8 | 8.02→7.48 |
| file-rename-16-paced | 16 | 3322 | 549.0 / 1170.0 / 2391.8 | 912.9 | 121.6 | 91.9 / 284.1 | – | 32/32 | 28.16→27.82 |

### Daemonless, one process per hook — paced (in-child timings; spawn-incl. = parent wall clock)

| run | sessions | decide n | decide p50 / p95 / p99 (µs) | state access p95 | lock wait p95 | observe p50 / p95 (µs) | spawn-incl. decide p95 | reconstruct | load (1m before→after) |
|---|---:|---:|---|---:|---:|---|---:|---|---|
| proc-slots-1-paced | 1 | 114 | 269.8 / 538.6 / 679.0 | 169.2 | 115.3 | 99.9 / 224.7 | 7354.3 | 1/1 | 8.42→8.18 |
| proc-slots-4-paced | 4 | 393 | 141.0 / 351.8 / 638.8 | 119.5 | 78.6 | 50.2 / 194.7 | 5697.0 | 4/4 | 8.47→8.11 |
| proc-slots-16-paced | 16 | 1662 | 154.2 / 538.2 / 1524.3 | 158.5 | 85.3 | 50.2 / 190.4 | 21411.8 | 16/16 | 23.43→26.52 |
| proc-rename-1-paced | 1 | 114 | 697.3 / 1172.0 / 1377.1 | 773.7 | 115.3 | 110.0 / 289.9 | 7726.7 | 1/1 | 7.61→7.16 |
| proc-rename-4-paced | 4 | 393 | 370.1 / 982.2 / 1802.6 | 769.0 | 95.5 | 48.9 / 200.5 | 9924.5 | 4/4 | 7.48→8.81 |
| proc-rename-16-paced | 16 | 1662 | 312.2 / 877.1 / 1800.3 | 681.0 | 76.7 | 38.2 / 170.2 | 11849.8 | 16/16 | 27.82→25.84 |

### Minimal daemon — paced (decide = client connect + round trip; state access = server-side handling)

| run | sessions | decide n | decide p50 / p95 / p99 (µs) | state access p95 | lock wait p95 | observe p50 / p95 (µs) | spawn-incl. decide p95 | reconstruct | load (1m before→after) |
|---|---:|---:|---|---:|---:|---|---:|---|---|
| daemon-1-paced | 1 | 225 | 134.4 / 607.2 / 877.4 | 29.0 | 0.0 | 231.5 / 961.7 | – | 2/2 | 7.16→6.60 |
| daemon-4-paced | 4 | 828 | 93.8 / 795.6 / 2709.5 | 20.2 | 0.0 | 176.0 / 1038.0 | – | 8/8 | 8.81→11.75 |
| daemon-16-paced | 16 | 3322 | 100.4 / 495.1 / 1107.5 | 18.9 | 0.0 | 173.7 / 700.2 | – | 32/32 | 24.25→21.05 |

### Daemonless, in-process driver — stress (state access = lock + load + store)

| run | sessions | decide n | decide p50 / p95 / p99 (µs) | state access p95 | lock wait p95 | observe p50 / p95 (µs) | spawn-incl. decide p95 | reconstruct | load (1m before→after) |
|---|---:|---:|---|---:|---:|---|---:|---|---|
| file-slots-1-stress | 1 | 443 | 106.0 / 211.8 / 335.1 | 69.6 | 37.7 | 39.5 / 114.8 | – | 2/2 | 8.18→8.18 |
| file-slots-4-stress | 4 | 1647 | 154.7 / 365.1 / 474.2 | 120.2 | 78.9 | 75.2 / 198.4 | – | 8/8 | 8.11→8.02 |
| file-slots-16-stress | 16 | 6475 | 162.6 / 1219.6 / 104517.8 | 214.6 | 86.9 | 52.0 / 478.7 | – | 32/32 | 26.52→28.16 |
| file-rename-1-stress | 1 | 443 | 200.7 / 273.8 / 302.0 | 211.0 | 23.5 | 31.5 / 75.6 | – | 2/2 | 7.16→7.16 |
| file-rename-4-stress | 4 | 1647 | 313.9 / 593.7 / 945.8 | 449.5 | 82.1 | 59.4 / 176.2 | – | 8/8 | 8.81→8.81 |
| file-rename-16-stress | 16 | 6475 | 1151.4 / 6077.9 / 14058.5 | 5408.0 | 123.1 | 75.8 / 1018.0 | – | 32/32 | 25.84→24.25 |

### Daemonless, one process per hook — stress (in-child timings; spawn-incl. = parent wall clock)

| run | sessions | decide n | decide p50 / p95 / p99 (µs) | state access p95 | lock wait p95 | observe p50 / p95 (µs) | spawn-incl. decide p95 | reconstruct | load (1m before→after) |
|---|---:|---:|---|---:|---:|---|---:|---|---|
| proc-slots-1-stress | 1 | 114 | 115.3 / 345.6 / 435.0 | 94.5 | 33.1 | 41.0 / 150.9 | 4003.2 | 1/1 | 6.60→9.03 |
| proc-slots-4-stress | 4 | 393 | 160.4 / 723.2 / 3641.8 | 172.5 | 95.7 | 56.5 / 254.7 | 16142.2 | 4/4 | 11.75→15.85 |
| proc-slots-16-stress | 16 | 1662 | 127.3 / 477.3 / 1173.8 | 167.4 | 82.2 | 41.0 / 187.4 | 37080.4 | 16/16 | 21.05→19.68 |

### Minimal daemon — stress (decide = client connect + round trip; state access = server-side handling)

| run | sessions | decide n | decide p50 / p95 / p99 (µs) | state access p95 | lock wait p95 | observe p50 / p95 (µs) | spawn-incl. decide p95 | reconstruct | load (1m before→after) |
|---|---:|---:|---|---:|---:|---|---:|---|---|
| daemon-1-stress | 1 | 443 | 41.2 / 96.4 / 134.1 | 5.2 | 0.0 | 67.1 / 132.6 | – | 2/2 | 6.60→6.60 |
| daemon-4-stress | 4 | 1647 | 194.4 / 799.1 / 1411.7 | 12.5 | 0.0 | 237.6 / 802.9 | – | 8/8 | 11.75→11.75 |
| daemon-16-stress | 16 | 6475 | 889.3 / 1474.5 / 1849.4 | 17.9 | 0.0 | 913.9 / 1511.2 | – | 32/32 | 21.05→21.05 |

### Durability and control runs (paced)

| run | sessions | decide n | decide p50 / p95 / p99 (µs) | state access p95 | lock wait p95 | observe p50 / p95 (µs) | spawn-incl. decide p95 | reconstruct | load (1m before→after) |
|---|---:|---:|---|---:|---:|---|---:|---|---|
| file-slots-1-paced-fsync | 1 | 114 | 286.2 / 1563.8 / 1668.0 | 657.7 | 299.9 | 148.2 / 733.1 | – | 1/1 | 19.68→18.35 |
| file-slots-16-paced-fsync | 16 | 1662 | 222.5 / 582.3 / 950.1 | 284.0 | 144.2 | 85.2 / 301.8 | – | 16/16 | 18.35→17.28 |
| file-slots-1-paced-full | 1 | 114 | 3711.2 / 5559.2 / 9122.1 | 5060.7 | 314.5 | 207.8 / 721.6 | – | 1/1 | 17.28→16.29 |
| file-slots-16-paced-full | 16 | 1662 | 9900.7 / 20986.3 / 26116.0 | 9753.0 | 143.2 | 2964.0 / 10866.8 | – | 16/16 | 16.29→14.02 |
| file-slots-16-paced-sequential | 16 | 1662 | 224.3 / 715.0 / 895.0 | 245.8 | 169.0 | 120.3 / 410.0 | – | 16/16 | 14.02→13.13 |

### Per decision point — file-slots-16-paced

| op | n | p50 | p95 | p99 | max | records folded p95 |
|---|---:|---:|---:|---:|---:|---:|
| decide:PostToolBatch | 1742 | 262.1 | 771.9 | 2537.9 | 7179.0 | 8 |
| decide:PreToolUse(Agent) | 262 | 340.8 | 872.0 | 2577.9 | 5385.7 | 8 |
| decide:SessionStart | 32 | 464.8 | 2096.9 | 4729.3 | 4729.3 | 0 |
| decide:Stop | 512 | 317.2 | 577.0 | 2033.6 | 3703.0 | 2 |
| decide:SubagentStart | 262 | 311.2 | 818.3 | 3161.5 | 3432.2 | 0 |
| decide:UserPromptSubmit | 512 | 333.0 | 1282.2 | 2431.7 | 3302.2 | 0 |
| observe:PostToolUse | 13936 | 89.7 | 312.5 | 1512.5 | 10296.3 | 0 |
| observe:SessionEnd | 32 | 169.6 | 1744.5 | 3470.8 | 3470.8 | 0 |
| observe:SubagentStop | 262 | 170.4 | 298.6 | 1116.5 | 3000.0 | 0 |
| peek | 512 | 54.7 | 108.7 | 222.2 | 1193.0 | 0 |

### Per decision point — proc-slots-16-paced

| op | n | p50 | p95 | p99 | max | records folded p95 |
|---|---:|---:|---:|---:|---:|---:|
| decide:PostToolBatch | 866 | 153.6 | 664.0 | 1599.1 | 4933.2 | 13 |
| decide:PreToolUse(Agent) | 134 | 156.5 | 464.1 | 1315.9 | 1344.3 | 14 |
| decide:SessionStart | 16 | 244.2 | 565.4 | 565.4 | 565.4 | 0 |
| decide:Stop | 256 | 154.0 | 449.5 | 949.5 | 2465.1 | 8 |
| decide:SubagentStart | 134 | 151.4 | 467.5 | 1182.3 | 2412.8 | 0 |
| decide:UserPromptSubmit | 256 | 147.9 | 444.7 | 909.2 | 2454.0 | 0 |
| observe:PostToolUse | 6928 | 50.0 | 190.0 | 679.0 | 4816.4 | 0 |
| observe:SessionEnd | 16 | 54.2 | 203.4 | 203.4 | 203.4 | 0 |
| observe:SubagentStop | 134 | 59.9 | 198.0 | 253.2 | 596.6 | 0 |
| peek | 256 | 42.3 | 104.7 | 133.5 | 220.7 | 0 |

### Per decision point — daemon-16-paced

| op | n | p50 | p95 | p99 | max | records folded p95 |
|---|---:|---:|---:|---:|---:|---:|
| decide:PostToolBatch | 1742 | 96.8 | 620.5 | 1363.6 | 4408.2 | 0 |
| decide:PreToolUse(Agent) | 262 | 103.0 | 332.7 | 501.2 | 1593.5 | 0 |
| decide:SessionStart | 32 | 106.9 | 222.5 | 244.5 | 244.5 | 0 |
| decide:Stop | 512 | 102.7 | 418.2 | 780.2 | 1574.8 | 0 |
| decide:SubagentStart | 262 | 98.6 | 398.8 | 713.9 | 941.0 | 0 |
| decide:UserPromptSubmit | 512 | 105.1 | 435.5 | 978.0 | 2426.6 | 0 |
| observe:PostToolUse | 13936 | 174.8 | 708.5 | 1429.0 | 4381.6 | 0 |
| observe:SessionEnd | 32 | 100.7 | 478.5 | 491.4 | 491.4 | 0 |
| observe:SubagentStop | 262 | 95.7 | 403.7 | 745.8 | 957.1 | 0 |
| peek | 512 | 63.5 | 363.5 | 846.5 | 3432.4 | 0 |

### Per decision point — file-slots-16-stress

| op | n | p50 | p95 | p99 | max | records folded p95 |
|---|---:|---:|---:|---:|---:|---:|
| decide:PostToolBatch | 3375 | 180.0 | 1554.3 | 205125.0 | 536416.3 | 13 |
| decide:PreToolUse(Agent) | 510 | 186.9 | 808.0 | 6174.8 | 289471.5 | 7 |
| decide:SessionStart | 32 | 1166.8 | 2540.0 | 2794.7 | 2794.7 | 0 |
| decide:Stop | 1024 | 135.0 | 912.9 | 10283.7 | 471896.6 | 2 |
| decide:SubagentStart | 510 | 123.0 | 838.0 | 2171.1 | 207007.7 | 6 |
| decide:UserPromptSubmit | 1024 | 127.1 | 1076.2 | 3807.6 | 424573.6 | 0 |
| observe:PostToolUse | 27000 | 52.2 | 482.0 | 6833.6 | 472234.0 | 0 |
| observe:SessionEnd | 32 | 29.3 | 102.5 | 124.2 | 124.2 | 0 |
| observe:SubagentStop | 510 | 39.6 | 203.7 | 1239.6 | 152059.3 | 0 |
| peek | 1024 | 31.3 | 494.5 | 2867.2 | 425586.2 | 0 |

### Disk (16 sessions × 32 turns)

```json
{
 "sessions": 16,
 "events": 17067,
 "events_per_session": 1067,
 "spool_bytes": 4191252,
 "spool_kib_per_1000_events": 239.8,
 "json_kib_per_1000_events": 411.0,
 "raw_text_kib_per_1000_events": 155.5,
 "state_image_bytes_per_session": 1214,
 "rebuild_from_spool_us": {
  "p50": 700.1,
  "max": 815.4
 }
}
```
