//! The compiled half of the Python package: `reconcilers._native`.
//!
//! This file is glue and nothing else. It receives the two sides and the
//! configuration from Python, calls the `reconcilers` library, and hands
//! the report back as text. All reconciliation logic lives in the library,
//! and everything Python users see lives in `python/reconcilers/*.py`.

use std::error::Error as _;

use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3_arrow::PyTable;

use reconcilers::config::CsvConfig;
use reconcilers::policy::SchemaPolicy;
use reconcilers::report::Report;
use reconcilers::schema::LogicalSchema;
use reconcilers::{Config, Dataset, Error, input};

create_exception!(
    _native,
    ReconcileError,
    PyException,
    "The comparison could not run: unreadable input or invalid configuration."
);

/// One side of a comparison, as Python hands it over.
///
/// `#[derive(FromPyObject)]` tries the variants in order: a list of paths
/// first, then any object that can export Arrow data.
#[derive(FromPyObject)]
enum Source {
    /// Paths or URLs of the files that together form the dataset.
    Files(Vec<String>),
    /// Arrow data in memory: a pyarrow table, a Polars dataframe, or any
    /// other object with the `__arrow_c_stream__` method.
    Table(PyTable),
}

impl Source {
    /// The schema only. For files, no rows are decoded.
    fn read_schema(self, csv: &CsvConfig) -> Result<LogicalSchema, Error> {
        match self {
            Self::Files(locations) => input::read_schema(&locations, csv),
            Self::Table(table) => Ok(table_to_dataset(table)?.schema),
        }
    }

    fn read_dataset(self, csv: &CsvConfig) -> Result<Dataset, Error> {
        match self {
            Self::Files(locations) => input::read_dataset(&locations, csv),
            Self::Table(table) => table_to_dataset(table),
        }
    }
}

fn table_to_dataset(table: PyTable) -> Result<Dataset, Error> {
    let (batches, schema) = table.into_inner();
    Dataset::from_batches(&schema, batches)
}

/// A report in the three forms Python needs: `(json, text, markdown)`.
type RenderedReport = (String, String, String);

fn render(report: &Report) -> RenderedReport {
    (report.to_json(), report.to_text(), report.to_markdown())
}

/// Turns a library error into a Python `ReconcileError`. The message holds
/// the whole chain of causes, like the command-line tool prints it.
fn to_python_error(error: Error) -> PyErr {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }
    ReconcileError::new_err(message)
}

/// Compares the schemas only.
#[pyfunction]
fn schema(left: Source, right: Source, strict: bool) -> PyResult<RenderedReport> {
    let policy = if strict {
        SchemaPolicy::Strict
    } else {
        SchemaPolicy::Compatible
    };

    // No configuration is passed here, so CSV files are read with the
    // default settings: every column is text.
    let csv = CsvConfig::default();
    let left = left.read_schema(&csv).map_err(to_python_error)?;
    let right = right.read_schema(&csv).map_err(to_python_error)?;
    Ok(render(&reconcilers::compare_schemas(&left, &right, policy)))
}

/// Compares the rows only.
#[pyfunction]
fn rowdiff(
    py: Python<'_>,
    left: Source,
    right: Source,
    config_json: &str,
) -> PyResult<RenderedReport> {
    run(py, left, right, config_json, reconcilers::compare_rows)
}

/// Runs the full reconciliation.
#[pyfunction]
fn compare(
    py: Python<'_>,
    left: Source,
    right: Source,
    config_json: &str,
) -> PyResult<RenderedReport> {
    run(py, left, right, config_json, reconcilers::compare)
}

/// The shared body of `rowdiff` and `compare`, which differ only in the
/// library function they call.
fn run(
    py: Python<'_>,
    left: Source,
    right: Source,
    config_json: &str,
    compare_datasets: fn(&Config, &Dataset, &Dataset) -> Result<Report, Error>,
) -> PyResult<RenderedReport> {
    // `detach` releases Python's global lock while Rust works, so other
    // Python threads keep running during a long comparison.
    py.detach(|| {
        let config = Config::from_json_str(config_json)?;
        let left = left.read_dataset(&config.csv)?;
        let right = right.read_dataset(&config.csv)?;
        let report = compare_datasets(&config, &left, &right)?;
        Ok(render(&report))
    })
    .map_err(to_python_error)
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(schema, module)?)?;
    module.add_function(wrap_pyfunction!(rowdiff, module)?)?;
    module.add_function(wrap_pyfunction!(compare, module)?)?;
    module.add("ReconcileError", module.py().get_type::<ReconcileError>())?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
