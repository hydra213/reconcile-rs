"""The three public functions. Each one prepares its inputs in Python and
then calls the compiled library in ``_native``."""

from . import _native
from ._config import config_to_json
from ._inputs import to_source
from ._result import Result

_SCHEMA_POLICIES = ("strict", "compatible")


def schema(left, right, *, policy="strict"):
    """Compare the schemas of two datasets. No rows are compared.

    ``policy`` is ``"strict"`` (any difference fails) or ``"compatible"``
    (harmless differences, such as an added column, only warn).
    """
    if policy not in _SCHEMA_POLICIES:
        raise ValueError(f"policy must be one of {_SCHEMA_POLICIES}, not {policy!r}")

    rendered = _native.schema(to_source(left), to_source(right), policy == "strict")
    return Result._from_native(rendered)


def rowdiff(left, right, *, keys=None, config=None):
    """Compare the rows of two datasets by key. Schemas are not judged.

    Pass ``keys`` for the simple case, or ``config`` for everything else:
    a path to a ``reconcile.toml`` file, or a dict with the same sections.
    """
    rendered = _native.rowdiff(
        to_source(left), to_source(right), config_to_json(keys, config)
    )
    return Result._from_native(rendered)


def compare(left, right, *, keys=None, config=None):
    """Run the full reconciliation: schemas, rows and fingerprints.

    Pass ``keys`` for the simple case, or ``config`` for everything else:
    a path to a ``reconcile.toml`` file, or a dict with the same sections.
    """
    rendered = _native.compare(
        to_source(left), to_source(right), config_to_json(keys, config)
    )
    return Result._from_native(rendered)
