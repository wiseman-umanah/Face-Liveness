from importlib.metadata import version as dist_version

import liveness_py


def test_version_matches_package():
    assert liveness_py.version() == dist_version("liveness-py")
