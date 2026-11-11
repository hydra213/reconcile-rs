"""Turning what the caller passed as configuration into JSON for the
compiled library, which validates it."""

import json
import os

try:
    import tomllib  # Python 3.11 and later
except ModuleNotFoundError:  # pragma: no cover - depends on the Python version
    tomllib = None


def config_to_json(keys, config):
    """Return the configuration as JSON text.

    Exactly one of ``keys`` and ``config`` must be given.

    - ``keys`` is the shortcut for the smallest configuration: a list of
      the columns that identify a row. Every other setting keeps its
      strict default.
    - ``config`` is a path to a ``reconcile.toml`` file, or a dict with
      the same sections as that file.
    """
    if (keys is None) == (config is None):
        raise TypeError("pass either keys=[...] or config=..., not both and not neither")

    if keys is not None:
        if isinstance(keys, str):
            keys = [keys]
        return json.dumps({"comparison": {"keys": list(keys)}})

    if isinstance(config, dict):
        return json.dumps(config)

    if isinstance(config, (str, os.PathLike)):
        return json.dumps(_read_toml(config))

    raise TypeError(
        f"config must be a path to a TOML file or a dict, not {type(config).__name__}"
    )


def _read_toml(path):
    if tomllib is None:
        raise RuntimeError(
            "reading a TOML configuration file needs Python 3.11 or later; "
            "on older versions pass the configuration as a dict"
        )
    with open(path, "rb") as file:
        return tomllib.load(file)
