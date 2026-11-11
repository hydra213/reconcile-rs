"""The result of a comparison."""

import json


class ReconciliationFailed(Exception):
    """Raised by :meth:`Result.raise_if_failed`. Holds the :class:`Result`."""

    def __init__(self, result):
        super().__init__(f"reconciliation verdict: {result.verdict}\n\n{result}")
        self.result = result


class Result:
    """What a comparison found.

    - ``verdict`` is ``"pass"``, ``"warn"``, ``"inconclusive"`` or ``"fail"``.
    - ``report`` is the full report as a dict. It has the same content as
      the ``findings.json`` file the command-line tool writes.
    - ``str(result)`` is the report as text.
    """

    def __init__(self, report, json_text, text, markdown):
        self.report = report
        self.verdict = report["verdict"]
        self._json = json_text
        self._text = text
        self._markdown = markdown

    @classmethod
    def _from_native(cls, rendered):
        json_text, text, markdown = rendered
        return cls(json.loads(json_text), json_text, text, markdown)

    @property
    def passed(self):
        """True for ``pass`` and ``warn``, like exit code 0 of the
        command-line tool. ``inconclusive`` and ``fail`` are not a pass."""
        return self.verdict in ("pass", "warn")

    def raise_if_failed(self):
        """Raise :class:`ReconciliationFailed` unless the result passed.

        Use it to stop a pipeline task on a failed reconciliation.
        """
        if not self.passed:
            raise ReconciliationFailed(self)

    def to_json(self):
        """The report as JSON text, byte-identical for the same data and
        configuration."""
        return self._json

    def to_markdown(self):
        """The report as a Markdown document."""
        return self._markdown

    def __str__(self):
        return self._text

    def __repr__(self):
        return f"<reconcilers.Result verdict={self.verdict!r}>"

    def _repr_markdown_(self):
        # Notebooks (Jupyter, Databricks) call this to display the result.
        return self._markdown
