| Method | attrs | click | marshmallow | urfave-cli | vue-core |
|---|---|---|---|---|---|
| literal | 1/10 (1 tasks) | 1/9 (1 tasks) | 2/6 (2 tasks) | 1/9 (1 tasks) | 1/8 (1 tasks) |
| argv | 1/10 (1 tasks) | 1/9 (1 tasks) | 2/6 (2 tasks) | 1/9 (1 tasks) | 1/8 (1 tasks) |
| headsub | 6/10 (6 tasks) ✓ | 7/9 (7 tasks) ✓ | 4/6 (4 tasks) | 9/9 (9 tasks) ✓ | 7/8 (7 tasks) ✓ |
| lgg-strict | 3/10 (3 tasks) | 2/9 (2 tasks) | 2/6 (2 tasks) | 3/9 (3 tasks) | 3/8 (3 tasks) |
| lgg-loose | 3/10 (3 tasks) | 2/9 (2 tasks) | 2/6 (2 tasks) | 6/9 (6 tasks) ✓ | 5/8 (5 tasks) ✓ |
| grammar-L1 | 9/10 (9 tasks) ✓ | 6/9 (6 tasks) ✓ | 6/6 (6 tasks) ✓ | 9/9 (9 tasks) ✓ | 7/8 (7 tasks) ✓ |
| grammar-L2 | 6/10 (6 tasks) ✓ | 3/9 (3 tasks) | 4/6 (4 tasks) | 9/9 (9 tasks) ✓ | 5/8 (5 tasks) ✓ |
| grammar-L3 | 6/10 (6 tasks) ✓ | 3/9 (3 tasks) | 3/6 (3 tasks) | 3/9 (3 tasks) | 4/8 (4 tasks) |
| grammar-L3e | 6/10 (6 tasks) ✓ | 3/9 (3 tasks) | 3/6 (3 tasks) | 3/9 (3 tasks) | 4/8 (4 tasks) |
| combo | 5/10 (5 tasks) ✓ | 3/9 (3 tasks) | 4/6 (4 tasks) | 6/9 (6 tasks) ✓ | 5/8 (5 tasks) ✓ |
| intent | 7/10 (7 tasks) ✓ | 7/9 (7 tasks) ✓ | 6/6 (6 tasks) ✓ | 6/9 (6 tasks) ✓ | 6/8 (6 tasks) ✓ |

attrs grammar-L3: test-run|system|test-id|src-test | lgg-loose: python pytest <P:test> <S> -k -m -xvs
click grammar-L3: test-run|system|file|src-test | lgg-loose: python pytest <T:test> -m -xvs
marshmallow grammar-L3: test-run|system|test-id|src-test | lgg-loose: python3 pytest <T:test> -m -xvs
urfave-cli grammar-L3: test-run|project|test-id|src-test | lgg-loose: go test <S> -run -v
vue-core grammar-L3: test-run|project|file|edited-test | lgg-loose: npm test <P:test> --
