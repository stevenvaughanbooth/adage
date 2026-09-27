from quayside.models_gen import Berth


def test_berth_has_length():
    assert Berth(id="b1", length_m=10.0).length_m == 10.0
