# Test fixtures

Frozen inputs the harness tests read: copies of the files they need from archived runs, under
the run's name and the same relative path (`<run>/<path>`). They never change after they are
copied; a test that needs different input gets a new fixture.

The archived runs themselves (`../results/<run>/`) are evidence, linked from the write-ups. Tests
never read them (`test_fixtures.py`), so pruning or rewriting a run cannot change a test. Tools
that take a run as input (`heldout_cohort.py` reads its frozen task material) still read
`results/`; their tests point them at the copy here.

To add one: copy the file from `../results/<run>/<path>` to `<run>/<path>` here and read it from
the test as `Path(__file__).parent / "fixtures/<run>/<path>"`.
