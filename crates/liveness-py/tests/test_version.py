from importlib.metadata import version as dist_version

import faceliveness


def test_version_matches_package():
    assert faceliveness.version() == dist_version("faceliveness")
